//! Background collection loop: one pass every `pollIntervalMs`, or sooner
//! when something calls `services.refresh.notify_one()`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bollard::Docker;
use tauri::{AppHandle, Emitter};

use super::model::{ProcessService, Snapshot};
use super::state::ServicesState;
use super::{compose, docker_stats as docker, model, probe, processes, sockets};
use crate::core::git::WorktreeCache;
use crate::core::procs::ProcReader;
use crate::core::state::CoreState;

/// Bound on connecting to Docker and listing containers; beyond it Docker is
/// treated as unreachable for this pass. Stats come after, each bounded by
/// `docker::STATS_TIMEOUT` (1 s) and run in parallel, so Docker delays a pass
/// by at most 3 s and a slow stats call never escalates to "unreachable".
const DOCKER_BUDGET: Duration = Duration::from_secs(2);
const WORKTREE_TTL: Duration = Duration::from_secs(30);

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Collector {
    home: String,
    reader: Arc<Mutex<ProcReader>>,
    worktrees: Arc<Mutex<WorktreeCache>>,
    docker: docker::DockerCollector,
    events: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Collector {
    pub fn new(home: String) -> Self {
        Self {
            home,
            reader: Arc::new(Mutex::new(ProcReader::new())),
            worktrees: Arc::new(Mutex::new(WorktreeCache::new(WORKTREE_TTL))),
            docker: docker::DockerCollector::default(),
            events: None,
        }
    }

    pub async fn pass(
        &mut self,
        core: &Arc<CoreState>,
        services: &Arc<ServicesState>,
    ) -> Result<Snapshot, String> {
        let show_all = core.settings.read().unwrap().show_all;
        let managed = {
            let mut registry = services.managed.lock().unwrap();
            registry.retain_alive(crate::core::kill::is_alive);
            registry.pids()
        };
        let (reader, worktrees, home) = (
            self.reader.clone(),
            self.worktrees.clone(),
            self.home.clone(),
        );
        let mut processes = tokio::task::spawn_blocking(move || {
            collect_processes(&reader, &worktrees, &home, show_all, &managed)
        })
        .await
        .map_err(|e| format!("collector task failed: {e}"))??;

        let mut docker_available = false;
        let mut containers = Vec::new();
        match tokio::time::timeout(DOCKER_BUDGET, self.docker_pass(core, services)).await {
            Ok(Some((docker, raws))) => {
                containers = self.docker.with_stats(&docker, raws).await;
                docker_available = true;
            }
            Ok(None) => {}
            Err(_) => {
                eprintln!(
                    "LsDev docker: no answer within {} s, skipping Docker for this pass",
                    DOCKER_BUDGET.as_secs()
                );
                *core.docker.write().unwrap() = None;
            }
        }

        let ports: Vec<u16> = processes
            .iter()
            .flat_map(|p| p.ports.iter().map(|x| x.port))
            .chain(
                containers
                    .iter()
                    .filter(|c| c.state == "running")
                    .flat_map(|c| c.ports.iter().map(|x| x.port)),
            )
            .collect();
        let probe = probe::probe_ports(ports).await;
        for p in &mut processes {
            for port in &mut p.ports {
                port.responsive = probe.get(&port.port).copied();
            }
        }
        for c in containers.iter_mut().filter(|c| c.state == "running") {
            for port in &mut c.ports {
                port.responsive = probe.get(&port.port).copied();
            }
        }

        let (compose, standalone) = compose::group(containers);
        Ok(Snapshot {
            taken_at: now_ms(),
            docker_available,
            processes,
            compose,
            standalone,
        })
    }

    /// Connect and list: the part bounded by `DOCKER_BUDGET`.
    async fn docker_pass(
        &mut self,
        core: &Arc<CoreState>,
        services: &Arc<ServicesState>,
    ) -> Option<(Docker, Vec<model::RawContainer>)> {
        let docker = self.ensure_docker(core, services).await?;
        match docker::DockerCollector::list(&docker).await {
            Ok(list) => Some((docker, list)),
            Err(e) => {
                eprintln!("LsDev docker: listing containers failed: {e}");
                *core.docker.write().unwrap() = None;
                None
            }
        }
    }

    async fn ensure_docker(
        &mut self,
        core: &Arc<CoreState>,
        services: &Arc<ServicesState>,
    ) -> Option<Docker> {
        let cached = core.docker.read().unwrap().clone();
        if let Some(docker) = cached {
            if watcher_needs_respawn(self.events.as_ref()) {
                self.events = Some(docker::spawn_event_watcher(
                    docker.clone(),
                    services.clone(),
                ));
            }
            return Some(docker);
        }
        let docker = crate::core::docker::connect(&self.home).await?;
        *core.docker.write().unwrap() = Some(docker.clone());
        if let Some(handle) = self.events.take() {
            handle.abort();
        }
        self.events = Some(docker::spawn_event_watcher(
            docker.clone(),
            services.clone(),
        ));
        Some(docker)
    }
}

fn watcher_needs_respawn(handle: Option<&tauri::async_runtime::JoinHandle<()>>) -> bool {
    handle.map_or(true, |h| h.inner().is_finished())
}

/// Blocking part of a pass: sysinfo, sockets, git.
fn collect_processes(
    reader: &Mutex<ProcReader>,
    worktrees: &Mutex<WorktreeCache>,
    home: &str,
    show_all: bool,
    managed: &std::collections::HashSet<u32>,
) -> Result<Vec<ProcessService>, String> {
    let procs = reader.lock().unwrap().read();
    let listeners = sockets::list_listeners(&procs)?;
    let mut rows = processes::assemble_processes(&listeners, &procs, home, now_secs(), show_all);
    processes::hide_own_session(&mut rows, &procs, std::process::id());
    processes::mark_managed(&mut rows, &procs, managed);
    let mut cache = worktrees.lock().unwrap();
    let now = Instant::now();
    for row in &mut rows {
        if let Some(cwd) = row.cwd.clone() {
            row.worktree = cache.get(&cwd, now, crate::core::git::detect);
        }
    }
    Ok(rows)
}

pub fn spawn(app: AppHandle, core: Arc<CoreState>, services: Arc<ServicesState>) {
    tauri::async_runtime::spawn(async move {
        let mut collector = Collector::new(std::env::var("HOME").unwrap_or_default());
        let mut stops = crate::tools::services::stops::StopTracker::default();
        loop {
            match collector.pass(&core, &services).await {
                Ok(snapshot) => publish(&app, &core, &services, &mut stops, snapshot),
                Err(message) => {
                    let _ = app.emit("collect:error", serde_json::json!({ "message": message }));
                }
            }
            let interval = core.settings.read().unwrap().poll_interval_ms;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(interval)) => {}
                _ = services.refresh.notified() => {}
            }
        }
    });
}

