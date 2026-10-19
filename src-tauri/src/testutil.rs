//! Fixtures captured on a real machine by ls-dev, read as a ProcTable.

use crate::core::label::basename;
use crate::core::procs::ProcInfo;
use crate::tools::services::model::{
    ComposeInfo, ContainerService, Listener, Port, ProcTable, ProcessKind, ProcessService,
};
use crate::tools::services::sockets::parse_lsof_listeners;

pub const HOME: &str = "/Users/me";
/// Fixed "now" so that start_time = NOW - etime.
pub const NOW: u64 = 1_800_000_000;

const PS: &str = include_str!("../fixtures/ps.txt");
const LSOF_LISTEN: &str = include_str!("../fixtures/lsof-listen.txt");
const LSOF_CWD: &str = include_str!("../fixtures/lsof-cwd.txt");

/// lsof-cwd.txt only covers listeners; launchers get their cwd here.
const LAUNCHER_CWDS: [(u32, &str); 2] = [
    (29414, "/Users/me/Developer/@acme/shop"),
    (79026, "/Users/me/Developer/@acme/blog"),
];

/// ps `etime`: `[[dd-]hh:]mm:ss`.
pub fn parse_etime(value: &str) -> Option<u64> {
    let (days, rest) = match value.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, value),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let (h, m, s) = match parts.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    Some(days * 86_400 + h * 3_600 + m * 60 + s)
}

pub fn proc_table() -> ProcTable {
    let mut table = ProcTable::new();
    for line in PS.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(etime)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let argv: Vec<String> = fields.map(String::from).collect();
        let pid: u32 = pid.parse().unwrap();
        table.insert(
            pid,
            ProcInfo {
                pid,
                ppid: ppid.parse().unwrap(),
                name: argv
                    .first()
                    .map(|a| basename(a).to_string())
                    .unwrap_or_default(),
                argv,
                cwd: None,
                start_time: parse_etime(etime).map(|e| NOW - e),
                cpu_percent: 0.0,
                mem_bytes: 0,
            },
        );
    }

    let mut pid = None;
    for line in LSOF_CWD.lines() {
        if let Some(v) = line.strip_prefix('p') {
            pid = v.parse::<u32>().ok();
        } else if let (Some(v), Some(p)) = (line.strip_prefix('n'), pid) {
            if let Some(entry) = table.get_mut(&p) {
                entry.cwd = Some(v.to_string());
            }
        }
    }
    for (p, cwd) in LAUNCHER_CWDS {
        table.get_mut(&p).unwrap().cwd = Some(cwd.to_string());
    }
    table
}

pub fn listeners() -> Vec<Listener> {
    parse_lsof_listeners(LSOF_LISTEN)
}

pub fn process(id: &str, project: &str, port: u16) -> ProcessService {
    ProcessService {
        id: id.into(),
        kind: ProcessKind::Dev,
        runtime: "node".into(),
        ports: vec![Port {
            port,
            responsive: Some(true),
        }],
        pid: 1000,
        root_pid: 999,
        cwd: Some(format!("/work/{project}")),
        project: project.into(),
        script: "pnpm dev".into(),
        command: "node /x/pnpm dev".into(),
        uptime_sec: Some(60),
        cpu_percent: 0.0,
        mem_bytes: 0,
        tree: vec![],
        restart: None,
        worktree: None,
        managed: false,
        managed_pid: None,
    }
}

pub fn container(id: &str, name: &str, state: &str, project: Option<&str>) -> ContainerService {
    ContainerService {
        id: format!("docker:{id}"),
        container_id: id.into(),
        name: name.into(),
        image: "img".into(),
        state: state.into(),
        status: String::new(),
        uptime_sec: None,
        ports: vec![],
        cpu_percent: None,
        mem_bytes: None,
        compose: project.map(|p| ComposeInfo {
            project: p.into(),
            service: name.into(),
            working_dir: None,
        }),
    }
}
