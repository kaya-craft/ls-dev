//! Open tunnels of this LsDev session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::process::{
    now_ms, Callbacks, Launch, Tunnel, TunnelKind, TunnelProcess, TunnelStatus, HOUR_MS,
};
use super::store::StableStore;

pub struct TunnelManager {
    entries: Mutex<Vec<Arc<TunnelProcess>>>,
    /// 30 s in the app; shorter in tests.
    pub ready_timeout: Duration,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            ready_timeout: Duration::from_secs(30),
        }
    }
}

impl TunnelManager {
    pub async fn start(
        &self,
        launch: Launch,
        port: u16,
        project: String,
        cwd: Option<String>,
        hours: u8,
        on_change: impl Fn() + Send + Sync + 'static,
        on_unexpected_exit: impl Fn(Tunnel) + Send + Sync + 'static,
    ) -> Tunnel {
        let started_at = now_ms();
        let tunnel = Tunnel {
            id: uuid::Uuid::new_v4().to_string(),
            kind: launch.kind,
            port,
            project,
            cwd,
            url: None,
            status: TunnelStatus::Starting,
            error: None,
            started_at,
            expires_at: started_at + u64::from(hours) * HOUR_MS,
        };
        let callbacks = Callbacks {
            on_change: Arc::new(on_change),
            on_unexpected_exit: Arc::new(on_unexpected_exit),
        };
        let proc = TunnelProcess::spawn(launch, tunnel, self.ready_timeout, callbacks);
        let snapshot = proc.snapshot();
        lock(&self.entries).push(proc);
        snapshot
    }

    pub fn list(&self) -> Vec<Tunnel> {
        lock(&self.entries).iter().map(|p| p.snapshot()).collect()
    }

    fn find(&self, id: &str) -> Option<Arc<TunnelProcess>> {
        lock(&self.entries).iter().find(|p| p.id() == id).cloned()
    }

    /// Removes the tunnel, then waits for its process to end.
    pub async fn stop(&self, id: &str) -> bool {
        let removed = {
            let mut entries = lock(&self.entries);
            entries
                .iter()
                .position(|p| p.id() == id)
                .map(|i| entries.remove(i))
        };
        match removed {
            Some(proc) => {
                proc.stop().await;
                true
            }
            None => false,
        }
    }

    pub fn extend(&self, id: &str, now_ms: u64) -> Option<Tunnel> {
        self.find(id).map(|p| p.extend(now_ms))
    }

    pub fn output(&self, id: &str) -> Option<Vec<String>> {
        self.find(id).map(|p| p.output())
    }

    /// Stops and removes every tunnel whose `expires_at <= now_ms`.
    pub async fn expire(&self, now_ms: u64) -> Vec<Tunnel> {
        let due: Vec<Arc<TunnelProcess>> = {
            let mut entries = lock(&self.entries);
            let (due, kept) = entries
                .drain(..)
                .partition(|p| p.snapshot().expires_at <= now_ms);
            *entries = kept;
            due
        };
        let mut expired = Vec::with_capacity(due.len());
        for proc in due {
            proc.stop().await;
            expired.push(proc.snapshot());
        }
        expired
    }

    /// App exit: SIGKILL every child still running, without waiting.
    pub fn kill_all(&self) {
        let entries = std::mem::take(&mut *lock(&self.entries));
        for proc in entries {
            proc.kill();
        }
    }

