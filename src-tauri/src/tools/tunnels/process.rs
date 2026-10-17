//! One cloudflared child: launch, readiness, last output lines, stop.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::cloudflared::{is_registered, quick_url};
use crate::core::kill::signal;

pub const OUTPUT_LINES: usize = 200;
pub const NOT_READY: &str = "cloudflared did not open the tunnel within 30 s.";
pub const STOPPED: &str = "cloudflared stopped.";
pub const HOUR_MS: u64 = 3_600_000;
const STOP_GRACE: Duration = Duration::from_secs(5);
const DRAIN: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TunnelKind {
    Quick,
    Stable,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TunnelStatus {
    Starting,
    Open,
    Error,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tunnel {
    pub id: String,
    pub kind: TunnelKind,
    pub port: u16,
    pub project: String,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub status: TunnelStatus,
    pub error: Option<String>,
    pub started_at: u64,
    pub expires_at: u64,
}

/// What to run. `known_url` is `https://<hostname>` for a stable tunnel.
#[derive(Clone, Debug)]
pub struct Launch {
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub kind: TunnelKind,
    pub known_url: Option<String>,
}

pub fn quick_args(port: u16) -> Vec<String> {
    [
        "tunnel",
        "--no-autoupdate",
        "--url",
        &format!("http://localhost:{port}"),
        "--http-host-header",
        "localhost",
    ]
    .map(String::from)
    .to_vec()
}

pub fn stable_args(port: u16, tunnel_id: &str) -> Vec<String> {
    [
        "tunnel",
        "--no-autoupdate",
        "run",
        "--url",
        &format!("http://localhost:{port}"),
        "--http-host-header",
        "localhost",
        tunnel_id,
    ]
    .map(String::from)
    .to_vec()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub type OnChange = Arc<dyn Fn() + Send + Sync>;
pub type OnExit = Arc<dyn Fn(Tunnel) + Send + Sync>;

pub struct Callbacks {
    pub on_change: OnChange,
    pub on_unexpected_exit: OnExit,
}

type StopRequest = oneshot::Sender<()>;

pub struct TunnelProcess {
    tunnel: Mutex<Tunnel>,
    output: Mutex<VecDeque<String>>,
    pid: Option<u32>,
    /// Set once the child has been reaped: its pid may then be reused.
    exited: AtomicBool,
    /// Set by stop, expiry, kill_all and the readiness timeout: the exit
    /// that follows is not reported as unexpected.
    stopping: AtomicBool,
    stop_tx: Mutex<Option<oneshot::Sender<StopRequest>>>,
    /// Readiness signs seen so far, in any order and on either stream.
    progress: Mutex<Progress>,
}

/// A quick tunnel is open once its URL and a registered connection have
/// both been printed; a stable tunnel at its first registered connection.
#[derive(Default)]
struct Progress {
    url: Option<String>,
    registered: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl TunnelProcess {
    fn new(
        tunnel: Tunnel,
        pid: Option<u32>,
        stop_tx: Option<oneshot::Sender<StopRequest>>,
    ) -> Self {
        Self {
            tunnel: Mutex::new(tunnel),
            output: Mutex::new(VecDeque::with_capacity(OUTPUT_LINES)),
            pid,
            exited: AtomicBool::new(pid.is_none()),
            stopping: AtomicBool::new(false),
            stop_tx: Mutex::new(stop_tx),
            progress: Mutex::new(Progress::default()),
        }
    }

    /// Spawns cloudflared without a shell. A spawn failure gives a tunnel
    /// in `Error` that stays listed until stopped.
    pub fn spawn(
        launch: Launch,
        mut tunnel: Tunnel,
        ready_timeout: Duration,
        callbacks: Callbacks,
    ) -> Arc<Self> {
        let spawned = Command::new(&launch.binary)
            .args(&launch.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                tunnel.status = TunnelStatus::Error;
                tunnel.error = Some(format!("cloudflared could not start: {e}"));
                return Arc::new(Self::new(tunnel, None, None));
            }
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let me = Arc::new(Self::new(tunnel, child.id(), Some(stop_tx)));
        let ready = Readiness {
            kind: launch.kind,
            known_url: launch.known_url,
        };
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(tokio::spawn(read_lines(
                out,
                me.clone(),
                ready.clone(),
                callbacks.on_change.clone(),
            )));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(tokio::spawn(read_lines(
                err,
                me.clone(),
                ready,
                callbacks.on_change.clone(),
            )));
        }
        tokio::spawn(monitor(
            me.clone(),
            child,
            stop_rx,
            readers,
            ready_timeout,
            callbacks,
        ));
        me
    }

    pub fn snapshot(&self) -> Tunnel {
        lock(&self.tunnel).clone()
    }

    pub fn id(&self) -> String {
        lock(&self.tunnel).id.clone()
    }

    #[cfg(test)]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn output(&self) -> Vec<String> {
        lock(&self.output).iter().cloned().collect()
    }

    /// +1 h from the later of the deadline and `now_ms`, at most 24 h after
    /// `now_ms`.
    pub fn extend(&self, now_ms: u64) -> Tunnel {
        let mut t = lock(&self.tunnel);
        t.expires_at = (t.expires_at.max(now_ms) + HOUR_MS).min(now_ms + 24 * HOUR_MS);
        t.clone()
    }

    /// SIGTERM, up to 5 s, then SIGKILL; returns once the child is reaped.
    pub async fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let tx = lock(&self.stop_tx).take();
        if let Some(tx) = tx {
            let (done_tx, done_rx) = oneshot::channel();
            if tx.send(done_tx).is_ok() {
                let _ = done_rx.await;
            }
        }
    }

    /// Synchronous SIGKILL for app exit; the monitor task reaps the child.
    pub fn kill(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(pid) = self.pid {
            if !self.exited.load(Ordering::SeqCst) {
                signal(pid, libc::SIGKILL);
            }
        }
    }

    /// Appends a line and applies the readiness rule. True when the status
    /// changed.
    fn record(&self, line: String, ready: &Readiness) -> bool {
        let url = {
            let mut p = lock(&self.progress);
            if p.url.is_none() {
                p.url = match ready.kind {
                    TunnelKind::Quick => quick_url(&line),
                    TunnelKind::Stable => ready.known_url.clone(),
                };
            }
            p.registered |= is_registered(&line);
            p.registered.then(|| p.url.clone()).flatten()
        };
        {
            let mut out = lock(&self.output);
            if out.len() == OUTPUT_LINES {
                out.pop_front();
            }
            out.push_back(line);
        }
        let Some(url) = url else { return false };
        let mut t = lock(&self.tunnel);
        if t.status != TunnelStatus::Starting {
            return false;
        }
        t.url = Some(url);
        t.status = TunnelStatus::Open;
        true
    }

    /// Moves to `Error` unless already there; returns the new state.
    fn fail(&self, message: &str, only_if_starting: bool) -> Option<Tunnel> {
        let mut t = lock(&self.tunnel);
        let allowed = match t.status {
            TunnelStatus::Starting => true,
            TunnelStatus::Open => !only_if_starting,
            TunnelStatus::Error => false,
        };
        if !allowed {
            return None;
        }
        t.status = TunnelStatus::Error;
        t.error = Some(message.to_string());
        Some(t.clone())
    }
}

#[derive(Clone)]
struct Readiness {
    kind: TunnelKind,
    known_url: Option<String>,
}

/// Lossy line reader: invalid UTF-8 must not stop the read, or the pipe
/// would fill and block cloudflared.
async fn read_lines<R>(stream: R, proc: Arc<TunnelProcess>, ready: Readiness, on_change: OnChange)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf)
            .trim_end_matches(['\n', '\r'])
            .to_string();
        if proc.record(line, &ready) {
            on_change();
        }
    }
}

