//! Process table from sysinfo.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use std::collections::HashMap;

/// One process as read from the system. Pure input of the collector.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    /// Executable name, e.g. "node".
    pub name: String,
    /// Full argv; empty when the system refused to reveal it.
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    /// Start time in seconds since the Unix epoch.
    pub start_time: Option<u64>,
    pub cpu_percent: f32,
    pub mem_bytes: u64,
}

pub type ProcTable = HashMap<u32, ProcInfo>;

pub struct ProcReader {
    sys: System,
}

impl ProcReader {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    /// Refresh and return every process. CPU usage needs two refreshes on
    /// the same reader: the first read reports 0 %.
    pub fn read(&mut self) -> ProcTable {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_cmd(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        self.sys
            .processes()
            .values()
            .map(|p| {
                let pid = p.pid().as_u32();
                (
                    pid,
                    ProcInfo {
                        pid,
                        ppid: p.parent().map(|x| x.as_u32()).unwrap_or(0),
                        name: p.name().to_string_lossy().into_owned(),
                        argv: p
                            .cmd()
                            .iter()
                            .map(|a| a.to_string_lossy().into_owned())
                            .collect(),
                        cwd: p.cwd().map(|c| c.to_string_lossy().into_owned()),
                        start_time: Some(p.start_time()).filter(|t| *t > 0),
                        cpu_percent: p.cpu_usage(),
                        mem_bytes: p.memory(),
                    },
                )
            })
            .collect()
    }
}

impl Default for ProcReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_current_process_with_argv_and_cwd() {
        let table = ProcReader::new().read();
        let me = table.get(&std::process::id()).expect("own pid");
        assert!(!me.argv.is_empty());
        assert_eq!(
            me.cwd.as_deref(),
            Some(std::env::current_dir().unwrap().to_str().unwrap())
        );
        assert!(me.start_time.is_some());
    }
}
