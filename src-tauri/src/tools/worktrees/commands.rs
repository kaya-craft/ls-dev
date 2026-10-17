//! Tauri commands of the Worktrees tool.

use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;

use super::inspect::{inspect, linked_worktrees, RepoContext};
use super::remove::{self as worktree_actions, RemoveError};
use super::scan::{eligible, scan_and_publish, skip_reason, RepoGroup, WorktreesSnapshot};
use super::state::WorktreesRef;
use crate::core::git::Worktree;
use crate::core::kill::kill_tree;
use crate::core::shell::{run_launch, shell_in_dir, terminal_launch};
use crate::core::state::CoreRef;
use crate::tools::services::model::ProcessService;
use crate::tools::services::state::{ServicesRef, ServicesState};

fn remove_error_text(e: RemoveError) -> String {
    match e {
        RemoveError::NeedsForce(m) => format!("needs-force:{m}"),
        RemoveError::Untrusted(m) => format!("untrusted:{m}"),
        RemoveError::Other(m) => m,
    }
}

/// Takes the worktree itself: after the first (refused) call the services
/// may be gone, and a lookup by service id would fail on the forced retry.
/// A non-forced call checks `git status` before stopping anything.
#[tauri::command]
pub async fn remove_worktree(
    app: AppHandle,
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    state: WorktreesRef<'_>,
    worktree: Worktree,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    if !force {
        let wt = worktree.clone();
        tauri::async_runtime::spawn_blocking(move || worktree_actions::check_clean(&wt))
            .await
            .map_err(|e| e.to_string())?
            .map_err(remove_error_text)?;
    }
    let processes = services.snapshot.read().unwrap().processes.clone();
    for service in worktree_actions::services_in(&worktree.path, &processes) {
        services.suppress_id(&service.id);
        // Err: the service already exited or its pid was reused, and nothing
        // was killed. Either way it no longer holds the worktree.
        let Ok(survivors) = kill_tree(service.root_pid, &service.command).await else {
            continue;
        };
        if !survivors.is_empty() {
            return Err(format!(
                "Could not stop {} (pids {survivors:?}); worktree kept.",
                service.project
            ));
        }
    }
    tauri::async_runtime::spawn_blocking(move || worktree_actions::remove(&worktree, force))
        .await
        .map_err(|e| e.to_string())?
        .map_err(remove_error_text)?;
    services.refresh.notify_one();
    spawn_scan(&app, &core, &services, &state);
    Ok(())
}

/// Opens the terminal chosen in Settings in the main worktree, with the
/// removal command printed but not run (untrusted repositories).
#[tauri::command]
pub async fn open_worktree_terminal(core: CoreRef<'_>, worktree: Worktree) -> Result<(), String> {
    let terminal = core.settings.read().unwrap().terminal.clone();
    let launch = terminal_launch(
        &terminal,
        &worktree_actions::manual_remove_command(&worktree),
    );
    tauri::async_runtime::spawn_blocking(move || run_launch(launch))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Could not open the terminal ({e}). Check Settings."))
}

/// Opens the terminal chosen in Settings with a shell in `path`.
#[tauri::command]
pub async fn open_worktree_shell(core: CoreRef<'_>, path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("{path} no longer exists."));
    }
    let terminal = core.settings.read().unwrap().terminal.clone();
    let launch = terminal_launch(&terminal, &shell_in_dir(&path));
    tauri::async_runtime::spawn_blocking(move || run_launch(launch))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Could not open the terminal ({e}). Check Settings."))
}

/// Scan in the background: the caller does not wait for it.
fn spawn_scan(
    app: &AppHandle,
    core: &CoreRef<'_>,
    services: &ServicesRef<'_>,
    state: &WorktreesRef<'_>,
) {
    let (app, core, services, state) = (
        app.clone(),
        core.inner().clone(),
        services.inner().clone(),
        state.inner().clone(),
    );
    tauri::async_runtime::spawn(async move {
        scan_and_publish(&app, &core, &services, &state).await;
    });
}

#[tauri::command]
pub fn worktrees_get(state: WorktreesRef<'_>) -> WorktreesSnapshot {
    state.snapshot.read().unwrap().clone()
}