    #[cfg(test)]
    fn pid(&self, id: &str) -> Option<u32> {
        self.find(id).and_then(|p| p.pid())
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub type TunnelsRef<'a> = tauri::State<'a, Arc<TunnelsState>>;

pub struct TunnelsState {
    pub manager: TunnelManager,
    /// Stable URLs remembered in `tunnels.json`.
    pub stable: Mutex<StableStore>,
    /// Serializes `cloudflared tunnel create` and `delete`.
    pub stable_ops: tokio::sync::Mutex<()>,
    /// Open requests in progress (stable creation can take a minute): port
    /// and, for a stable request, its folder.
    opening: Mutex<HashMap<u16, Option<String>>>,
}

/// "Tunnel closed: <project>" for each expired tunnel, except those already
/// in error: their failure was reported when it happened.
pub fn closed_notices(expired: &[Tunnel]) -> Vec<String> {
    expired
        .iter()
        .filter(|t| t.status != TunnelStatus::Error)
        .map(|t| format!("Tunnel closed: {}", t.project))
        .collect()
}

/// True when a tunnel that is not in error already serves `port`.
pub fn port_shared(tunnels: &[Tunnel], port: u16) -> bool {
    tunnels
        .iter()
        .any(|t| t.port == port && t.status != TunnelStatus::Error)
}

/// Port of the stable tunnel of `cwd` that is not in error, if any.
pub fn stable_cwd_port(tunnels: &[Tunnel], cwd: &str) -> Option<u16> {
    tunnels
        .iter()
        .find(|t| {
            t.kind == TunnelKind::Stable
                && t.cwd.as_deref() == Some(cwd)
                && t.status != TunnelStatus::Error
        })
        .map(|t| t.port)
}

/// Releases the reservation when the open request ends.
pub struct Reservation<'a> {
    state: &'a TunnelsState,
    port: u16,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        lock(&self.state.opening).remove(&self.port);
    }
}

impl TunnelsState {
    pub fn new(stable: StableStore) -> Self {
        Self {
            manager: TunnelManager::default(),
            stable: Mutex::new(stable),
            stable_ops: tokio::sync::Mutex::new(()),
            opening: Mutex::new(HashMap::new()),
        }
    }

