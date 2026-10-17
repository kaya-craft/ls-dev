//! Linked git worktree detection, cached per working directory.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub branch: String,
    /// Top level of the linked worktree.
    pub path: String,
    /// Main worktree of the repository, where `git worktree remove` runs.
    pub main_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeEntry {
    pub path: String,
    /// None when detached.
    pub branch: Option<String>,
}

/// Parse `git worktree list --porcelain`. The first entry is the main worktree.
pub fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    for block in porcelain.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(p.to_string());
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            }
        }
        if let Some(path) = path {
            entries.push(WorktreeEntry { path, branch });
        }
    }
    entries
}

/// Some only when `toplevel` is a linked (non-main) worktree.
pub fn worktree_for(toplevel: &str, entries: &[WorktreeEntry]) -> Option<Worktree> {
    let main = entries.first()?;
    if main.path == toplevel {
        return None;
    }
    let entry = entries.iter().find(|e| e.path == toplevel)?;
    Some(Worktree {
        branch: entry.branch.clone().unwrap_or_else(|| "detached".into()),
        path: toplevel.to_string(),
        main_path: main.path.clone(),
    })
}

/// Two git calls; None outside a repository or when git is missing.
pub fn detect(cwd: &str) -> Option<Worktree> {
    let toplevel = git_output(cwd, &["rev-parse", "--show-toplevel"])?;
    let list = git_output(cwd, &["worktree", "list", "--porcelain"])?;
    worktree_for(toplevel.trim(), &parse_worktree_list(&list))
}

/// Base git command for user repositories: runs in `dir`, English messages,
/// no stdin, no inherited `GIT_*` variable, no optional locks, and the config
/// keys that most commonly execute programs (fsmonitor, hooks, signature
/// display) neutralised. This does not stop every program a repository config
/// can run (filter drivers, `gpg.program`, ...): callers that run status, log
/// or reflog must first check `repo_config_is_trusted`.
pub fn git_command(dir: impl AsRef<std::path::Path>) -> Command {
    let mut cmd = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(name);
        }
    }
    cmd.arg("-C")
        .arg(dir.as_ref())
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "log.showSignature=false",
            "-c",
            "status.showUntrackedFiles=all",
            "-c",
            "submodule.recurse=false",
            "--no-optional-locks",
        ])
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null());
    cmd
}

/// True when a `git config --name-only --list` output has a key that can make
/// git run a program. `core.fsmonitor` and `core.hooksPath` are absent: the
/// `-c` overrides of `git_command` take precedence over the repository config
/// (husky sets `core.hooksPath`). Config-based hooks (`hook.*`) are not covered
/// by `core.hooksPath` and stay untrusted.
pub fn config_keys_untrusted(keys: &str) -> bool {
    const EXACT: &[&str] = &[
        "core.sshcommand",
        "core.pager",
        "core.editor",
        "core.askpass",
        "core.alternaterefscommand",
        "core.gitproxy",
    ];
    const PREFIXES: &[&str] = &[
        "filter.",
        "diff.",
        "gpg.",
        "hook.",
        "include.",
        "includeif.",
        "submodule.",
    ];
    keys.lines().any(|k| {
        let k = k.trim().to_ascii_lowercase();
        EXACT.contains(&k.as_str()) || PREFIXES.iter().any(|p| k.starts_with(p))
    })
}

/// Upper bound for a git call made by a scan.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(15);

/// Exit status and stdout of `cmd` when it ends within `timeout`; the child is
/// killed and reaped otherwise (None). Stdout is drained while the child runs,
/// so a large output cannot fill the pipe and stall it.
pub fn run_with_timeout_status(
    mut cmd: Command,
    timeout: Duration,
) -> Option<(ExitStatus, String)> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let deadline = Instant::now() + timeout;
    // Stdout reaches EOF when the child exits: wait on it instead of polling.
    // A grandchild keeping the pipe open ends in the timeout branch.
    let Ok(buf) = rx.recv_timeout(timeout) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    Some((status, String::from_utf8_lossy(&buf).into_owned()))
}

/// Stdout of `cmd` when it exits within `timeout`, whatever its status: `du`
/// exits 1 on an unreadable subdirectory but still prints the total of what
/// it read.
pub fn run_with_timeout(cmd: Command, timeout: Duration) -> Option<String> {
    run_with_timeout_status(cmd, timeout).map(|(_, out)| out)
}

/// Stdout of a git command that succeeds within `GIT_TIMEOUT`; None on a
/// failure or a timeout.
pub fn git_stdout_timeout(cmd: Command) -> Option<String> {
    let (status, out) = run_with_timeout_status(cmd, GIT_TIMEOUT)?;
    status.success().then_some(out)
}

