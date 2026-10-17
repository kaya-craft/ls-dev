//! Scan of the repositories: snapshot types, eligibility for the grouped
//! removal and the orchestration that publishes `worktrees:changed`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::discover;
use super::inspect::{inspect, linked_worktrees, RepoContext, UpstreamState, WorktreeInfo};
use super::state::WorktreesState;
use crate::core::settings::Settings;
use crate::core::state::CoreState;
use crate::tools::services::state::ServicesState;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesSnapshot {
    /// Unix milliseconds of the last finished scan; 0 before the first one.
    pub taken_at: u64,
    pub scanning: bool,
    pub repos: Vec<RepoGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoGroup {
    pub path: String,
    /// Last segment of the repository path.
    pub name: String,
    /// Size of the main checkout; None until measured.
    pub main_size: Option<u64>,
    pub worktrees: Vec<WorktreeInfo>,
}

/// Why a worktree is not part of "Remove all merged"; None when it is.
/// One source for the verdict and the text shown to the user.
pub fn skip_reason(info: &WorktreeInfo, has_service: bool) -> Option<&'static str> {
    if !info.trusted {
        return Some("repository config not trusted");
    }
    let Some(changes) = info.changes else {
        return Some("local state unknown");
    };
    if changes > 0 {
        return Some("has local changes");
    }
    if matches!(info.upstream, UpstreamState::Tracking { ahead } if ahead > 0) {
        return Some("has unpushed commits");
    }
    if !info.merged {
        return Some("not merged");
    }
    if has_service {
        return Some("a service is running");
    }
    None
}

/// Eligible for "Remove all merged": trusted, merged, no local change, no
/// unpushed commit and no running service.
pub fn eligible(info: &WorktreeInfo, has_service: bool) -> bool {
    skip_reason(info, has_service).is_none()
}

