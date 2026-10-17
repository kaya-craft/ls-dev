//! Discovery of git repositories: a bounded filesystem walk plus git lookups
//! for linked worktrees and service working directories.

use std::path::{Path, PathBuf};

use crate::core::git::{git_command, git_stdout_timeout};

/// Directory names never entered, at any depth.
pub const SKIP_ANYWHERE: &[&str] = &[
    "node_modules",
    "Library",
    ".Trash",
    "target",
    ".cache",
    ".npm",
    ".pnpm-store",
    ".cargo",
    ".rustup",
    ".local",
];

/// Directory names never entered when directly under the home directory.
pub const SKIP_UNDER_HOME: &[&str] = &[
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
];

/// Maximum depth below a root (the root itself is depth 0).
pub const MAX_DEPTH: usize = 6;

#[derive(Debug, Default, PartialEq)]
pub struct Walk {
    /// Directories containing a `.git` directory.
    pub repos: Vec<PathBuf>,
    /// Directories containing a `.git` file (linked worktrees).
    pub linked: Vec<PathBuf>,
}

/// Filesystem walk below `root`: no git involved. Symlinks are never followed
/// and unreadable directories are ignored. `home` enables the extra skip list
/// for the directories directly under it.
pub fn walk(root: &Path, home: &Path) -> Walk {
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let mut out = Walk::default();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        match std::fs::symlink_metadata(dir.join(".git")) {
            Ok(m) if m.is_dir() => {
                out.repos.push(dir.clone());
                // A root (often $HOME with a dotfiles repository) is recorded
                // but still searched: the repositories under it are the point.
                if depth > 0 {
                    continue;
                }
            }
            Ok(m) if m.is_file() => {
                out.linked.push(dir);
                continue;
            }
            _ => {}
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let under_home = depth == 0 && dir.canonicalize().map(|d| d == home).unwrap_or(false);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_ANYWHERE.contains(&name.as_ref())
                || (under_home && SKIP_UNDER_HOME.contains(&name.as_ref()))
            {
                continue;
            }
            stack.push((entry.path(), depth + 1));
        }
    }
    for list in [&mut out.repos, &mut out.linked] {
        list.sort();
        list.dedup();
    }
    out
}

