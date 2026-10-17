//! Data types shared by the collector, the actions and the frontend.
//! Serialized types use camelCase to match `src/types.ts`.

use serde::{Deserialize, Serialize};

pub use crate::core::docker::{RawContainer, RawPort};
pub use crate::core::git::Worktree;
pub use crate::core::procs::ProcTable;

/// A process holding at least one listening TCP socket.
#[derive(Debug, Clone, PartialEq)]
pub struct Listener {
    pub pid: u32,
    pub name: String,
    /// Sorted, deduplicated.
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    pub port: u16,
    /// None when not probed (stopped container).
    pub responsive: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessKind {
    Dev,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub pid: u32,
    pub command: String,
    /// 0 for the launcher, 1 for its children, ...
    pub depth: u32,
}

/// What `restart_process` relaunches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartSpec {
    /// Package-manager launchers: readable argv (no interpreter, no absolute
    /// path to the binary). Other launchers: their argv, binary as a bare name.
    pub argv: Vec<String>,
    /// Working directory of the root launcher.
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessService {
    /// "proc:<rootPid>:<cwd>"; stable across watcher restarts.
    pub id: String,
    pub kind: ProcessKind,
    /// Name of the binary holding the socket.
    pub runtime: String,
    pub ports: Vec<Port>,
    /// Process holding the socket.
    pub pid: u32,
    /// Root launcher.
    pub root_pid: u32,
    /// Working directory of the process holding the socket.
    pub cwd: Option<String>,
    pub project: String,
    /// Readable launcher command.
    pub script: String,
    /// Full launcher command line.
    pub command: String,
    pub uptime_sec: Option<u64>,
    /// Sum over the launcher and all its descendants.
    pub cpu_percent: f32,
    pub mem_bytes: u64,
    /// Launcher first (depth 0), then its subtree in depth-first order.
    pub tree: Vec<TreeNode>,
    pub restart: Option<RestartSpec>,
    pub worktree: Option<Worktree>,
    pub managed: bool,
    /// Pid registered by `restart_process`, when this row descends from it.
    #[serde(skip)]
    pub managed_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeInfo {
    pub project: String,
    pub service: String,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerService {
    /// "docker:<containerId>"
    pub id: String,
    pub container_id: String,
    pub name: String,
    pub image: String,
    /// running | exited | paused | restarting | created | dead
    pub state: String,
    pub status: String,
    /// Approximate, parsed from `status`; None when not running.
    pub uptime_sec: Option<u64>,
    pub ports: Vec<Port>,
    pub cpu_percent: Option<f64>,
    pub mem_bytes: Option<u64>,
    pub compose: Option<ComposeInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeGroup {
    pub project: String,
    pub working_dir: Option<String>,
    pub containers: Vec<ContainerService>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Milliseconds since the Unix epoch; 0 before the first pass.
    pub taken_at: u64,
    pub docker_available: bool,
    pub processes: Vec<ProcessService>,
    pub compose: Vec<ComposeGroup>,
    pub standalone: Vec<ContainerService>,
}