    /// One tunnel per port, and one stable tunnel per folder (two runs of the
    /// same Cloudflare tunnel would serve the hostname from both ports).
    /// `stable` is `(cwd, hostname)` for a stable request.
    pub fn reserve(
        &self,
        port: u16,
        stable: Option<(&str, &str)>,
    ) -> Result<Reservation<'_>, String> {
        let mut opening = lock(&self.opening);
        let tunnels = self.manager.list();
        if opening.contains_key(&port) || port_shared(&tunnels, port) {
            return Err(format!("Port {port} is already shared."));
        }
        if let Some((cwd, hostname)) = stable {
            let in_progress = opening
                .iter()
                .find(|(_, c)| c.as_deref() == Some(cwd))
                .map(|(p, _)| *p);
            if let Some(used) = stable_cwd_port(&tunnels, cwd).or(in_progress) {
                return Err(format!("{hostname} is already shared on port {used}."));
            }
        }
        opening.insert(port, stable.map(|(cwd, _)| cwd.to_string()));
        Ok(Reservation { state: self, port })
    }

    /// The stable URL of `cwd` cannot be deleted while it is in use.
    pub fn check_deletable(&self, cwd: &str) -> Result<(), String> {
        if self.cwd_busy(cwd) {
            Err("Stop the tunnel first.".into())
        } else {
            Ok(())
        }
    }

    /// True while a stable tunnel of `cwd` is listed (not in error) or being opened.
    fn cwd_busy(&self, cwd: &str) -> bool {
        let opening = lock(&self.opening);
        opening.values().any(|c| c.as_deref() == Some(cwd))
            || stable_cwd_port(&self.manager.list(), cwd).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tunnels::process::{NOT_READY, STOPPED};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    const URL_LINE: &str = "echo 'INF |  https://shiny-fox.trycloudflare.com  |' >&2";
    const REG_LINE: &str = "echo 'INF Registered tunnel connection connIndex=0 location=cdg01'";

    fn fake(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("cloudflared");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn quick(binary: PathBuf) -> Launch {
        Launch {
            binary,
            args: vec![],
            kind: TunnelKind::Quick,
            known_url: None,
        }
    }

    /// -1 once the pid is reaped (ESRCH).
    fn gone(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == -1 }
    }

    async fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        cond()
    }

    fn status(m: &TunnelManager, id: &str) -> Option<TunnelStatus> {
        m.list().into_iter().find(|t| t.id == id).map(|t| t.status)
    }

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + 'static) {
        let n = Arc::new(AtomicUsize::new(0));
        let c = n.clone();
        (n, move || {
            c.fetch_add(1, Ordering::SeqCst);
        })
    }

    async fn start_quick(m: &TunnelManager, binary: &Path, hours: u8) -> Tunnel {
        m.start(
            quick(binary.to_path_buf()),
            3000,
            "shop".into(),
            Some("/w/shop".into()),
            hours,
            || {},
            |_| {},
        )
        .await
    }

    #[tokio::test]
    async fn quick_tunnel_opens_with_the_printed_url() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(
            dir.path(),
            &format!("sleep 0.2\n{URL_LINE}\n{REG_LINE}\nexec sleep 30"),
        );
        let m = TunnelManager::default();
        let (changes, on_change) = counter();
        let t = m
            .start(quick(bin), 3000, "shop".into(), None, 4, on_change, |_| {})
            .await;
        assert_eq!(t.status, TunnelStatus::Starting);
        assert_eq!(t.url, None);
        assert_eq!(t.expires_at, t.started_at + 4 * HOUR_MS);
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Open)).await);
        let open = m.list().remove(0);
        assert_eq!(
            open.url.as_deref(),
            Some("https://shiny-fox.trycloudflare.com")
        );
        assert!(changes.load(Ordering::SeqCst) >= 1);
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn quick_tunnel_waits_for_a_registered_connection() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(
            dir.path(),
            &format!("{URL_LINE}\nsleep 0.6\n{REG_LINE}\nexec sleep 30"),
        );
        let m = TunnelManager::default();
        let t = start_quick(&m, &bin, 4).await;
        assert!(
            wait_until(|| m
                .output(&t.id)
                .is_some_and(|o| o.iter().any(|l| l.contains("trycloudflare"))))
            .await
        );
        assert_eq!(status(&m, &t.id), Some(TunnelStatus::Starting));
        assert_eq!(m.list()[0].url, None);
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Open)).await);
        assert_eq!(
            m.list()[0].url.as_deref(),
            Some("https://shiny-fox.trycloudflare.com")
        );
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn quick_tunnel_opens_when_the_connection_comes_before_the_url() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(
            dir.path(),
            &format!("{REG_LINE}\nsleep 0.2\n{URL_LINE}\nexec sleep 30"),
        );
        let m = TunnelManager::default();
        let t = start_quick(&m, &bin, 4).await;
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Open)).await);
        assert_eq!(
            m.list()[0].url.as_deref(),
            Some("https://shiny-fox.trycloudflare.com")
        );
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn a_url_without_a_connection_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), &format!("{URL_LINE}\nexec sleep 30"));
        let m = TunnelManager {
            ready_timeout: Duration::from_millis(400),
            ..Default::default()
        };
        let t = start_quick(&m, &bin, 4).await;
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Error)).await);
        assert_eq!(m.list()[0].error.as_deref(), Some(NOT_READY));
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn stable_tunnel_opens_with_the_known_url() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(
            dir.path(),
            "echo 'INF Registered tunnel connection connIndex=0' >&2\nexec sleep 30",
        );
        let m = TunnelManager::default();
        let launch = Launch {
            binary: bin,
            args: vec![],
            kind: TunnelKind::Stable,
            known_url: Some("https://khalif-shop.example.dev".into()),
        };
        let t = m
            .start(launch, 3000, "shop".into(), None, 1, || {}, |_| {})
            .await;
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Open)).await);
        assert_eq!(
            m.list()[0].url.as_deref(),
            Some("https://khalif-shop.example.dev")
        );
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn readiness_timeout_errors_and_stops_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let m = TunnelManager {
            ready_timeout: Duration::from_millis(300),
            ..Default::default()
        };
        let t = start_quick(&m, &bin, 4).await;
        let pid = m.pid(&t.id).unwrap();
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Error)).await);
        assert_eq!(m.list()[0].error.as_deref(), Some(NOT_READY));
        assert!(wait_until(|| gone(pid)).await);
        assert_eq!(m.list().len(), 1, "a failed tunnel stays until stopped");
        assert!(m.stop(&t.id).await);
        assert!(m.list().is_empty());
    }

    #[tokio::test]
    async fn unexpected_exit_errors_and_reports_once() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), &format!("{URL_LINE}\nsleep 0.3\nexit 0"));
        let m = TunnelManager::default();
        let exits = Arc::new(Mutex::new(Vec::<Tunnel>::new()));
        let seen = exits.clone();
        let t = m
            .start(
                quick(bin),
                3000,
                "shop".into(),
                None,
                4,
                || {},
                move |t| seen.lock().unwrap().push(t),
            )
            .await;
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Error)).await);
        assert_eq!(m.list()[0].error.as_deref(), Some(STOPPED));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let exits = exits.lock().unwrap().clone();
        assert_eq!(exits.len(), 1);
        assert_eq!(exits[0].id, t.id);
        assert_eq!(exits[0].status, TunnelStatus::Error);
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn stop_ends_the_process_and_removes_the_tunnel() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(
            dir.path(),
            &format!("{URL_LINE}\n{REG_LINE}\nexec sleep 30"),
        );
        let m = TunnelManager::default();
        let (exits, _) = counter();
        let seen = exits.clone();
        let t = m
            .start(
                quick(bin),
                3000,
                "shop".into(),
                None,
                4,
                || {},
                move |_| {
                    seen.fetch_add(1, Ordering::SeqCst);
                },
            )
            .await;
        assert!(wait_until(|| status(&m, &t.id) == Some(TunnelStatus::Open)).await);
        let pid = m.pid(&t.id).unwrap();
        assert!(m.stop(&t.id).await);
        assert!(gone(pid));
        assert!(m.list().is_empty());
        assert!(!m.stop(&t.id).await);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            exits.load(Ordering::SeqCst),
            0,
            "a stop is not an unexpected exit"
        );
    }

    #[tokio::test]
    async fn extend_adds_an_hour_capped_at_24_hours_from_now() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let m = TunnelManager::default();
        let t = start_quick(&m, &bin, 23).await;
        let now = t.started_at;
        assert_eq!(t.expires_at, now + 23 * HOUR_MS);
        for _ in 0..3 {
            m.extend(&t.id, now).unwrap();
        }
        assert_eq!(m.list()[0].expires_at, now + 24 * HOUR_MS);
        assert_eq!(m.extend("nope", now), None);
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn extend_starts_from_now_when_the_tunnel_is_past_due() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let m = TunnelManager::default();
        let t = start_quick(&m, &bin, 1).await;
        let now = t.started_at + 3 * HOUR_MS;
        let extended = m.extend(&t.id, now).unwrap();
        assert_eq!(extended.expires_at, now + HOUR_MS);
        assert!(m.stop(&t.id).await);
    }

    #[tokio::test]
    async fn a_stop_during_the_exit_drain_is_not_reported_as_unexpected() {
        let dir = tempfile::tempdir().unwrap();
        // The background sleep keeps the pipes open: the monitor waits the
        // whole drain after the child exits.
        let bin = fake(dir.path(), &format!("{URL_LINE}\n(sleep 2 &)\nexit 0"));
        let m = TunnelManager::default();
        let (exits, _) = counter();
        let seen = exits.clone();
        let t = m
            .start(
                quick(bin),
                3000,
                "shop".into(),
                None,
                4,
                || {},
                move |_| {
                    seen.fetch_add(1, Ordering::SeqCst);
                },
            )
            .await;
        let pid = m.pid(&t.id).unwrap();
        assert!(wait_until(|| gone(pid)).await);
        assert!(m.stop(&t.id).await);
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(exits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn expire_stops_due_tunnels_and_keeps_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let m = TunnelManager::default();
        let due = start_quick(&m, &bin, 1).await;
        let kept = start_quick(&m, &bin, 4).await;
        let due_pid = m.pid(&due.id).unwrap();
        let expired = m.expire(due.started_at + 2 * HOUR_MS).await;
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, due.id);
        assert!(gone(due_pid));
        let ids: Vec<String> = m.list().into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![kept.id.clone()]);
        assert!(m.stop(&kept.id).await);
    }

    #[tokio::test]
    async fn an_expired_tunnel_in_error_is_removed_without_a_closed_notice() {
        let dir = tempfile::tempdir().unwrap();
        let failed_bin = fake(dir.path(), "exit 0");
        let m = TunnelManager::default();
        let failed = start_quick(&m, &failed_bin, 1).await;
        assert!(wait_until(|| status(&m, &failed.id) == Some(TunnelStatus::Error)).await);
        let live_dir = tempfile::tempdir().unwrap();
        let live_bin = fake(live_dir.path(), "exec sleep 30");
        let live = start_quick(&m, &live_bin, 1).await;
        let expired = m.expire(failed.started_at + 2 * HOUR_MS).await;
        assert_eq!(expired.len(), 2);
        assert!(m.list().is_empty());
        assert_eq!(closed_notices(&expired), vec!["Tunnel closed: shop"]);
        assert!(expired
            .iter()
            .any(|t| t.id == live.id && t.status != TunnelStatus::Error));
    }

    #[test]
    fn closed_notices_skip_tunnels_in_error() {
        let list = [
            listed(3000, TunnelStatus::Open),
            listed(4000, TunnelStatus::Error),
            listed(5000, TunnelStatus::Starting),
        ];
        assert_eq!(
            closed_notices(&list),
            vec!["Tunnel closed: shop", "Tunnel closed: shop"]
        );
        assert!(closed_notices(&[listed(4000, TunnelStatus::Error)]).is_empty());
    }

    #[tokio::test]
    async fn kill_all_leaves_no_process_running() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let m = TunnelManager::default();
        let a = start_quick(&m, &bin, 4).await;
        let b = start_quick(&m, &bin, 4).await;
        let pids = [m.pid(&a.id).unwrap(), m.pid(&b.id).unwrap()];
        m.kill_all();
        assert!(wait_until(|| pids.iter().all(|&p| gone(p))).await);
        assert!(m.list().is_empty());
    }

    #[tokio::test]
    async fn output_keeps_the_last_200_lines() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(
            dir.path(),
            "i=1\nwhile [ $i -le 250 ]; do echo \"line $i\"; i=$((i+1)); done\nexec sleep 30",
        );
        let m = TunnelManager::default();
        let t = start_quick(&m, &bin, 4).await;
        assert!(
            wait_until(|| m
                .output(&t.id)
                .is_some_and(|o| o.last().map(String::as_str) == Some("line 250")))
            .await
        );
        let out = m.output(&t.id).unwrap();
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], "line 51");
        assert_eq!(m.output("nope"), None);
        assert!(m.stop(&t.id).await);
    }

    fn listed(port: u16, status: TunnelStatus) -> Tunnel {
        Tunnel {
            id: format!("t{port}"),
            kind: TunnelKind::Quick,
            port,
            project: "shop".into(),
            cwd: None,
            url: None,
            status,
            error: None,
            started_at: 0,
            expires_at: 1,
        }
    }

    #[test]
    fn port_shared_ignores_failed_tunnels_and_other_ports() {
        let list = [
            listed(3000, TunnelStatus::Open),
            listed(4000, TunnelStatus::Error),
            listed(5000, TunnelStatus::Starting),
        ];
        assert!(port_shared(&list, 3000));
        assert!(port_shared(&list, 5000));
        assert!(!port_shared(&list, 4000));
        assert!(!port_shared(&list, 6000));
        assert!(!port_shared(&[], 3000));
    }

    #[tokio::test]
    async fn reserve_port_refuses_a_second_open_on_the_same_port() {
        let state = TunnelsState::new(StableStore::default());
        let guard = state.reserve(3000, None).unwrap();
        assert_eq!(
            state.reserve(3000, None).err(),
            Some("Port 3000 is already shared.".to_string())
        );
        assert!(state.reserve(3001, None).is_ok());
        drop(guard);
        assert!(state.reserve(3000, None).is_ok());

        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let t = start_quick(&state.manager, &bin, 4).await;
        assert_eq!(
            state.reserve(3000, None).err(),
            Some("Port 3000 is already shared.".to_string())
        );
        assert!(state.manager.stop(&t.id).await);
        assert!(state.reserve(3000, None).is_ok());
    }

    fn stable_listed(port: u16, cwd: &str, status: TunnelStatus) -> Tunnel {
        Tunnel {
            kind: TunnelKind::Stable,
            cwd: Some(cwd.into()),
            ..listed(port, status)
        }
    }

    #[test]
    fn stable_cwd_port_finds_an_active_stable_tunnel_of_the_folder() {
        let list = [
            listed(3000, TunnelStatus::Open),
            stable_listed(4000, "/w/a", TunnelStatus::Error),
            stable_listed(5000, "/w/b", TunnelStatus::Starting),
        ];
        assert_eq!(stable_cwd_port(&list, "/w/b"), Some(5000));
        assert_eq!(stable_cwd_port(&list, "/w/a"), None);
        assert_eq!(stable_cwd_port(&list, "/w/c"), None);
    }

    #[tokio::test]
    async fn reserve_refuses_a_second_stable_open_for_the_same_folder() {
        let host = "khalif-shop.dev.example.com";
        let state = TunnelsState::new(StableStore::default());
        let guard = state.reserve(3000, Some(("/w/shop", host))).unwrap();
        assert_eq!(
            state.reserve(3001, Some(("/w/shop", host))).err(),
            Some(format!("{host} is already shared on port 3000."))
        );
        let stop_first = Err("Stop the tunnel first.".to_string());
        assert_eq!(state.check_deletable("/w/shop"), stop_first);
        assert_eq!(state.check_deletable("/w/other"), Ok(()));
        // A quick tunnel of the same folder on another port is allowed.
        assert!(state.reserve(3001, None).is_ok());
        assert!(state
            .reserve(3002, Some(("/w/other", "o.dev.example.com")))
            .is_ok());
        drop(guard);
        assert_eq!(state.check_deletable("/w/shop"), Ok(()));
        assert!(state.reserve(3001, Some(("/w/shop", host))).is_ok());

        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "exec sleep 30");
        let launch = Launch {
            binary: bin,
            args: vec![],
            kind: TunnelKind::Stable,
            known_url: None,
        };
        let t = state
            .manager
            .start(
                launch,
                4000,
                "shop".into(),
                Some("/w/shop".into()),
                4,
                || {},
                |_| {},
            )
            .await;
        assert_eq!(state.check_deletable("/w/shop"), stop_first);
        assert_eq!(
            state.reserve(3001, Some(("/w/shop", host))).err(),
            Some(format!("{host} is already shared on port 4000."))
        );
        assert!(state.manager.stop(&t.id).await);
        assert_eq!(state.check_deletable("/w/shop"), Ok(()));
    }

    #[tokio::test]
    async fn a_missing_binary_gives_an_error_tunnel() {
        let dir = tempfile::tempdir().unwrap();
        let m = TunnelManager::default();
        let t = start_quick(&m, &dir.path().join("missing"), 4).await;
        assert_eq!(t.status, TunnelStatus::Error);
        assert!(t
            .error
            .as_deref()
            .unwrap()
            .starts_with("cloudflared could not start: "));
        assert!(m.stop(&t.id).await);
    }
}
