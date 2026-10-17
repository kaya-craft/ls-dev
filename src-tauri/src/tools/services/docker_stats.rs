//! Docker through its socket (bollard): containers, stats, events.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bollard::container::{ListContainersOptions, StatsOptions};
use bollard::system::EventsOptions;
use bollard::Docker;
use futures_util::StreamExt;

use super::compose::{cpu_percent, to_service, ContainerUsage, CpuSample};
use super::model::{ContainerService, RawContainer};
use super::state::ServicesState;
use crate::core::docker::to_raw;

/// Per-container stats call. Stats run after the bounded connect + list step
/// (`DOCKER_BUDGET` in mod.rs), in parallel: a slow container only loses its
/// CPU and memory figures and never makes Docker look unreachable.
pub const STATS_TIMEOUT: Duration = Duration::from_secs(1);
const REFRESH_ACTIONS: [&str; 7] = [
    "start", "stop", "die", "destroy", "create", "pause", "unpause",
];

/// Keeps the previous CPU sample of each container to compute a percentage.
#[derive(Default)]
pub struct DockerCollector {
    prev_cpu: HashMap<String, CpuSample>,
}

impl DockerCollector {
    pub async fn list(docker: &Docker) -> Result<Vec<RawContainer>, String> {
        let summaries = docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await
            .map_err(|e| e.to_string())?;
        Ok(summaries.iter().map(to_raw).collect())
    }

    /// Adds stats to listed containers; each call is bounded by `STATS_TIMEOUT`.
    pub async fn with_stats(
        &mut self,
        docker: &Docker,
        raws: Vec<RawContainer>,
    ) -> Vec<ContainerService> {
        let samples = futures_util::future::join_all(raws.iter().map(|raw| async move {
            if raw.state != "running" {
                return None;
            }
            one_stats(docker, &raw.id).await
        }))
        .await;

        let mut next_prev = HashMap::new();
        let mut out = Vec::with_capacity(raws.len());
        for (raw, sample) in raws.iter().zip(samples) {
            let usage = sample.map(|(cpu, mem)| {
                let pct = self
                    .prev_cpu
                    .get(&raw.id)
                    .and_then(|prev| cpu_percent(*prev, cpu));
                next_prev.insert(raw.id.clone(), cpu);
                ContainerUsage {
                    cpu_percent: pct,
                    mem_bytes: mem,
                }
            });
            out.push(to_service(raw, usage.as_ref()));
        }
        self.prev_cpu = next_prev;
        out
    }
}

async fn one_stats(docker: &Docker, id: &str) -> Option<(CpuSample, Option<u64>)> {
    let mut stream = docker.stats(
        id,
        Some(StatsOptions {
            stream: false,
            one_shot: true,
        }),
    );
    let stats = tokio::time::timeout(STATS_TIMEOUT, stream.next())
        .await
        .ok()??
        .ok()?;
    Some((
        CpuSample {
            total: stats.cpu_stats.cpu_usage.total_usage,
            system: stats.cpu_stats.system_cpu_usage.unwrap_or(0),
            online_cpus: stats.cpu_stats.online_cpus.unwrap_or(1),
        },
        stats.memory_stats.usage,
    ))
}

/// Ask for an immediate pass on every container lifecycle event.
pub fn spawn_event_watcher(
    docker: Docker,
    services: Arc<ServicesState>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let filters = HashMap::from([("type".to_string(), vec!["container".to_string()])]);
        let mut events = docker.events(Some(EventsOptions::<String> {
            filters,
            ..Default::default()
        }));
        while let Some(event) = events.next().await {
            match event {
                Ok(message) => {
                    if message
                        .action
                        .as_deref()
                        .is_some_and(|a| REFRESH_ACTIONS.contains(&a))
                    {
                        services.refresh.notify_one();
                    }
                }
                Err(_) => break,
            }
        }
    })
}