pub fn publish(
    app: &AppHandle,
    core: &CoreState,
    services: &ServicesState,
    stops: &mut crate::tools::services::stops::StopTracker,
    snapshot: Snapshot,
) {
    let prev = std::mem::replace(&mut *services.snapshot.write().unwrap(), snapshot.clone());
    // The tracker advances on every pass so that turning notifications on
    // never reports a stop older than one pass.
    let suppressed = services.suppress.lock().unwrap().active(Instant::now());
    let messages = stops.step(&prev, &snapshot, &suppressed);
    if core.settings.read().unwrap().notify_on_stop {
        for body in crate::tools::services::stops::collapse(messages) {
            crate::core::notify::send(app, &body);
        }
    }
    let _ = app.emit("services:changed", &snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::Settings;

    #[test]
    fn slow_stats_time_out_before_docker_counts_as_unreachable() {
        assert!(docker::STATS_TIMEOUT < DOCKER_BUDGET);
        assert!(DOCKER_BUDGET + docker::STATS_TIMEOUT <= Duration::from_secs(3));
    }

    #[tokio::test]
    async fn watcher_respawns_when_absent_or_finished() {
        assert!(watcher_needs_respawn(None));
        let done = tauri::async_runtime::spawn(async {});
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(watcher_needs_respawn(Some(&done)));
        let running = tauri::async_runtime::spawn(std::future::pending::<()>());
        assert!(!watcher_needs_respawn(Some(&running)));
        running.abort();
    }

    /// `cargo test --manifest-path src-tauri/Cargo.toml live_pass -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "reads the live machine"]
    async fn live_pass_prints_a_snapshot() {
        let core = Arc::new(CoreState::new(Settings::default()));
        let services = Arc::new(ServicesState::new());
        let mut collector = Collector::new(std::env::var("HOME").unwrap());
        let snapshot = collector.pass(&core, &services).await.expect("pass");
        println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
    }
}