fn rev_parse_path(dir: &Path, flag: &str) -> Option<std::path::PathBuf> {
    let mut cmd = git_command(dir);
    cmd.args(["rev-parse", "--path-format=absolute", flag]);
    let text = git_stdout_timeout(cmd)?;
    let line = text.trim();
    (!line.is_empty()).then(|| std::path::PathBuf::from(line))
}

fn config_file_untrusted(dir: &Path, file: &Path) -> bool {
    let mut cmd = git_command(dir);
    cmd.arg("config")
        .arg("--file")
        .arg(file)
        .args(["--name-only", "--list"]);
    // A failure or a timeout is untrusted.
    git_stdout_timeout(cmd).is_none_or(|keys| config_keys_untrusted(&keys))
}

/// True when the worktree at `repo` or its repository has a submodule: a
/// submodule carries its own config, which status would use.
fn has_submodules(repo: &Path, common: &Path) -> bool {
    if common.join("modules").exists() {
        return true;
    }
    if let Some(top) = rev_parse_path(repo, "--show-toplevel") {
        if top.join(".gitmodules").exists() {
            return true;
        }
    }
    let mut cmd = git_command(repo);
    cmd.args(["ls-files", "--stage"]);
    // A failure or a timeout counts as "has submodules".
    git_stdout_timeout(cmd).is_none_or(|out| out.lines().any(|l| l.starts_with("160000 ")))
}

/// True when the repository containing `repo` has no config key that can run
/// a program, and no submodule. Reads the shared config and the worktree config as plain files
/// (nothing is executed); anything unreadable is untrusted.
pub fn repo_config_is_trusted(repo: &Path) -> bool {
    let (Some(common), Some(git_dir)) = (
        rev_parse_path(repo, "--git-common-dir"),
        rev_parse_path(repo, "--git-dir"),
    ) else {
        return false;
    };
    if config_file_untrusted(repo, &common.join("config")) || has_submodules(repo, &common) {
        return false;
    }
    let wt_config = git_dir.join("config.worktree");
    !(wt_config.exists() && config_file_untrusted(repo, &wt_config))
}

/// Canonical absolute common git directory of the repository containing `repo`.
pub fn repo_common_dir(repo: &Path) -> Option<std::path::PathBuf> {
    rev_parse_path(repo, "--git-common-dir")?
        .canonicalize()
        .ok()
}

/// The part of the trust verdict that belongs to one linked worktree: it must
/// share the repository's common git directory (`repo_common`, from
/// `repo_common_dir`): a `.git` file redirected to another repository makes
/// git read that repository's config. Its `config.worktree` and submodules
/// (the checked-out branch decides them) are checked too. The shared config
/// is not read: callers check it once per repository with
/// `repo_config_is_trusted`.
pub fn worktree_config_is_trusted(wt: &Path, repo_common: &Path) -> bool {
    let (Some(common), Some(git_dir)) = (
        rev_parse_path(wt, "--git-common-dir"),
        rev_parse_path(wt, "--git-dir"),
    ) else {
        return false;
    };
    if common.canonicalize().ok().as_deref() != Some(repo_common) {
        return false;
    }
    if has_submodules(wt, &common) {
        return false;
    }
    let wt_config = git_dir.join("config.worktree");
    !(wt_config.exists() && config_file_untrusted(wt, &wt_config))
}

