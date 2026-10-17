//! `git worktree remove` for the worktree a service runs in.

use crate::core::git::{git_command, repo_config_is_trusted, Worktree};
use crate::core::shell::shell_quote;
use crate::tools::services::model::ProcessService;

#[derive(Debug, Clone, PartialEq)]
pub enum RemoveError {
    /// Git refused because of local changes; retry with --force.
    NeedsForce(String),
    /// The repository config can run programs: the user removes the worktree
    /// from a terminal (`manual_remove_command`).
    Untrusted(String),
    Other(String),
}

pub fn classify(stderr: &str) -> RemoveError {
    let message = stderr.trim().to_string();
    if message.contains("--force") || message.contains("modified or untracked") {
        RemoveError::NeedsForce(message)
    } else {
        RemoveError::Other(message)
    }
}

/// Services whose cwd is the worktree or one of its subdirectories.
pub fn services_in(path: &str, processes: &[ProcessService]) -> Vec<ProcessService> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    processes
        .iter()
        .filter(|p| {
            p.cwd
                .as_deref()
                .is_some_and(|cwd| cwd == path || cwd.starts_with(&prefix))
        })
        .cloned()
        .collect()
}

const LISTED_FILES: usize = 5;

/// Message for `git status --porcelain` output; None when the worktree is clean.
pub fn dirty_message(path: &str, porcelain: &str) -> Option<String> {
    let files: Vec<&str> = porcelain
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.get(3..).unwrap_or(l).trim())
        .collect();
    if files.is_empty() {
        return None;
    }
    let mut listed = files
        .iter()
        .take(LISTED_FILES)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if files.len() > LISTED_FILES {
        listed.push_str(&format!(" and {} more", files.len() - LISTED_FILES));
    }
    Some(format!(
        "{path} contains modified or untracked files: {listed}"
    ))
}

const UNTRUSTED: &str =
    "This repository's git config can run programs; remove the worktree from a terminal.";

fn ensure_trusted(wt: &Worktree) -> Result<(), RemoveError> {
    if repo_config_is_trusted(std::path::Path::new(&wt.path))
        && repo_config_is_trusted(std::path::Path::new(&wt.main_path))
    {
        Ok(())
    } else {
        Err(RemoveError::Untrusted(UNTRUSTED.into()))
    }
}

/// Shell line typed in the user's terminal: goes to the main worktree and
/// prints the removal command without running it, then leaves a login shell.
pub fn manual_remove_command(wt: &Worktree) -> String {
    let remove = format!("git worktree remove {}", shell_quote(&wt.path));
    format!(
        "cd {} && printf '%s\\n\\n  %s\\n\\n' 'Review the repository config, then run:' {}; exec \"${{SHELL:-/bin/zsh}}\" -l",
        shell_quote(&wt.main_path),
        shell_quote(&remove)
    )
}