/// Owns the child: waits for its exit, a stop request or the readiness
/// deadline, whichever comes first.
async fn monitor(
    proc: Arc<TunnelProcess>,
    mut child: Child,
    mut stop_rx: oneshot::Receiver<StopRequest>,
    readers: Vec<JoinHandle<()>>,
    ready_timeout: Duration,
    callbacks: Callbacks,
) {
    let deadline = tokio::time::sleep(ready_timeout);
    tokio::pin!(deadline);
    let mut deadline_armed = true;
    loop {
        tokio::select! {
            _ = child.wait() => {
                proc.exited.store(true, Ordering::SeqCst);
                if proc.stopping.load(Ordering::SeqCst) {
                    return;
                }
                // Let the readers record the last lines before reporting.
                for reader in readers {
                    let _ = tokio::time::timeout(DRAIN, reader).await;
                }
                // A stop or an expiry during the drain is not unexpected.
                if proc.stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(t) = proc.fail(STOPPED, false) {
                    (callbacks.on_change)();
                    (callbacks.on_unexpected_exit)(t);
                }
                return;
            }
            _ = &mut deadline, if deadline_armed => {
                deadline_armed = false;
                if proc.fail(NOT_READY, true).is_some() {
                    terminate(&proc, &mut child).await;
                    (callbacks.on_change)();
                    return;
                }
            }
            request = &mut stop_rx => {
                terminate(&proc, &mut child).await;
                if let Ok(done) = request {
                    let _ = done.send(());
                }
                return;
            }
        }
    }
}

async fn terminate(proc: &TunnelProcess, child: &mut Child) {
    proc.stopping.store(true, Ordering::SeqCst);
    // `id()` is None once reaped, so a reused pid is never signalled.
    if let Some(pid) = child.id() {
        signal(pid, libc::SIGTERM);
    }
    if tokio::time::timeout(STOP_GRACE, child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
    proc.exited.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_args_are_the_exact_list() {
        assert_eq!(
            quick_args(3000),
            [
                "tunnel",
                "--no-autoupdate",
                "--url",
                "http://localhost:3000",
                "--http-host-header",
                "localhost"
            ]
        );
    }

    #[test]
    fn stable_args_are_the_exact_list() {
        assert_eq!(
            stable_args(5173, "6ff42ae2-765d-4adf-8112-31c55c1551ef"),
            [
                "tunnel",
                "--no-autoupdate",
                "run",
                "--url",
                "http://localhost:5173",
                "--http-host-header",
                "localhost",
                "6ff42ae2-765d-4adf-8112-31c55c1551ef"
            ]
        );
    }

    #[test]
    fn serde_uses_lowercase_kinds_and_camel_case_fields() {
        let t = Tunnel {
            id: "a".into(),
            kind: TunnelKind::Stable,
            port: 3000,
            project: "shop".into(),
            cwd: None,
            url: None,
            status: TunnelStatus::Starting,
            error: None,
            started_at: 1,
            expires_at: 2,
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["kind"], "stable");
        assert_eq!(v["status"], "starting");
        assert_eq!(v["startedAt"], 1);
        assert_eq!(v["expiresAt"], 2);
        let k: TunnelKind = serde_json::from_str("\"quick\"").unwrap();
        assert_eq!(k, TunnelKind::Quick);
    }
}
