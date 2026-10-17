//! State of the Services tool, shared by the collector and the commands.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use super::logs::LogSession;
use super::model::{ComposeGroup, ContainerService, ProcessService, Snapshot};

const SUPPRESS_FOR: Duration = Duration::from_secs(10);

pub type ServicesRef<'a> = tauri::State<'a, Arc<ServicesState>>;

pub struct ServicesState {
    pub snapshot: RwLock<Snapshot>,
    /// `notify_one()` asks the collector for an immediate pass; several
    /// calls before the pass starts collapse into one.
    pub refresh: Notify,
    pub managed: Mutex<ManagedRegistry>,
    pub logs: Mutex<Option<LogSession>>,
    pub suppress: Mutex<Suppressions>,
}

impl Default for ServicesState {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicesState {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(Snapshot::default()),
            refresh: Notify::new(),
            managed: Mutex::new(ManagedRegistry::default()),
            logs: Mutex::new(None),
            suppress: Mutex::new(Suppressions::default()),
        }
    }

    pub fn find_process(&self, id: &str) -> Option<ProcessService> {
        self.snapshot
            .read()
            .unwrap()
            .processes
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    pub fn find_container(&self, id: &str) -> Option<ContainerService> {
        let snapshot = self.snapshot.read().unwrap();
        snapshot
            .compose
            .iter()
            .flat_map(|g| g.containers.iter())
            .chain(snapshot.standalone.iter())
            .find(|c| c.id == id)
            .cloned()
    }

    pub fn compose_group(&self, project: &str) -> Option<ComposeGroup> {
        self.snapshot
            .read()
            .unwrap()
            .compose
            .iter()
            .find(|g| g.project == project)
            .cloned()
    }

    /// No "stopped" notification for this id during the next 10 seconds.
    pub fn suppress_id(&self, id: &str) {
        self.suppress.lock().unwrap().add(id, Instant::now());
    }
}

/// Processes relaunched by LsDev: pid -> log file.
#[derive(Debug, Default)]
pub struct ManagedRegistry {
    logs: HashMap<u32, PathBuf>,
}

impl ManagedRegistry {
    pub fn insert(&mut self, pid: u32, log: PathBuf) {
        self.logs.insert(pid, log);
    }
    pub fn log_path(&self, pid: u32) -> Option<PathBuf> {
        self.logs.get(&pid).cloned()
    }
    pub fn pids(&self) -> HashSet<u32> {
        self.logs.keys().copied().collect()
    }
    pub fn retain_alive(&mut self, alive: impl Fn(u32) -> bool) {
        self.logs.retain(|pid, _| alive(*pid));
    }
}

/// Service ids stopped on purpose, with the instant until which they stay muted.
#[derive(Debug, Default)]
pub struct Suppressions {
    until: HashMap<String, Instant>,
}

impl Suppressions {
    pub fn add(&mut self, id: &str, now: Instant) {
        self.until.insert(id.to_string(), now + SUPPRESS_FOR);
    }
    pub fn active(&mut self, now: Instant) -> HashSet<String> {
        self.until.retain(|_, t| *t > now);
        self.until.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{container, process};

    fn state_with_snapshot() -> ServicesState {
        let state = ServicesState::new();
        *state.snapshot.write().unwrap() = Snapshot {
            taken_at: 1,
            docker_available: true,
            processes: vec![process("proc:1:/a", "a", 3000)],
            compose: vec![ComposeGroup {
                project: "p".into(),
                working_dir: None,
                containers: vec![container("c1", "p-db-1", "running", Some("p"))],
            }],
            standalone: vec![container("c2", "solo", "exited", None)],
        };
        state
    }

    #[test]
    fn finds_services_by_id() {
        let s = state_with_snapshot();
        assert_eq!(s.find_process("proc:1:/a").unwrap().project, "a");
        assert_eq!(s.find_container("docker:c1").unwrap().name, "p-db-1");
        assert_eq!(s.find_container("docker:c2").unwrap().name, "solo");
        assert!(s.find_process("proc:2:/b").is_none());
        assert_eq!(s.compose_group("p").unwrap().containers.len(), 1);
    }

    #[test]
    fn suppressions_expire_after_ten_seconds() {
        let mut s = Suppressions::default();
        let t0 = Instant::now();
        s.add("x", t0);
        assert!(s.active(t0 + Duration::from_secs(9)).contains("x"));
        assert!(!s.active(t0 + Duration::from_secs(11)).contains("x"));
    }

    #[test]
    fn managed_registry_tracks_live_pids() {
        let mut r = ManagedRegistry::default();
        r.insert(10, PathBuf::from("/logs/a.log"));
        r.insert(11, PathBuf::from("/logs/b.log"));
        assert_eq!(r.log_path(10), Some(PathBuf::from("/logs/a.log")));
        r.retain_alive(|pid| pid == 11);
        assert_eq!(r.log_path(10), None);
        assert_eq!(r.pids(), HashSet::from([11]));
    }
}