/// True for the caller that must run the scan; false while one is running.
pub fn try_begin_scan(scanning: &AtomicBool) -> bool {
    scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// Blocking: filesystem walk and git calls. `size_of` supplies the sizes
/// already known (path -> bytes). Repositories without linked worktree are
/// left out.
pub fn run_scan(
    home: &Path,
    settings: &Settings,
    service_cwds: &[PathBuf],
    size_of: impl Fn(&str) -> Option<u64>,
) -> Vec<RepoGroup> {
    let roots: Vec<PathBuf> = settings.worktree_roots.iter().map(PathBuf::from).collect();
    let excludes: Vec<PathBuf> = settings
        .worktree_excludes
        .iter()
        .map(PathBuf::from)
        .collect();
    let mut groups = Vec::new();
    for repo in discover::repositories(home, &roots, service_cwds, &excludes) {
        // First, and alone: a repository without linked worktree costs one git call.
        let entries = linked_worktrees(&repo);
        if entries.is_empty() {
            continue;
        }
        let ctx = RepoContext::load(&repo);
        let worktrees: Vec<WorktreeInfo> = entries
            .iter()
            .map(|entry| {
                let mut info = inspect(&repo, entry, &ctx);
                info.size_bytes = size_of(&info.path);
                info
            })
            .collect();
        let path = repo.to_string_lossy().into_owned();
        groups.push(RepoGroup {
            name: repo
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone()),
            main_size: size_of(&path),
            path,
            worktrees,
        });
    }
    groups
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Copy of the current snapshot; a poisoned lock is read anyway, so a panic
/// elsewhere cannot stop the size worker.
fn current_snapshot(state: &WorktreesState) -> WorktreesSnapshot {
    state
        .snapshot
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Sends the current snapshot to the frontend.
pub fn publish(app: &AppHandle, state: &WorktreesState) {
    let _ = app.emit("worktrees:changed", &current_snapshot(state));
}

fn report_error(app: &AppHandle, message: &str) {
    let _ = app.emit("worktrees:error", serde_json::json!({ "message": message }));
}

/// Every path whose size the snapshot shows: main checkouts and worktrees.
fn sized_paths(repos: &[RepoGroup]) -> Vec<String> {
    repos
        .iter()
        .flat_map(|r| {
            std::iter::once(r.path.clone()).chain(r.worktrees.iter().map(|w| w.path.clone()))
        })
        .collect()
}

/// Clears `scanning` when dropped, panic included.
struct ScanGuard<'a>(&'a AtomicBool);

impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Runs `pass` unless a scan is already running; then returns at once, after
/// asking the running one to run once more when it finishes: a caller that
/// just changed the repositories must not be answered with a result that
/// predates its change.
async fn coalesced<F, Fut>(state: &WorktreesState, mut pass: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Request first, then try to begin: a request that fails to begin is
    // seen by the running scan, which clears `scanning` before reading it.
    state.rescan_pending.store(true, Ordering::SeqCst);
    if !try_begin_scan(&state.scanning) {
        return;
    }
    let mut guard = ScanGuard(&state.scanning);
    loop {
        state.rescan_pending.store(false, Ordering::SeqCst);
        pass().await;
        drop(guard);
        if state.rescan_pending.load(Ordering::SeqCst) && try_begin_scan(&state.scanning) {
            guard = ScanGuard(&state.scanning);
            continue;
        }
        break;
    }
}

/// Scans, stores the snapshot, queues the stale sizes and publishes.
pub async fn scan_and_publish(
    app: &AppHandle,
    core: &Arc<CoreState>,
    services: &Arc<ServicesState>,
    state: &Arc<WorktreesState>,
) {
    coalesced(state, || scan_pass(app, core, services, state)).await;
}

async fn scan_pass(
    app: &AppHandle,
    core: &Arc<CoreState>,
    services: &Arc<ServicesState>,
    state: &Arc<WorktreesState>,
) {
    state.snapshot.write().unwrap().scanning = true;
    publish(app, state);

    let settings = core.settings.read().unwrap().clone();
    let cwds: Vec<PathBuf> = services
        .snapshot
        .read()
        .unwrap()
        .processes
        .iter()
        .filter_map(|p| p.cwd.as_deref().map(PathBuf::from))
        .collect();
    let job_state = state.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Err("HOME is not set".to_string());
        };
        // A size past its TTL stays visible until the worker replaces it.
        let previous: HashMap<String, u64> = {
            let snapshot = job_state.snapshot.read().unwrap();
            snapshot
                .repos
                .iter()
                .flat_map(|r| {
                    std::iter::once((r.path.clone(), r.main_size))
                        .chain(r.worktrees.iter().map(|w| (w.path.clone(), w.size_bytes)))
                })
                .filter_map(|(p, s)| s.map(|s| (p, s)))
                .collect()
        };
        let now = Instant::now();
        Ok(run_scan(&home, &settings, &cwds, |p| {
            job_state
                .sizes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(p, now)
                .or_else(|| previous.get(p).copied())
        }))
    })
    .await;

    match result {
        Ok(Ok(mut repos)) => {
            let now = Instant::now();
            let stale = {
                let cache = state.sizes.lock().unwrap_or_else(|e| e.into_inner());
                // A worker result that landed during the scan is not lost.
                for repo in &mut repos {
                    if repo.main_size.is_none() {
                        repo.main_size = cache.get(&repo.path, now);
                    }
                    for wt in &mut repo.worktrees {
                        if wt.size_bytes.is_none() {
                            wt.size_bytes = cache.get(&wt.path, now);
                        }
                    }
                }
                cache.stale(&sized_paths(&repos), now)
            };
            {
                let mut snapshot = state.snapshot.write().unwrap();
                snapshot.repos = repos;
                snapshot.taken_at = now_ms();
                snapshot.scanning = false;
            }
            state.enqueue_sizes(stale);
        }
        Ok(Err(message)) => {
            state.snapshot.write().unwrap().scanning = false;
            report_error(app, &message);
        }
        Err(e) => {
            state.snapshot.write().unwrap().scanning = false;
            report_error(app, &format!("scan failed: {e}"));
        }
    }
    publish(app, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::Settings;
    use crate::tools::worktrees::inspect::{UpstreamState, WorktreeInfo};
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::atomic::AtomicBool;

    fn info(
        merged: bool,
        changes: Option<u32>,
        upstream: UpstreamState,
        trusted: bool,
    ) -> WorktreeInfo {
        WorktreeInfo {
            path: "/x/wt".into(),
            branch: Some("b".into()),
            last_commit_at: None,
            changes,
            upstream,
            merged,
            trusted,
            size_bytes: None,
        }
    }

    #[test]
    fn eligibility_truth_table() {
        let t = UpstreamState::Tracking { ahead: 0 };
        assert!(eligible(&info(true, Some(0), t.clone(), true), false));
        assert!(eligible(
            &info(true, Some(0), UpstreamState::Gone, true),
            false
        ));
        assert!(eligible(
            &info(true, Some(0), UpstreamState::None, true),
            false
        ));
        assert!(!eligible(&info(true, Some(0), t.clone(), true), true));
        assert!(!eligible(&info(false, Some(0), t.clone(), true), false));
        assert!(!eligible(&info(true, Some(2), t.clone(), true), false));
        assert!(!eligible(&info(true, None, t.clone(), true), false));
        assert!(!eligible(&info(true, Some(0), t.clone(), false), false));
        assert!(!eligible(
            &info(true, Some(0), UpstreamState::Tracking { ahead: 1 }, true),
            false
        ));
    }

    #[test]
    fn skip_reasons_use_the_documented_texts() {
        let t = UpstreamState::Tracking { ahead: 0 };
        let reason = |i: WorktreeInfo, s: bool| skip_reason(&i, s);
        assert_eq!(reason(info(true, Some(0), t.clone(), true), false), None);
        assert_eq!(
            reason(info(true, Some(0), t.clone(), false), false),
            Some("repository config not trusted")
        );
        assert_eq!(
            reason(info(true, None, t.clone(), true), false),
            Some("local state unknown")
        );
        assert_eq!(
            reason(info(true, Some(1), t.clone(), true), false),
            Some("has local changes")
        );
        assert_eq!(
            reason(
                info(true, Some(0), UpstreamState::Tracking { ahead: 2 }, true),
                false
            ),
            Some("has unpushed commits")
        );
        assert_eq!(
            reason(info(false, Some(0), t.clone(), true), false),
            Some("not merged")
        );
        assert_eq!(
            reason(info(true, Some(0), t, true), true),
            Some("a service is running")
        );
    }

    #[test]
    fn a_request_during_a_scan_triggers_exactly_one_more_pass() {
        let state = WorktreesState::new();
        let passes = std::cell::Cell::new(0u32);
        tauri::async_runtime::block_on(coalesced(&state, || async {
            passes.set(passes.get() + 1);
            if passes.get() == 1 {
                // Three requests while the first pass runs: all return at once
                // and none runs a pass of its own.
                for _ in 0..3 {
                    coalesced(&state, || async { panic!("must not run") }).await;
                }
            }
        }));
        assert_eq!(passes.get(), 2);
        assert!(!state.scanning.load(Ordering::SeqCst));
        // Idle again: a new request runs a pass.
        tauri::async_runtime::block_on(coalesced(&state, || async {
            passes.set(passes.get() + 1);
        }));
        assert_eq!(passes.get(), 3);
    }

    #[test]
    fn a_panicking_pass_does_not_leave_scanning_set() {
        let state = WorktreesState::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tauri::async_runtime::block_on(coalesced(&state, || async { panic!("boom") }));
        }));
        assert!(result.is_err());
        assert!(!state.scanning.load(Ordering::SeqCst));
    }

    #[test]
    fn only_one_scan_may_begin_at_a_time() {
        let flag = AtomicBool::new(false);
        assert!(try_begin_scan(&flag));
        assert!(!try_begin_scan(&flag));
        flag.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(try_begin_scan(&flag));
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn run_scan_groups_linked_worktrees_by_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap().join("home");
        let repo = home.join("proj");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        for name in ["wt-a", "wt-b"] {
            git(
                &repo,
                &["worktree", "add", "-q", "-b", name, &format!("../{name}")],
            );
        }
        // A repository without linked worktree has nothing to list.
        let lonely = home.join("lonely");
        fs::create_dir_all(&lonely).unwrap();
        git(&lonely, &["init", "-q"]);

        let sized = repo.to_string_lossy().to_string();
        let groups = run_scan(&home, &Settings::default(), &[], |p| {
            (p == sized).then_some(7)
        });
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.name, "proj");
        assert_eq!(g.path, repo.to_string_lossy());
        assert_eq!(g.main_size, Some(7));
        let mut branches: Vec<_> = g
            .worktrees
            .iter()
            .map(|w| w.branch.clone().unwrap())
            .collect();
        branches.sort();
        assert_eq!(branches, vec!["wt-a", "wt-b"]);
        assert!(g.worktrees.iter().all(|w| w.size_bytes.is_none()));
    }

    #[test]
    fn the_snapshot_is_readable_after_a_lock_poisoning() {
        let state = WorktreesState::new();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.snapshot.write().unwrap();
            panic!("poison");
        }));
        assert!(state.snapshot.is_poisoned());
        assert_eq!(current_snapshot(&state).taken_at, 0);
    }

    #[test]
    fn snapshot_serializes_in_camel_case() {
        let s = WorktreesSnapshot {
            taken_at: 5,
            scanning: true,
            repos: vec![RepoGroup {
                path: "/r".into(),
                name: "r".into(),
                main_size: None,
                worktrees: vec![],
            }],
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"takenAt":5,"scanning":true,"repos":[{"path":"/r","name":"r","mainSize":null,"worktrees":[]}]}"#
        );
    }
}
