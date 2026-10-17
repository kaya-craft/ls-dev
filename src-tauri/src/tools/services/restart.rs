//! Relaunch a dev server as a process owned by LsDev, its output in a log file.

use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::core::kill::is_alive;
use crate::core::shell::exec_line;

use super::model::RestartSpec;

/// Start `spec` through `<shell> -lic 'exec …'` in its own process group,
/// stdout and stderr appended to `log_path`. A file (not a pipe) lets the
/// process outlive LsDev without dying of SIGPIPE. A thread reaps the child
/// so that a killed process does not linger as a zombie.
pub fn spawn_managed(spec: &RestartSpec, shell: &str, log_path: &Path) -> io::Result<u32> {
    if let Some(dir) = log_path.parent() {
        fs::create_dir_all(dir)?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let mut child = Command::new(shell)
        .arg("-lic")
        .arg(exec_line(&spec.argv))
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

/// How long a relaunched command must survive to count as started.
const STARTUP_CHECK: Duration = Duration::from_millis(500);
const TAIL_LINES: usize = 5;

/// Last `n` non-empty lines of `text`.
pub fn log_tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// `spawn_managed`, then fail with the end of the log when the command has
/// already exited after `STARTUP_CHECK` (bad command, port taken, ...): the
/// shell itself always starts, so `spawn_managed` alone cannot tell.
pub async fn spawn_checked(
    spec: &RestartSpec,
    shell: &str,
    log_path: &Path,
) -> Result<u32, String> {
    let pid = spawn_managed(spec, shell, log_path).map_err(|e| e.to_string())?;
    tokio::time::sleep(STARTUP_CHECK).await;
    if is_alive(pid) {
        return Ok(pid);
    }
    let tail = log_tail(
        &fs::read_to_string(log_path).unwrap_or_default(),
        TAIL_LINES,
    );
    Err(if tail.is_empty() {
        "The relaunched command exited immediately without output.".to_string()
    } else {
        format!("The relaunched command exited immediately: {tail}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kill::terminate;

    #[tokio::test]
    async fn spawn_managed_writes_output_to_the_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs/out.log");
        let spec = RestartSpec {
            argv: vec!["echo".into(), "hello world".into()],
            cwd: dir.path().to_str().unwrap().into(),
        };
        spawn_managed(&spec, "/bin/bash", &log).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(fs::read_to_string(&log).unwrap().contains("hello world"));
    }

    #[tokio::test]
    async fn terminate_reaps_managed_child() {
        let dir = tempfile::tempdir().unwrap();
        let spec = RestartSpec {
            argv: vec!["sleep".into(), "30".into()],
            cwd: dir.path().to_str().unwrap().into(),
        };
        let pid = spawn_managed(&spec, "/bin/bash", &dir.path().join("x.log")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(is_alive(pid));
        assert!(
            terminate(&[pid]).await.is_empty(),
            "a zombie would be reported as a survivor"
        );
    }

    #[tokio::test]
    async fn spawn_checked_reports_a_command_that_exits_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let spec = RestartSpec {
            argv: vec!["sh".into(), "-c".into(), "echo boom >&2; exit 3".into()],
            cwd: dir.path().to_str().unwrap().into(),
        };
        let err = spawn_checked(&spec, "/bin/bash", &dir.path().join("x.log"))
            .await
            .unwrap_err();
        assert!(
            err.starts_with("The relaunched command exited immediately"),
            "{err}"
        );
        assert!(err.contains("boom"), "{err}");
    }

    #[tokio::test]
    async fn spawn_checked_returns_the_pid_of_a_running_command() {
        let dir = tempfile::tempdir().unwrap();
        let spec = RestartSpec {
            argv: vec!["sleep".into(), "30".into()],
            cwd: dir.path().to_str().unwrap().into(),
        };
        let pid = spawn_checked(&spec, "/bin/bash", &dir.path().join("x.log"))
            .await
            .unwrap();
        assert!(is_alive(pid));
        assert!(terminate(&[pid]).await.is_empty());
    }

    #[test]
    fn log_tail_keeps_the_last_non_empty_lines() {
        assert_eq!(log_tail("a\n\nb\nc\nd\ne\nf\n\n", 3), "d\ne\nf");
        assert_eq!(log_tail("", 3), "");
    }
}