/// Absolute common git directory of the repository containing `dir`.
fn git_common_dir(dir: &Path) -> Option<PathBuf> {
    let mut cmd = git_command(dir);
    cmd.args(["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let text = git_stdout_timeout(cmd)?;
    let line = text.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Main repository of a path inside any repository or linked worktree:
/// the parent of the common git directory, when that ends with `.git`.
pub fn main_repo_of(linked: &Path) -> Option<PathBuf> {
    let common = git_common_dir(linked)?;
    if common.file_name()? != ".git" {
        return None;
    }
    common.parent().map(Path::to_path_buf)
}

/// Every repository known from the home directory, the extra roots and the
/// service working directories, canonicalized, minus the excludes.
pub fn repositories(
    home: &Path,
    extra_roots: &[PathBuf],
    service_cwds: &[PathBuf],
    excludes: &[PathBuf],
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let home_walk = walk(home, home);
    found.extend(home_walk.repos);
    found.extend(home_walk.linked.iter().filter_map(|l| main_repo_of(l)));
    for root in extra_roots {
        let w = walk(root, home);
        found.extend(w.repos);
        found.extend(w.linked.iter().filter_map(|l| main_repo_of(l)));
    }
    found.extend(service_cwds.iter().filter_map(|c| main_repo_of(c)));

    let excluded: Vec<PathBuf> = excludes
        .iter()
        .map(|e| e.canonicalize().unwrap_or_else(|_| e.clone()))
        .collect();
    let mut repos: Vec<PathBuf> = found
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .filter(|p| !excluded.contains(p))
        .collect();
    repos.sort();
    repos.dedup();
    repos
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command, Stdio};
    use tempfile::tempdir;

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
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    fn mkrepo(root: &Path, rel: &str) -> PathBuf {
        let p = root.join(rel);
        fs::create_dir_all(p.join(".git")).unwrap();
        p
    }

    fn real_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
    }

    fn canon(p: &Path) -> PathBuf {
        p.canonicalize().unwrap()
    }

    #[test]
    fn finds_repos_up_to_depth_six_only() {
        let t = tempdir().unwrap();
        let r = t.path();
        let d2 = mkrepo(r, "a/b");
        let d6 = mkrepo(r, "a/b2/c/d/e/f");
        mkrepo(r, "a/b3/c/d/e/f/g");
        let w = walk(r, Path::new("/nonexistent-home"));
        assert_eq!(w.repos, vec![d2, d6]);
        assert!(w.linked.is_empty());
    }

    #[test]
    fn skips_heavy_directories_at_any_depth() {
        let t = tempdir().unwrap();
        let r = t.path();
        mkrepo(r, "node_modules/x");
        mkrepo(r, "target/y");
        mkrepo(r, "Library/z");
        mkrepo(r, "p/node_modules/x");
        let ok = mkrepo(r, "p/ok");
        let w = walk(r, Path::new("/nonexistent-home"));
        assert_eq!(w.repos, vec![ok]);
    }

    #[test]
    fn skips_user_folders_only_directly_under_home() {
        let t = tempdir().unwrap();
        let home = t.path();
        mkrepo(home, "Documents/r");
        let found = mkrepo(home, "work/Documents/r");
        let w = walk(home, home);
        assert_eq!(w.repos, vec![found]);
    }

    #[test]
    fn git_file_directories_are_linked_not_repos() {
        let t = tempdir().unwrap();
        let r = t.path();
        fs::create_dir_all(r.join("wt")).unwrap();
        fs::write(r.join("wt/.git"), "gitdir: /elsewhere\n").unwrap();
        let w = walk(r, Path::new("/nonexistent-home"));
        assert!(w.repos.is_empty());
        assert_eq!(w.linked, vec![r.join("wt")]);
    }

    #[test]
    fn a_repository_is_not_descended() {
        let t = tempdir().unwrap();
        let r = t.path();
        let repo = mkrepo(r, "repo");
        mkrepo(r, "repo/sub");
        let w = walk(r, Path::new("/nonexistent-home"));
        assert_eq!(w.repos, vec![repo]);
    }

    #[test]
    fn a_root_that_is_a_repository_is_recorded_and_still_descended() {
        let t = tempdir().unwrap();
        let home = mkrepo(t.path(), "home");
        let nested = mkrepo(&home, "dev/repo");
        let w = walk(&home, &home);
        assert_eq!(w.repos, vec![home, nested]);
    }

    #[test]
    fn symlink_to_an_ancestor_does_not_loop_and_is_not_followed() {
        let t = tempdir().unwrap();
        let r = t.path();
        let repo = mkrepo(r, "a/repo");
        std::os::unix::fs::symlink(r, r.join("a/loop")).unwrap();
        let w = walk(r, Path::new("/nonexistent-home"));
        assert_eq!(w.repos, vec![repo]);
    }

    #[test]
    fn repositories_unions_roots_linked_worktrees_cwds_and_excludes() {
        let t = tempdir().unwrap();
        let base = canon(t.path());
        let home = base.join("home");
        let other = base.join("other");
        let outside = base.join("outside");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&other).unwrap();
        let main = home.join("proj");
        real_repo(&main);
        let sibling = other.join("proj-feat");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat",
                sibling.to_str().unwrap(),
            ],
        );
        assert_eq!(main_repo_of(&sibling), Some(main.clone()));

        let found = repositories(&home, &[other.clone()], &[], &[]);
        assert_eq!(found, vec![main.clone()]);

        let excluded = repositories(&home, &[other.clone()], &[], &[main.clone()]);
        assert!(excluded.is_empty());

        let lone = outside.join("lone");
        real_repo(&lone);
        let cwd = lone.join("src");
        fs::create_dir_all(&cwd).unwrap();
        let with_cwd = repositories(&home, &[other], &[cwd], &[]);
        assert_eq!(with_cwd, vec![main, lone]);
    }
}