/// Returns once the scan is done, or at once when one is already running.
#[tauri::command]
pub async fn worktrees_scan(
    app: AppHandle,
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    state: WorktreesRef<'_>,
) -> Result<(), String> {
    let (core, services, state) = (
        core.inner().clone(),
        services.inner().clone(),
        state.inner().clone(),
    );
    scan_and_publish(&app, &core, &services, &state).await;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkippedWorktree {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RemoveMergedReport {
    pub removed: Vec<String>,
    pub skipped: Vec<SkippedWorktree>,
}

fn remove_error_reason(e: RemoveError) -> String {
    match e {
        RemoveError::NeedsForce(_) => "has local changes".into(),
        RemoveError::Untrusted(m) | RemoveError::Other(m) => m,
    }
}

/// Removes the listed worktrees one by one. Each path is looked up in the
/// last snapshot for its repository only; everything else (branch state,
/// local changes, services) is read again from git and from `processes`
/// right before the removal. Never forces.
pub fn remove_merged_blocking(
    repos: &[RepoGroup],
    paths: &[String],
    processes: impl Fn() -> Vec<ProcessService>,
) -> RemoveMergedReport {
    let mut report = RemoveMergedReport::default();
    for path in paths {
        let mut skip = |reason: &str| {
            report.skipped.push(SkippedWorktree {
                path: path.clone(),
                reason: reason.to_string(),
            });
        };
        let Some(repo) = repos
            .iter()
            .find(|r| r.worktrees.iter().any(|w| &w.path == path))
        else {
            skip("unknown worktree");
            continue;
        };
        let repo_path = std::path::Path::new(&repo.path);
        let Some(entry) = linked_worktrees(repo_path)
            .into_iter()
            .find(|e| &e.path == path)
        else {
            skip("worktree no longer exists");
            continue;
        };
        let info = inspect(repo_path, &entry, &RepoContext::load(repo_path));
        let has_service = !worktree_actions::services_in(path, &processes()).is_empty();
        if !eligible(&info, has_service) {
            skip(skip_reason(&info, has_service).unwrap_or("not eligible"));
            continue;
        }
        let wt = Worktree {
            branch: entry.branch.clone().unwrap_or_else(|| "detached".into()),
            path: path.clone(),
            main_path: repo.path.clone(),
        };
        let outcome =
            worktree_actions::check_clean(&wt).and_then(|()| worktree_actions::remove(&wt, false));
        match outcome {
            Ok(()) => report.removed.push(path.clone()),
            Err(e) => skip(&remove_error_reason(e)),
        }
    }
    report
}

#[tauri::command]
pub async fn worktrees_remove_merged(
    app: AppHandle,
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    state: WorktreesRef<'_>,
    paths: Vec<String>,
) -> Result<RemoveMergedReport, String> {
    let repos = state.snapshot.read().unwrap().repos.clone();
    let services_state: Arc<ServicesState> = services.inner().clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        remove_merged_blocking(&repos, &paths, || {
            services_state.snapshot.read().unwrap().processes.clone()
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    if !report.removed.is_empty() {
        services.refresh.notify_one();
    }
    spawn_scan(&app, &core, &services, &state);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::worktrees::scan::run_scan;
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Stdio};

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

    /// Repository `proj` with the merged, clean worktrees `a` and `b`.
    fn merged_pair() -> (tempfile::TempDir, Vec<RepoGroup>, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap().join("home");
        let repo = home.join("proj");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        for name in ["a", "b"] {
            git(
                &repo,
                &["worktree", "add", "-q", "-b", name, &format!("../{name}")],
            );
            let wt = home.join(name);
            git(&wt, &["commit", "-q", "--allow-empty", "-m", name]);
            git(&repo, &["merge", "-q", "--ff-only", name]);
        }
        let groups = run_scan(&home, &Default::default(), &[], |_| None);
        let path = |n: &str| home.join(n).to_string_lossy().into_owned();
        let (a, b) = (path("a"), path("b"));
        (tmp, groups, a, b)
    }

    #[test]
    fn remove_merged_removes_clean_ones_and_rechecks_dirty_ones() {
        let (_tmp, groups, a, b) = merged_pair();
        assert!(groups[0].worktrees.iter().all(|w| w.merged), "{groups:?}");
        // Dirty after the scan: the fresh state must win over the snapshot.
        fs::write(Path::new(&b).join("scratch.txt"), "x").unwrap();
        let report = remove_merged_blocking(&groups, &[a.clone(), b.clone()], Vec::new);
        assert_eq!(report.removed, vec![a.clone()]);
        assert_eq!(
            report.skipped,
            vec![SkippedWorktree {
                path: b.clone(),
                reason: "has local changes".into()
            }]
        );
        assert!(!Path::new(&a).exists());
        assert!(Path::new(&b).join("scratch.txt").exists());
    }

    #[test]
    fn remove_merged_skips_a_worktree_with_a_running_service() {
        let (_tmp, groups, a, _b) = merged_pair();
        let mut service = crate::testutil::process("p", "proj", 3000);
        service.cwd = Some(format!("{a}/packages/app"));
        let report = remove_merged_blocking(&groups, &[a.clone()], || vec![service.clone()]);
        assert!(report.removed.is_empty());
        assert_eq!(report.skipped[0].reason, "a service is running");
        assert!(Path::new(&a).exists());
    }

    #[test]
    fn remove_merged_ignores_paths_outside_the_snapshot() {
        let (_tmp, groups, _a, _b) = merged_pair();
        let report = remove_merged_blocking(&groups, &["/etc".to_string()], Vec::new);
        assert!(report.removed.is_empty());
        assert_eq!(report.skipped[0].reason, "unknown worktree");
    }
}
