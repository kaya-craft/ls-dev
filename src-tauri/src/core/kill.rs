//! Kill a launcher and its whole subtree.

use std::time::{Duration, Instant};

use crate::core::procs::ProcReader;
use crate::core::procs::ProcTable;
use crate::core::tree::descendants_kill_order;

const GRACE: Duration = Duration::from_secs(3);
const STEP: Duration = Duration::from_millis(100);
const AFTER_KILL: Duration = Duration::from_millis(200);

/// Launcher first (it supervises; a watcher would respawn a killed leaf),
/// then descendants deepest first. Empty for unknown or system pids.
pub fn kill_plan(root_pid: u32, procs: &ProcTable) -> Vec<u32> {
    let me = std::process::id();
    if root_pid <= 1 || root_pid == me || !procs.contains_key(&root_pid) {
        return Vec::new();
    }
    let mut plan = vec![root_pid];
    plan.extend(
        descendants_kill_order(root_pid, procs)
            .into_iter()
            .filter(|p| *p != me),
    );
    plan
}

/// EPERM means the process exists but belongs to someone else.
pub fn is_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || io_errno() == Some(libc::EPERM)
}

fn io_errno() -> Option<i32> {
    std::io::Error::last_os_error().raw_os_error()
}

/// Never signals init, LsDev itself, or a pid that would become negative
/// (a negative pid targets a process group, -1 every process).
pub fn signal(pid: u32, sig: i32) {
    if pid <= 1 || pid == std::process::id() || pid > i32::MAX as u32 {
        return;
    }
    unsafe {
        libc::kill(pid as i32, sig);
    }
}

/// SIGTERM, up to 3 s of grace, SIGKILL, then report who is still alive.
pub async fn terminate(pids: &[u32]) -> Vec<u32> {
    for &pid in pids {
        signal(pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline {
        if pids.iter().all(|&p| !is_alive(p)) {
            return Vec::new();
        }
        tokio::time::sleep(STEP).await;
    }
    let stubborn: Vec<u32> = pids.iter().copied().filter(|&p| is_alive(p)).collect();
    for &pid in &stubborn {
        signal(pid, libc::SIGKILL);
    }
    tokio::time::sleep(AFTER_KILL).await;
    stubborn.into_iter().filter(|&p| is_alive(p)).collect()
}

/// The root pid must still run the command line shown to the user: a
/// confirmation can stay open for minutes and the pid may have been reused.
pub fn check_identity(
    root_pid: u32,
    expected_command: &str,
    procs: &ProcTable,
) -> Result<(), String> {
    let root = procs
        .get(&root_pid)
        .ok_or("This service is no longer running; nothing was killed.")?;
    if root.argv.join(" ") != expected_command {
        return Err("The process changed since the list was refreshed; nothing was killed.".into());
    }
    Ok(())
}

/// Fresh process table (the snapshot may be 3 s old), identity check, plan,
/// terminate. Ok carries the pids that survived SIGKILL.
pub async fn kill_tree(root_pid: u32, expected_command: &str) -> Result<Vec<u32>, String> {
    let procs = tokio::task::spawn_blocking(|| ProcReader::new().read())
        .await
        .unwrap_or_default();
    check_identity(root_pid, expected_command, &procs)?;
    let plan = kill_plan(root_pid, &procs);
    if plan.is_empty() {
        return Err("This service is no longer running; nothing was killed.".into());
    }
    Ok(terminate(&plan).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::proc_table;
    use std::process::{Command, Stdio};

    #[test]
    fn plan_puts_the_launcher_first_then_the_subtree_deepest_first() {
        assert_eq!(
            kill_plan(29414, &proc_table()),
            vec![29414, 46048, 29482, 29431]
        );
    }

    #[test]
    fn plan_covers_a_nuxt_chain() {
        let mut plan = kill_plan(79026, &proc_table());
        plan.sort();
        assert_eq!(plan, vec![79026, 79040, 79064, 93994]);
    }

    #[test]
    fn identity_check_accepts_the_same_command_line() {
        let t = proc_table();
        let expected = t[&29414].argv.join(" ");
        assert_eq!(check_identity(29414, &expected, &t), Ok(()));
    }

    #[test]
    fn identity_check_refuses_a_reused_pid() {
        let mut t = proc_table();
        let expected = t[&29414].argv.join(" ");
        t.get_mut(&29414).unwrap().argv = vec!["/usr/bin/some-other-tool".into()];
        assert_eq!(
            check_identity(29414, &expected, &t),
            Err("The process changed since the list was refreshed; nothing was killed.".into())
        );
    }

    #[test]
    fn identity_check_refuses_a_vanished_pid() {
        assert_eq!(
            check_identity(424242, "node x", &proc_table()),
            Err("This service is no longer running; nothing was killed.".into())
        );
    }

    #[tokio::test]
    async fn kill_tree_reports_an_unknown_root_as_an_error() {
        assert_eq!(
            kill_tree(u32::MAX - 7, "sleep 30").await,
            Err("This service is no longer running; nothing was killed.".into())
        );
    }

    #[tokio::test]
    async fn kill_tree_only_kills_the_expected_command() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(kill_tree(pid, "sleep 31").await.is_err());
        assert!(is_alive(pid));
        assert_eq!(kill_tree(pid, "sleep 30").await, Ok(vec![]));
        assert!(!is_alive(pid));
    }

    #[test]
    fn plan_refuses_unknown_and_system_pids() {
        assert!(kill_plan(424242, &proc_table()).is_empty());
        assert!(kill_plan(1, &proc_table()).is_empty());
    }

    #[tokio::test]
    async fn terminate_stops_a_plain_process() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        assert!(terminate(&[pid]).await.is_empty());
        assert!(!is_alive(pid));
    }

    #[tokio::test]
    async fn terminate_escalates_to_sigkill() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' TERM; while :; do sleep 0.1; done"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(terminate(&[pid]).await.is_empty());
    }
}
