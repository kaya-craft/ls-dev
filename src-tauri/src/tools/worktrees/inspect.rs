//! Git state of each linked worktree: last commit, local changes, upstream
//! state and "merged" status.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::git::{
    git_command, git_stdout_timeout, parse_worktree_list, repo_common_dir, repo_config_is_trusted,
    worktree_config_is_trusted, WorktreeEntry,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpstreamState {
    None,
    Gone,
    Tracking { ahead: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
    pub last_commit_at: Option<u64>,
    /// None when git status failed: the state is unknown, not clean.
    pub changes: Option<u32>,
    pub upstream: UpstreamState,
    pub merged: bool,
    /// False when the repository config can run programs: status, log and
    /// merge state were not read.
    pub trusted: bool,
    /// Filled later by the scan.
    pub size_bytes: Option<u64>,
}

/// `upstream` and `track` are the `%(upstream)` and `%(upstream:track)`
/// fields of `git for-each-ref`.
pub fn parse_track(upstream: &str, track: &str) -> UpstreamState {
    if upstream.is_empty() {
        return UpstreamState::None;
    }
    let track = track.trim();
    if track == "[gone]" {
        return UpstreamState::Gone;
    }
    let ahead = track
        .strip_prefix("[ahead ")
        .and_then(|rest| rest.split([',', ']']).next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    UpstreamState::Tracking { ahead }
}

/// `origin_head` is the target of `refs/remotes/origin/HEAD`, if any.
pub fn pick_main_branch(
    origin_head: Option<&str>,
    has_ref: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(name) = origin_head.and_then(|h| h.strip_prefix("refs/remotes/")) {
        if has_ref(name) {
            return Some(name.to_string());
        }
    }
    ["main", "master", "dev"]
        .into_iter()
        .find(|name| has_ref(name))
        .map(str::to_string)
}

/// A branch is merged when it is not the main branch itself, its tip is an
/// ancestor of the main branch, and its reflog records a commit or
/// cherry-pick made on it. Fresh, renamed or fast-forwarded branches have no
/// such entry, and neither do branches with an expired reflog. A detached
/// HEAD is never merged.
pub fn is_merged(
    ancestor: bool,
    has_commit_entry: bool,
    branch: Option<&str>,
    main_local: Option<&str>,
) -> bool {
    match branch {
        Some(b) => Some(b) != main_local && ancestor && has_commit_entry,
        None => false,
    }
}

/// True when a `%gs` reflog listing has a commit or cherry-pick subject.
pub fn has_commit_entry(subjects: &str) -> bool {
    subjects
        .lines()
        .any(|l| l.starts_with("commit") || l.starts_with("cherry-pick"))
}

/// Local name of a qualified main ref (`refs/remotes/origin/main` -> `main`).
fn local_name(qualified: &str) -> Option<&str> {
    if let Some(rest) = qualified.strip_prefix("refs/remotes/") {
        return rest.split_once('/').map(|(_, name)| name);
    }
    qualified.strip_prefix("refs/heads/")
}

/// Stdout of a git call that succeeds within `GIT_TIMEOUT`; None on a failure
/// or a timeout, which each caller maps to its safe value.
fn git_stdout(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = git_command(dir);
    cmd.args(args);
    git_stdout_timeout(cmd)
}

fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    git_stdout(dir, args).is_some()
}

/// Fully qualified ref (`refs/heads/x` or `refs/remotes/x`) for a short name.
fn qualify_ref(repo: &Path, name: &str) -> Option<String> {
    ["refs/heads/", "refs/remotes/"]
        .iter()
        .map(|prefix| format!("{prefix}{name}"))
        .find(|full| git_succeeds(repo, &["rev-parse", "--verify", "--quiet", full]))
}

/// Main branch of the repository (`origin/<x>` when known, else a local name).
pub fn main_branch(repo: &Path) -> Option<String> {
    let origin_head = git_stdout(
        repo,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
    pick_main_branch(origin_head.as_deref(), |name| {
        qualify_ref(repo, name).is_some()
    })
}

fn upstream_of(repo: &Path, branch: &str) -> UpstreamState {
    let refname = format!("refs/heads/{branch}");
    let Some(text) = git_stdout(
        repo,
        &[
            "for-each-ref",
            "--format=%(upstream)%09%(upstream:track)",
            &refname,
        ],
    ) else {
        return UpstreamState::None;
    };
    let line = text.lines().next().unwrap_or("");
    let (upstream, track) = line.split_once('\t').unwrap_or((line, ""));
    parse_track(upstream, track)
}

fn merged_state(repo: &Path, branch: Option<&str>, main: Option<&str>) -> bool {
    let (Some(branch), Some(main)) = (branch, main) else {
        return false;
    };
    let Some(main_ref) = qualify_ref(repo, main) else {
        return false;
    };
    let refname = format!("refs/heads/{branch}");
    let ancestor = git_succeeds(repo, &["merge-base", "--is-ancestor", &refname, &main_ref]);
    let commit_entry = git_stdout(repo, &["reflog", "show", "--format=%gs", &refname])
        .map(|s| has_commit_entry(&s))
        .unwrap_or(false);
    is_merged(ancestor, commit_entry, Some(branch), local_name(&main_ref))
}

/// What a repository decides for all its worktrees, computed once per scan.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoContext {
    /// Main branch (`main_branch`).
    pub main: Option<String>,
    /// Verdict of `repo_config_is_trusted` on the repository (false when the
    /// common git directory cannot be resolved).
    pub trusted: bool,
    /// Canonical common git directory: a worktree must share it.
    pub common: Option<PathBuf>,
}

impl RepoContext {
    pub fn load(repo: &Path) -> Self {
        let common = repo_common_dir(repo);
        Self {
            main: main_branch(repo),
            trusted: common.is_some() && repo_config_is_trusted(repo),
            common,
        }
    }
}

pub fn inspect(repo: &Path, entry: &WorktreeEntry, ctx: &RepoContext) -> WorktreeInfo {
    let wt = Path::new(&entry.path);
    let branch = entry.branch.as_deref();
    let main = ctx.main.as_deref();
    let upstream = branch.map_or(UpstreamState::None, |b| upstream_of(repo, b));
    let trusted = ctx.trusted
        && ctx
            .common
            .as_deref()
            .is_some_and(|common| worktree_config_is_trusted(wt, common));
    if !trusted {
        return WorktreeInfo {
            path: entry.path.clone(),
            branch: entry.branch.clone(),
            last_commit_at: None,
            changes: None,
            upstream,
            merged: false,
            trusted,
            size_bytes: None,
        };
    }
    let last_commit_at =
        git_stdout(wt, &["log", "-1", "--format=%ct"]).and_then(|s| s.trim().parse::<u64>().ok());
    let changes = git_stdout(
        wt,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )
    .map(|s| s.lines().filter(|l| !l.is_empty()).count() as u32);
    WorktreeInfo {
        path: entry.path.clone(),
        branch: entry.branch.clone(),
        last_commit_at,
        changes,
        upstream,
        merged: merged_state(repo, branch, main),
        trusted,
        size_bytes: None,
    }
}

/// Linked worktrees of `repo`: the main entry and entries whose directory is
/// gone (prunable) are skipped.
pub fn linked_worktrees(repo: &Path) -> Vec<WorktreeEntry> {
    let Some(text) = git_stdout(repo, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    parse_worktree_list(&text)
        .into_iter()
        .skip(1)
        .filter(|e| Path::new(&e.path).exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command, Stdio};
    use tempfile::tempdir;

    fn g(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
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
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("repo");
        fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q"]);
        g(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        (tmp, repo)
    }

    fn add_wt(repo: &Path, name: &str) -> PathBuf {
        g(
            repo,
            &["worktree", "add", "-q", "-b", name, &format!("../{name}")],
        );
        repo.parent().unwrap().join(name)
    }

    fn info_of(repo: &Path, name: &str) -> WorktreeInfo {
        let ctx = RepoContext::load(repo);
        let entry = linked_worktrees(repo)
            .into_iter()
            .find(|e| e.path.ends_with(&format!("/{name}")))
            .unwrap();
        inspect(repo, &entry, &ctx)
    }

    #[test]
    fn inspect_uses_the_precomputed_repository_context() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "done");
        let entry = linked_worktrees(&repo)
            .into_iter()
            .find(|e| e.path.ends_with("/done"))
            .unwrap();
        // The repository is clean and trusted; the context says otherwise and wins.
        let untrusted = RepoContext {
            trusted: false,
            ..RepoContext::load(&repo)
        };
        let info = inspect(&repo, &entry, &untrusted);
        assert!(!info.trusted);
        assert_eq!(info.changes, None);
        // Same for the main branch: no main in the context, nothing is merged,
        // although the branch would be merged against the real main.
        g(&wt, &["commit", "-q", "--allow-empty", "-m", "work"]);
        g(&repo, &["merge", "-q", "--ff-only", "done"]);
        let no_main = RepoContext {
            main: None,
            ..RepoContext::load(&repo)
        };
        assert!(!inspect(&repo, &entry, &no_main).merged);
        assert!(inspect(&repo, &entry, &RepoContext::load(&repo)).merged);
    }

    #[test]
    fn parse_track_cases() {
        assert_eq!(parse_track("", ""), UpstreamState::None);
        assert_eq!(parse_track("", "[gone]"), UpstreamState::None);
        assert_eq!(
            parse_track("refs/remotes/o/x", "[gone]"),
            UpstreamState::Gone
        );
        assert_eq!(
            parse_track("refs/remotes/o/x", "[ahead 2]"),
            UpstreamState::Tracking { ahead: 2 }
        );
        assert_eq!(
            parse_track("refs/remotes/o/x", "[ahead 3, behind 1]"),
            UpstreamState::Tracking { ahead: 3 }
        );
        assert_eq!(
            parse_track("refs/remotes/o/x", "[behind 4]"),
            UpstreamState::Tracking { ahead: 0 }
        );
        assert_eq!(
            parse_track("refs/remotes/o/x", ""),
            UpstreamState::Tracking { ahead: 0 }
        );
    }

    #[test]
    fn upstream_serialization() {
        let j = |s: &UpstreamState| serde_json::to_string(s).unwrap();
        assert_eq!(j(&UpstreamState::None), r#"{"kind":"none"}"#);
        assert_eq!(j(&UpstreamState::Gone), r#"{"kind":"gone"}"#);
        assert_eq!(
            j(&UpstreamState::Tracking { ahead: 2 }),
            r#"{"kind":"tracking","ahead":2}"#
        );
    }

    #[test]
    fn pick_main_branch_order_and_fallbacks() {
        let has = |names: &'static [&'static str]| move |n: &str| names.contains(&n);
        assert_eq!(
            pick_main_branch(
                Some("refs/remotes/origin/main"),
                has(&["origin/main", "main"])
            ),
            Some("origin/main".into())
        );
        assert_eq!(
            pick_main_branch(Some("refs/remotes/origin/main"), has(&["master"])),
            Some("master".into())
        );
        assert_eq!(
            pick_main_branch(None, has(&["dev", "master"])),
            Some("master".into())
        );
        assert_eq!(
            pick_main_branch(None, has(&["main", "master", "dev"])),
            Some("main".into())
        );
        assert_eq!(pick_main_branch(None, has(&["dev"])), Some("dev".into()));
        assert_eq!(pick_main_branch(None, has(&[])), None);
    }

    #[test]
    fn is_merged_truth_table() {
        assert!(is_merged(true, true, Some("b"), Some("main")));
        assert!(!is_merged(true, false, Some("b"), Some("main")));
        assert!(!is_merged(false, true, Some("b"), Some("main")));
        assert!(!is_merged(true, true, None, Some("main")));
        assert!(!is_merged(true, true, Some("main"), Some("main")));
        assert!(is_merged(true, true, Some("b"), None));
    }

    #[test]
    fn commit_entry_detection() {
        assert!(has_commit_entry("branch: Created from HEAD\ncommit: x"));
        assert!(has_commit_entry("commit (amend): x"));
        assert!(has_commit_entry("commit (merge): x"));
        assert!(has_commit_entry("commit (initial): x"));
        assert!(has_commit_entry("cherry-pick: x"));
        assert!(!has_commit_entry("branch: Created from HEAD"));
        assert!(!has_commit_entry(
            "merge main: Fast-forward\nBranch: renamed a to b"
        ));
        assert!(!has_commit_entry(""));
    }

    #[test]
    fn renamed_fresh_branch_is_not_merged() {
        let (_tmp, repo) = setup();
        add_wt(&repo, "fresh");
        g(&repo, &["branch", "-m", "fresh", "renamed"]);
        assert!(!info_of(&repo, "fresh").merged);
    }

    #[test]
    fn fresh_branch_fast_forwarded_to_newer_main_is_not_merged() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "ff");
        g(&repo, &["commit", "-q", "--allow-empty", "-m", "newer"]);
        g(&wt, &["merge", "-q", "--ff-only", "main"]);
        assert!(!info_of(&repo, "ff").merged);
    }

    #[test]
    fn worktree_on_main_while_main_checkout_is_elsewhere_is_not_merged() {
        let (_tmp, repo) = setup();
        g(&repo, &["checkout", "-q", "-b", "other"]);
        g(&repo, &["worktree", "add", "-q", "../m", "main"]);
        assert!(!info_of(&repo, "m").merged);
    }

    #[test]
    fn tag_named_like_an_unmerged_branch_does_not_make_it_merged() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "v1");
        g(&wt, &["commit", "-q", "--allow-empty", "-m", "work"]);
        g(&repo, &["tag", "v1", "main"]);
        assert!(!info_of(&repo, "v1").merged);
    }

    #[test]
    fn changes_ignore_show_untracked_files_config() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "cfg");
        g(&repo, &["config", "status.showUntrackedFiles", "no"]);
        fs::write(wt.join("u.txt"), "x").unwrap();
        assert_eq!(info_of(&repo, "cfg").changes, Some(1));
    }

    #[test]
    fn changes_are_unknown_when_status_fails() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "vanish");
        let ctx = RepoContext::load(&repo);
        let entry = linked_worktrees(&repo)
            .into_iter()
            .find(|e| e.path.ends_with("/vanish"))
            .unwrap();
        fs::remove_dir_all(&wt).unwrap();
        assert_eq!(inspect(&repo, &entry, &ctx).changes, None);
    }

    #[test]
    fn inspect_does_not_run_repository_fsmonitor() {
        let (tmp, repo) = setup();
        let wt = add_wt(&repo, "fsm");
        let marker = tmp.path().join("marker");
        let script = tmp.path().join("hook.sh");
        fs::write(&script, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        g(
            &repo,
            &["config", "core.fsmonitor", script.to_str().unwrap()],
        );
        // Sanity: an unprotected status does run the hook.
        g(&wt, &["status", "--porcelain"]);
        assert!(marker.exists(), "the hook must fire without protection");
        fs::remove_file(&marker).unwrap();
        info_of(&repo, "fsm");
        assert!(!marker.exists(), "inspect must not run core.fsmonitor");
    }

    fn marker_script(tmp: &Path, name: &str, body: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let marker = tmp.join(format!("{name}.marker"));
        let script = tmp.join(format!("{name}.sh"));
        fs::write(
            &script,
            format!("#!/bin/sh\ntouch {}\n{body}\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        (script, marker)
    }

    #[test]
    fn filter_driver_repo_is_untrusted_and_never_executed() {
        let (tmp, repo) = setup();
        let wt = add_wt(&repo, "flt");
        fs::write(wt.join(".gitattributes"), "* filter=x\n").unwrap();
        fs::write(wt.join("a.txt"), "one\n").unwrap();
        g(&wt, &["add", "."]);
        g(&wt, &["commit", "-q", "-m", "files"]);
        let (script, marker) = marker_script(tmp.path(), "flt", "cat");
        g(
            &repo,
            &["config", "filter.x.clean", script.to_str().unwrap()],
        );
        fs::write(wt.join("a.txt"), "two\n").unwrap();
        // Sanity: an unprotected status runs the filter.
        g(&wt, &["status", "--porcelain"]);
        assert!(marker.exists(), "the filter must fire without protection");
        fs::remove_file(&marker).unwrap();
        assert!(!repo_config_is_trusted(&repo));
        let info = info_of(&repo, "flt");
        assert!(!marker.exists());
        assert!(!info.trusted);
        assert_eq!(info.changes, None);
        assert_eq!(info.last_commit_at, None);
        assert!(!info.merged);
    }

    #[test]
    fn worktree_redirected_to_another_repository_is_untrusted_and_never_executed() {
        let (tmp, repo) = setup();
        let wt = add_wt(&repo, "red");
        let root = tmp.path().canonicalize().unwrap();
        let evil = root.join("evil");
        fs::create_dir_all(&evil).unwrap();
        g(&evil, &["init", "-q"]);
        fs::write(evil.join(".gitattributes"), "* filter=x\n").unwrap();
        fs::write(evil.join("a.txt"), "one\n").unwrap();
        g(&evil, &["add", "."]);
        g(&evil, &["commit", "-q", "-m", "evil"]);
        let (script, marker) = marker_script(&root, "red", "cat");
        g(
            &evil,
            &["config", "filter.x.clean", script.to_str().unwrap()],
        );
        // The worktree now points at the evil repository; the clean repository
        // still lists it.
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", evil.join(".git").display()),
        )
        .unwrap();
        fs::write(wt.join(".gitattributes"), "* filter=x\n").unwrap();
        fs::write(wt.join("a.txt"), "two\n").unwrap();
        assert!(repo_config_is_trusted(&repo));
        let info = info_of(&repo, "red");
        assert!(!marker.exists(), "inspect must not run the evil filter");
        assert!(!info.trusted);
        assert_eq!(info.changes, None);
        assert_eq!(info.last_commit_at, None);
        assert!(!info.merged);
        // The removal gate refuses it as well, before any git status.
        let target = crate::core::git::Worktree {
            branch: "red".into(),
            path: wt.to_string_lossy().into_owned(),
            main_path: repo.to_string_lossy().into_owned(),
        };
        let refused = crate::tools::worktrees::remove::check_clean(&target);
        assert!(refused.is_err(), "{refused:?}");
        assert!(!marker.exists(), "the removal gate must not run the filter");
    }

    #[test]
    fn submodule_with_filter_driver_is_untrusted_and_never_executed() {
        let (tmp, repo) = setup();
        let sub_src = tmp.path().canonicalize().unwrap().join("sub-src");
        fs::create_dir_all(&sub_src).unwrap();
        g(&sub_src, &["init", "-q"]);
        fs::write(sub_src.join(".gitattributes"), "* filter=x\n").unwrap();
        fs::write(sub_src.join("f.txt"), "one\n").unwrap();
        g(&sub_src, &["add", "."]);
        g(&sub_src, &["commit", "-q", "-m", "sub"]);
        let wt = add_wt(&repo, "withsub");
        g(
            &wt,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                sub_src.to_str().unwrap(),
                "sub",
            ],
        );
        g(&wt, &["commit", "-q", "-m", "add sub"]);
        let (script, marker) = marker_script(tmp.path(), "sm", "cat");
        g(
            &wt.join("sub"),
            &["config", "filter.x.clean", script.to_str().unwrap()],
        );
        fs::write(wt.join("sub/f.txt"), "two\n").unwrap();
        assert!(!repo_config_is_trusted(&wt));
        let info = info_of(&repo, "withsub");
        assert!(!marker.exists(), "submodule filter must not run");
        assert!(!info.trusted);
        assert_eq!(info.changes, None);
        // check_clean and remove refuse too.
        let w = crate::core::git::Worktree {
            branch: "withsub".into(),
            path: wt.to_str().unwrap().into(),
            main_path: repo.to_str().unwrap().into(),
        };
        assert!(matches!(
            super::super::remove::check_clean(&w),
            Err(super::super::remove::RemoveError::Untrusted(_))
        ));
        assert!(super::super::remove::remove(&w, true).is_err());
        assert!(wt.exists());
        assert!(!marker.exists());
    }

    #[test]
    fn show_signature_with_gpg_program_never_executes() {
        let (tmp, repo) = setup();
        let wt = add_wt(&repo, "sig");
        let tree = g(&repo, &["hash-object", "-t", "tree", "-w", "/dev/null"]);
        let commit = format!(
            "tree {}\nauthor t <t@example.com> 1 +0000\ncommitter t <t@example.com> 1 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n fake\n -----END PGP SIGNATURE-----\n\nmsg\n",
            tree.trim()
        );
        let file = tmp.path().join("commit.txt");
        fs::write(&file, commit).unwrap();
        let hash = g(
            &repo,
            &["hash-object", "-t", "commit", "-w", file.to_str().unwrap()],
        );
        g(&wt, &["reset", "-q", "--hard", hash.trim()]);
        let (script, marker) = marker_script(tmp.path(), "gpg", "exit 1");
        g(&repo, &["config", "log.showSignature", "true"]);
        g(&repo, &["config", "gpg.program", script.to_str().unwrap()]);
        let info = info_of(&repo, "sig");
        assert!(!marker.exists(), "gpg.program must not run");
        assert!(!info.trusted);
    }

    #[test]
    fn main_branch_uses_origin_head_when_present() {
        let (tmp, repo) = setup();
        let remote = tmp.path().canonicalize().unwrap().join("origin.git");
        fs::create_dir_all(&remote).unwrap();
        g(&remote, &["init", "-q", "--bare"]);
        g(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        g(&repo, &["push", "-q", "-u", "origin", "main"]);
        g(&repo, &["remote", "set-head", "origin", "-a"]);
        assert_eq!(main_branch(&repo), Some("origin/main".into()));
    }

    #[test]
    fn fresh_branch_without_commit_is_not_merged() {
        let (_tmp, repo) = setup();
        add_wt(&repo, "fresh");
        let info = info_of(&repo, "fresh");
        assert!(!info.merged);
        assert_eq!(info.changes, Some(0));
        assert_eq!(info.upstream, UpstreamState::None);
        assert_eq!(info.branch.as_deref(), Some("fresh"));
        assert!(info.last_commit_at.is_some());
    }

    #[test]
    fn merged_after_commit_and_fast_forward() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "done");
        g(&wt, &["commit", "-q", "--allow-empty", "-m", "work"]);
        g(&repo, &["merge", "-q", "--ff-only", "done"]);
        assert!(info_of(&repo, "done").merged);
    }

    #[test]
    fn unmerged_commit_is_not_merged() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "wip");
        g(&wt, &["commit", "-q", "--allow-empty", "-m", "work"]);
        assert!(!info_of(&repo, "wip").merged);
    }

    #[test]
    fn untracked_file_counts_as_one_change() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "dirty");
        fs::write(wt.join("new.txt"), "x").unwrap();
        assert_eq!(info_of(&repo, "dirty").changes, Some(1));
    }

    #[test]
    fn upstream_tracking_then_gone() {
        let (tmp, repo) = setup();
        let remote = tmp.path().canonicalize().unwrap().join("origin.git");
        fs::create_dir_all(&remote).unwrap();
        g(&remote, &["init", "-q", "--bare"]);
        g(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        let wt = add_wt(&repo, "done");
        g(&wt, &["commit", "-q", "--allow-empty", "-m", "one"]);
        g(&wt, &["push", "-q", "-u", "origin", "done"]);
        assert_eq!(
            info_of(&repo, "done").upstream,
            UpstreamState::Tracking { ahead: 0 }
        );
        g(&wt, &["commit", "-q", "--allow-empty", "-m", "two"]);
        assert_eq!(
            info_of(&repo, "done").upstream,
            UpstreamState::Tracking { ahead: 1 }
        );
        g(&remote, &["branch", "-q", "-D", "done"]);
        g(&repo, &["fetch", "-q", "--prune"]);
        assert_eq!(info_of(&repo, "done").upstream, UpstreamState::Gone);
    }

    #[test]
    fn detached_worktree_has_no_branch_and_is_not_merged() {
        let (_tmp, repo) = setup();
        g(&repo, &["worktree", "add", "-q", "--detach", "../det"]);
        let info = info_of(&repo, "det");
        assert_eq!(info.branch, None);
        assert!(!info.merged);
        assert_eq!(info.upstream, UpstreamState::None);
    }

    #[test]
    fn deleted_worktree_directory_is_not_listed() {
        let (_tmp, repo) = setup();
        let wt = add_wt(&repo, "gone");
        add_wt(&repo, "kept");
        fs::remove_dir_all(&wt).unwrap();
        let listed = linked_worktrees(&repo);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].path.ends_with("/kept"));
    }

    #[test]
    fn main_branch_falls_back_to_local_main() {
        let (_tmp, repo) = setup();
        assert_eq!(main_branch(&repo), Some("main".into()));
    }
}