/// Refuses (NeedsForce) when the worktree has local changes. Run before any
/// service is stopped, so that a refused removal leaves everything running.
pub fn check_clean(wt: &Worktree) -> Result<(), RemoveError> {
    ensure_trusted(wt)?;
    let output = git_command(&wt.path)
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ])
        .output()
        .map_err(|e| RemoveError::Other(format!("git is unavailable: {e}")))?;
    if !output.status.success() {
        return Err(RemoveError::Other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    match dirty_message(&wt.path, &String::from_utf8_lossy(&output.stdout)) {
        Some(message) => Err(RemoveError::NeedsForce(message)),
        None => Ok(()),
    }
}

/// Runs from the main worktree; the Git branch is kept.
pub fn remove(wt: &Worktree, force: bool) -> Result<(), RemoveError> {
    ensure_trusted(wt)?;
    let mut cmd = git_command(&wt.main_path);
    cmd.args(["worktree", "remove"]);
    if force {
        cmd.arg("--force");
    }
    cmd.arg(&wt.path);
    // Git messages are translated; classify() matches the English text.
    let output = cmd
        .output()
        .map_err(|e| RemoveError::Other(format!("git is unavailable: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(classify(&String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::process;
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
    fn classify_detects_the_force_hint() {
        let stderr =
            "fatal: '/x/wt' contains modified or untracked files, use --force to delete it";
        assert_eq!(classify(stderr), RemoveError::NeedsForce(stderr.into()));
        assert_eq!(
            classify("fatal: '/x' is not a working tree"),
            RemoveError::Other("fatal: '/x' is not a working tree".into())
        );
    }

    #[test]
    fn services_in_respects_path_boundaries() {
        let mut a = process("a", "a", 3000);
        a.cwd = Some("/x/wt".into());
        let mut b = process("b", "b", 3001);
        b.cwd = Some("/x/wt/packages/app".into());
        let mut c = process("c", "c", 3002);
        c.cwd = Some("/x/wt-2/packages/app".into());
        let ids: Vec<String> = services_in("/x/wt", &[a, b, c])
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    /// Temporary repository with one linked worktree; returns (tmp guard, main, worktree).
    fn temp_worktree() -> (tempfile::TempDir, std::path::PathBuf, Worktree) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let main = root.join("repo");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let wt_path = root.join("repo-wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                wt_path.to_str().unwrap(),
                "-b",
                "wt",
            ],
        );
        let wt = Worktree {
            branch: "wt".into(),
            path: wt_path.to_str().unwrap().into(),
            main_path: main.to_str().unwrap().into(),
        };
        (tmp, main, wt)
    }

    #[test]
    fn untracked_files_are_protected_despite_show_untracked_config() {
        let (_tmp, main, wt) = temp_worktree();
        git(&main, &["config", "status.showUntrackedFiles", "no"]);
        let file = Path::new(&wt.path).join("precious.txt");
        fs::write(&file, "x").unwrap();
        assert!(matches!(check_clean(&wt), Err(RemoveError::NeedsForce(_))));
        assert!(remove(&wt, false).is_err());
        assert!(file.exists());
    }

    #[test]
    fn untrusted_repository_is_refused_before_anything_runs() {
        let (_tmp, main, wt) = temp_worktree();
        git(&main, &["config", "core.pager", "true"]);
        let refused = Err(RemoveError::Untrusted(UNTRUSTED.into()));
        assert_eq!(check_clean(&wt), refused);
        assert_eq!(remove(&wt, true), refused);
        assert!(Path::new(&wt.path).exists());
    }

    #[test]
    fn manual_remove_command_prints_the_removal_without_running_it() {
        let wt = Worktree {
            branch: "feat/x".into(),
            path: "/w/my repo-x".into(),
            main_path: "/w/my repo".into(),
        };
        assert_eq!(
            manual_remove_command(&wt),
            r#"cd '/w/my repo' && printf '%s\n\n  %s\n\n' 'Review the repository config, then run:' 'git worktree remove '\''/w/my repo-x'\'''; exec "${SHELL:-/bin/zsh}" -l"#
        );
    }

    #[test]
    fn dirty_message_lists_porcelain_entries() {
        assert_eq!(dirty_message("/x/wt", ""), None);
        assert_eq!(dirty_message("/x/wt", "\n"), None);
        assert_eq!(
            dirty_message("/x/wt", "?? scratch.txt\n M src/a.rs\n").as_deref(),
            Some("/x/wt contains modified or untracked files: scratch.txt, src/a.rs")
        );
        let many: String = (0..7).map(|i| format!("?? f{i}\n")).collect();
        assert_eq!(
            dirty_message("/x/wt", &many).as_deref(),
            Some("/x/wt contains modified or untracked files: f0, f1, f2, f3, f4 and 2 more")
        );
    }

    #[test]
    fn check_clean_refuses_a_worktree_with_untracked_files() {
        let (_tmp, _main, wt) = temp_worktree();
        assert_eq!(check_clean(&wt), Ok(()));
        fs::write(Path::new(&wt.path).join("scratch.txt"), "x").unwrap();
        match check_clean(&wt) {
            Err(RemoveError::NeedsForce(m)) => assert!(m.contains("scratch.txt"), "{m}"),
            other => panic!("expected NeedsForce, got {other:?}"),
        }
        assert!(Path::new(&wt.path).exists());
    }

    #[test]
    fn remove_asks_for_force_with_untracked_files_then_removes() {
        let (_tmp, main, wt) = temp_worktree();
        let wt_path = std::path::PathBuf::from(&wt.path);
        fs::write(wt_path.join("scratch.txt"), "x").unwrap();
        assert!(matches!(
            remove(&wt, false),
            Err(RemoveError::NeedsForce(_))
        ));
        assert!(wt_path.exists());
        remove(&wt, true).unwrap();
        assert!(!wt_path.exists());
        let branches = Command::new("git")
            .arg("-C")
            .arg(&main)
            .args(["branch", "--list", "wt"])
            .output()
            .unwrap();
        assert!(!branches.stdout.is_empty(), "branch wt must be kept");
    }
}