fn git_output(cwd: &str, args: &[&str]) -> Option<String> {
    let output = git_command(cwd)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub struct WorktreeCache {
    ttl: Duration,
    entries: HashMap<String, (Instant, Option<Worktree>)>,
}

impl WorktreeCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    pub fn get(
        &mut self,
        cwd: &str,
        now: Instant,
        detect: impl FnOnce(&str) -> Option<Worktree>,
    ) -> Option<Worktree> {
        if let Some((at, value)) = self.entries.get(cwd) {
            if now.duration_since(*at) < self.ttl {
                return value.clone();
            }
        }
        let value = detect(cwd);
        self.entries.insert(cwd.to_string(), (now, value.clone()));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;

    const PORCELAIN: &str = "worktree /repo\nHEAD 1111\nbranch refs/heads/main\n\nworktree /repo-feature\nHEAD 2222\nbranch refs/heads/feature/x\n\nworktree /repo-detached\nHEAD 3333\ndetached\n";

    #[test]
    fn a_command_over_the_timeout_is_killed() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = Instant::now();
        assert_eq!(run_with_timeout(cmd, Duration::from_millis(100)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_timeout_is_a_failure_for_the_status_variant_too() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        assert!(run_with_timeout_status(cmd, Duration::from_millis(100)).is_none());
    }

    #[test]
    fn a_large_output_does_not_stall_the_child() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 1000000 /dev/zero | tr '\\0' x"]);
        let (status, out) = run_with_timeout_status(cmd, Duration::from_secs(10)).unwrap();
        assert!(status.success());
        assert_eq!(out.len(), 1_000_000);
    }

    #[test]
    fn a_failing_command_is_reported_as_a_failure() {
        let cmd = Command::new("false");
        assert!(!run_with_timeout_status(cmd, Duration::from_secs(5))
            .unwrap()
            .0
            .success());
        assert_eq!(git_stdout_timeout(Command::new("false")), None);
    }

    #[test]
    fn parses_porcelain_entries() {
        let entries = parse_worktree_list(PORCELAIN);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].path, "/repo-feature");
        assert_eq!(entries[1].branch.as_deref(), Some("feature/x"));
        assert_eq!(entries[2].branch, None);
    }

    #[test]
    fn main_worktree_is_not_reported() {
        assert_eq!(worktree_for("/repo", &parse_worktree_list(PORCELAIN)), None);
    }

    #[test]
    fn linked_worktree_reports_branch_and_main_path() {
        assert_eq!(
            worktree_for("/repo-feature", &parse_worktree_list(PORCELAIN)),
            Some(Worktree {
                branch: "feature/x".into(),
                path: "/repo-feature".into(),
                main_path: "/repo".into()
            })
        );
        assert_eq!(
            worktree_for("/repo-detached", &parse_worktree_list(PORCELAIN))
                .unwrap()
                .branch,
            "detached"
        );
    }

    pub(crate) fn git(dir: &Path, args: &[&str]) {
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
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn config_key_classification() {
        assert!(!config_keys_untrusted(
            "core.bare\ncore.filemode\nuser.name\ncore.fsmonitor\ncore.hooksPath\n"
        ));
        for k in [
            "filter.x.clean",
            "diff.x.textconv",
            "gpg.program",
            "include.path",
            "includeIf.gitdir:/x.path",
            "hook.lint.command",
            "core.sshCommand",
            "core.pager",
            "core.editor",
            "core.askpass",
            "core.alternateRefsCommand",
            "core.gitProxy",
        ] {
            assert!(config_keys_untrusted(k), "{k}");
        }
    }

    #[test]
    fn plain_repo_is_trusted_and_include_path_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("r");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        assert!(repo_config_is_trusted(&repo));
        git(&repo, &["config", "include.path", "/dev/null"]);
        assert!(!repo_config_is_trusted(&repo));
        assert!(!repo_config_is_trusted(&tmp.path().join("missing")));
    }

    #[test]
    fn husky_hooks_path_keeps_a_repo_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("r");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "core.hooksPath", ".husky/_"]);
        git(&repo, &["config", "core.fsmonitor", "true"]);
        assert!(repo_config_is_trusted(&repo));
    }

    #[test]
    fn gitmodules_file_makes_a_repo_untrusted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("r");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        assert!(repo_config_is_trusted(&repo));
        fs::write(repo.join(".gitmodules"), "").unwrap();
        assert!(!repo_config_is_trusted(&repo));
    }

    #[test]
    fn detect_finds_a_real_linked_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let main = root.join("repo");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let wt = root.join("repo-feature");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                wt.to_str().unwrap(),
                "-b",
                "feature",
            ],
        );
        fs::create_dir(wt.join("pkg")).unwrap();

        let found = detect(wt.join("pkg").to_str().unwrap()).expect("linked worktree");
        assert_eq!(found.branch, "feature");
        assert_eq!(found.path, wt.to_str().unwrap());
        assert_eq!(found.main_path, main.to_str().unwrap());
        assert_eq!(detect(main.to_str().unwrap()), None);
        assert_eq!(detect(root.to_str().unwrap()), None);
    }

    #[test]
    fn cache_calls_detect_once_per_ttl() {
        let calls = Cell::new(0);
        let detector = |_: &str| {
            calls.set(calls.get() + 1);
            None
        };
        let mut cache = WorktreeCache::new(Duration::from_secs(30));
        let t0 = Instant::now();
        cache.get("/a", t0, detector);
        cache.get("/a", t0 + Duration::from_secs(29), detector);
        assert_eq!(calls.get(), 1);
        cache.get("/a", t0 + Duration::from_secs(31), detector);
        assert_eq!(calls.get(), 2);
    }
}
