//! Join listeners and the process table into the rows we display. Pure.

use std::collections::{BTreeMap, HashSet};

use super::model::{Listener, Port, ProcTable, ProcessKind, ProcessService, RestartSpec, TreeNode};
use crate::core::label::{basename, format_script, project_label, readable_argv};
use crate::core::tree::{ancestors, is_package_manager, root_launcher, subtree};

const DEV_RUNTIMES: [&str; 3] = ["node", "bun", "deno"];
/// Docker Desktop holds every published container port; containers own them.
const DOCKER_PREFIX: &str = "com.docker";

/// A row is managed when it descends from a process LsDev relaunched.
pub fn mark_managed(rows: &mut [ProcessService], procs: &ProcTable, managed: &HashSet<u32>) {
    for row in rows {
        row.managed_pid = ancestors(row.pid, procs)
            .iter()
            .map(|p| p.pid)
            .find(|pid| managed.contains(pid));
        row.managed = row.managed_pid.is_some();
    }
}

/// Drops the rows started by the session that runs LsDev itself: under
/// `pnpm tauri dev`, the Vite server shares its root launcher with LsDev.
/// An installed LsDev has no such ancestor, so nothing is dropped.
pub fn hide_own_session(rows: &mut Vec<ProcessService>, procs: &ProcTable, self_pid: u32) {
    let own: HashSet<u32> = ancestors(self_pid, procs).iter().map(|p| p.pid).collect();
    rows.retain(|r| !own.contains(&r.root_pid));
}

/// What a restart runs. A package-manager launcher is relaunched in its
/// readable form (its absolute path dies with the shell that started it);
/// any other launcher keeps all its arguments, only the binary is reduced to
/// its name: `node dist/server.js` must not become `server`.
pub fn restart_argv(argv: &[String]) -> Vec<String> {
    if is_package_manager(argv) {
        return readable_argv(argv);
    }
    let mut out = argv.to_vec();
    if let Some(first) = out.first_mut() {
        *first = basename(first).to_string();
    }
    out
}

pub fn assemble_processes(
    listeners: &[Listener],
    procs: &ProcTable,
    home: &str,
    now: u64,
    show_all: bool,
) -> Vec<ProcessService> {
    let mut rows: BTreeMap<String, ProcessService> = BTreeMap::new();

    for listener in listeners {
        if listener.name.starts_with(DOCKER_PREFIX) || listener.ports.is_empty() {
            continue;
        }
        let is_dev = DEV_RUNTIMES.contains(&listener.name.as_str());
        if !is_dev && !show_all {
            continue;
        }

        let own = procs.get(&listener.pid);
        let launcher = root_launcher(listener.pid, procs).or(own);
        let cwd = own.and_then(|p| p.cwd.clone());
        let root_pid = launcher.map(|l| l.pid).unwrap_or(listener.pid);
        let id = format!("proc:{root_pid}:{}", cwd.as_deref().unwrap_or(""));

        if let Some(existing) = rows.get_mut(&id) {
            for &port in &listener.ports {
                if !existing.ports.iter().any(|p| p.port == port) {
                    existing.ports.push(Port {
                        port,
                        responsive: None,
                    });
                }
            }
            existing.ports.sort_by_key(|p| p.port);
            continue;
        }

        let launcher_argv = launcher.map(|l| l.argv.clone()).unwrap_or_default();
        let restart_argv = restart_argv(&launcher_argv);
        let restart = match (
            launcher.and_then(|l| l.cwd.clone()),
            restart_argv.is_empty(),
        ) {
            (Some(launcher_cwd), false) if is_dev => Some(RestartSpec {
                argv: restart_argv,
                cwd: launcher_cwd,
            }),
            _ => None,
        };

        let mut tree = Vec::new();
        if let Some(l) = launcher {
            tree.push(TreeNode {
                pid: l.pid,
                command: l.argv.join(" "),
                depth: 0,
            });
            for (p, depth) in subtree(l.pid, procs) {
                tree.push(TreeNode {
                    pid: p.pid,
                    command: p.argv.join(" "),
                    depth,
                });
            }
        }
        let (cpu_percent, mem_bytes) = tree
            .iter()
            .filter_map(|n| procs.get(&n.pid))
            .fold((0.0_f32, 0_u64), |(c, m), p| {
                (c + p.cpu_percent, m + p.mem_bytes)
            });

        rows.insert(
            id.clone(),
            ProcessService {
                id,
                kind: if is_dev {
                    ProcessKind::Dev
                } else {
                    ProcessKind::Other
                },
                runtime: listener.name.clone(),
                ports: listener
                    .ports
                    .iter()
                    .map(|&port| Port {
                        port,
                        responsive: None,
                    })
                    .collect(),
                pid: listener.pid,
                root_pid,
                project: project_label(cwd.as_deref(), home)
                    .unwrap_or_else(|| listener.name.clone()),
                cwd,
                script: format_script(&launcher_argv),
                command: launcher_argv.join(" "),
                uptime_sec: launcher
                    .and_then(|l| l.start_time)
                    .map(|s| now.saturating_sub(s)),
                cpu_percent,
                mem_bytes,
                tree,
                restart,
                worktree: None,
                managed: false,
                managed_pid: None,
            },
        );
    }

    let mut out: Vec<ProcessService> = rows.into_values().collect();
    out.sort_by_key(|r| (r.ports.first().map(|p| p.port).unwrap_or(0), r.pid));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::procs::ProcInfo;
    use crate::testutil::{listeners, parse_etime, proc_table, HOME, NOW};

    fn rows(show_all: bool) -> Vec<ProcessService> {
        assemble_processes(&listeners(), &proc_table(), HOME, NOW, show_all)
    }

    #[test]
    fn mark_managed_finds_a_registered_ancestor() {
        let t = proc_table();
        let mut out = assemble_processes(&listeners(), &t, HOME, NOW, false);
        let managed = std::collections::HashSet::from([29414_u32]);
        mark_managed(&mut out, &t, &managed);
        let backend = out.iter().find(|r| r.root_pid == 29414).unwrap();
        assert!(backend.managed);
        assert_eq!(backend.managed_pid, Some(29414));
        assert!(out
            .iter()
            .filter(|r| r.root_pid != 29414)
            .all(|r| !r.managed));
    }

    #[test]
    fn hide_own_session_drops_rows_sharing_an_ancestor_of_lsdev() {
        let t = proc_table();
        let mut out = assemble_processes(&listeners(), &t, HOME, NOW, false);
        let before = out.len();
        let sibling = out.iter().find(|r| r.root_pid == 29414).unwrap().pid;
        hide_own_session(&mut out, &t, sibling);
        assert_eq!(out.len(), before - 1);
        assert!(out.iter().all(|r| r.root_pid != 29414));
    }

    #[test]
    fn hide_own_session_keeps_everything_for_an_unrelated_pid() {
        let t = proc_table();
        let mut out = assemble_processes(&listeners(), &t, HOME, NOW, false);
        let before = out.len();
        hide_own_session(&mut out, &t, 999_999);
        assert_eq!(out.len(), before);
    }

    fn first_ports(rows: &[ProcessService]) -> Vec<u16> {
        rows.iter().map(|r| r.ports[0].port).collect()
    }

    fn by_port(rows: &[ProcessService], port: u16) -> ProcessService {
        rows.iter()
            .find(|r| r.ports.iter().any(|p| p.port == port))
            .unwrap()
            .clone()
    }

    #[test]
    fn lists_dev_servers_sorted_by_port() {
        assert_eq!(first_ports(&rows(false)), vec![3001, 4000, 4001, 5173]);
    }

    #[test]
    fn show_all_adds_other_listeners_but_never_docker_ones() {
        let all = rows(true);
        assert_eq!(
            first_ports(&all),
            vec![3001, 4000, 4001, 5173, 5198, 7000, 55385]
        );
        let control = by_port(&all, 7000);
        assert_eq!(control.project, "ControlCenter");
        assert_eq!(control.kind, ProcessKind::Other);
    }

    #[test]
    fn reports_the_launcher_uptime_not_the_restarted_leaf() {
        let backend = by_port(&rows(false), 4000);
        assert_eq!(backend.uptime_sec, parse_etime("09-21:23:10"));
        assert_eq!(backend.root_pid, 29414);
        assert_eq!(backend.pid, 46048);
    }

    #[test]
    fn labels_a_row_with_project_and_launcher_script() {
        let backend = by_port(&rows(false), 4000);
        assert_eq!(backend.project, "shop/packages/backend");
        assert_eq!(backend.script, "pnpm --filter @acme/backend dev");
        assert_eq!(backend.kind, ProcessKind::Dev);
        assert_eq!(backend.runtime, "node");
        assert_eq!(
            backend.id,
            "proc:29414:/Users/klahbibi/Developer/@acme/shop/packages/backend"
        );
    }

    #[test]
    fn keeps_several_ports_of_one_server_on_one_row() {
        let web = by_port(&rows(false), 5173);
        let ports: Vec<u16> = web.ports.iter().map(|p| p.port).collect();
        assert_eq!(ports, vec![5173, 24678]);
    }

    #[test]
    fn restart_uses_launcher_cwd_and_readable_argv() {
        let backend = by_port(&rows(false), 4000);
        assert_eq!(
            backend.restart,
            Some(RestartSpec {
                argv: vec![
                    "pnpm".into(),
                    "--filter".into(),
                    "@acme/backend".into(),
                    "dev".into()
                ],
                cwd: "/Users/klahbibi/Developer/@acme/shop".into(),
            })
        );
        // The web launcher's cwd is unknown: no restart.
        assert_eq!(by_port(&rows(false), 5173).restart, None);
    }

    #[test]
    fn tree_starts_at_the_launcher() {
        let backend = by_port(&rows(false), 4000);
        let nodes: Vec<(u32, u32)> = backend.tree.iter().map(|n| (n.pid, n.depth)).collect();
        assert_eq!(nodes, vec![(29414, 0), (29431, 1), (29482, 2), (46048, 3)]);
    }

    fn proc(pid: u32, ppid: u32, argv: &str, cwd: &str, cpu: f32, mem: u64) -> ProcInfo {
        let argv: Vec<String> = argv.split_whitespace().map(String::from).collect();
        ProcInfo {
            pid,
            ppid,
            name: argv[0].clone(),
            argv,
            cwd: Some(cwd.into()),
            start_time: Some(NOW - 60),
            cpu_percent: cpu,
            mem_bytes: mem,
        }
    }

    #[test]
    fn merges_listeners_of_one_launcher_and_cwd_and_sums_usage() {
        let mut t = ProcTable::new();
        t.insert(100, proc(100, 1, "pnpm dev", "/a", 1.0, 10));
        t.insert(101, proc(101, 100, "node server.js", "/a/x", 2.0, 20));
        t.insert(102, proc(102, 100, "node hmr.js", "/a/x", 3.0, 30));
        t.insert(103, proc(103, 100, "node api.js", "/a/y", 4.0, 40));
        let ls = vec![
            Listener {
                pid: 101,
                name: "node".into(),
                ports: vec![3000],
            },
            Listener {
                pid: 102,
                name: "node".into(),
                ports: vec![3001],
            },
            Listener {
                pid: 103,
                name: "node".into(),
                ports: vec![4000],
            },
        ];
        let out = assemble_processes(&ls, &t, HOME, NOW, false);
        assert_eq!(out.len(), 2);
        let ports: Vec<u16> = out[0].ports.iter().map(|p| p.port).collect();
        assert_eq!(ports, vec![3000, 3001]);
        assert_eq!(out[0].cpu_percent, 10.0);
        assert_eq!(out[0].mem_bytes, 100);
    }

    fn listener(pid: u32, port: u16) -> Listener {
        Listener {
            pid,
            name: "node".into(),
            ports: vec![port],
        }
    }

    #[test]
    fn restart_of_a_bare_node_launcher_keeps_its_script_path() {
        let mut t = ProcTable::new();
        t.insert(300, proc(300, 1, "-zsh", "/b", 0.0, 0));
        t.insert(
            301,
            proc(
                301,
                300,
                "/Users/k/.local/share/fnm/node-versions/v26/bin/node dist/server.js",
                "/b",
                0.0,
                0,
            ),
        );
        let out = assemble_processes(&[listener(301, 4100)], &t, HOME, NOW, false);
        assert_eq!(out[0].script, "server");
        assert_eq!(
            out[0].restart,
            Some(RestartSpec {
                argv: vec!["node".into(), "dist/server.js".into()],
                cwd: "/b".into(),
            })
        );
    }

    #[test]
    fn restart_of_a_tsx_chain_without_package_manager_keeps_absolute_arguments() {
        let mut t = ProcTable::new();
        t.insert(400, proc(400, 1, "-zsh", "/c", 0.0, 0));
        t.insert(
            401,
            proc(
                401,
                400,
                "node /c/node_modules/tsx/dist/cli.mjs watch src/server.ts",
                "/c",
                0.0,
                0,
            ),
        );
        t.insert(
            402,
            proc(
                402,
                401,
                "/usr/local/bin/node --require /c/node_modules/tsx/dist/preflight.cjs src/server.ts",
                "/c",
                0.0,
                0,
            ),
        );
        let out = assemble_processes(&[listener(402, 4200)], &t, HOME, NOW, false);
        assert_eq!(
            out[0].script,
            "node --require /c/node_modules/tsx/dist/preflight.cjs src/server.ts"
        );
        assert_eq!(
            out[0].restart.as_ref().unwrap().argv,
            vec![
                "node",
                "--require",
                "/c/node_modules/tsx/dist/preflight.cjs",
                "src/server.ts"
            ]
        );
    }

    #[test]
    fn other_rows_are_never_restartable() {
        let mut t = ProcTable::new();
        t.insert(
            500,
            proc(500, 1, "python3 -m http.server 8000", "/d", 0.0, 0),
        );
        let ls = [Listener {
            pid: 500,
            name: "Python".into(),
            ports: vec![8000],
        }];
        let out = assemble_processes(&ls, &t, HOME, NOW, true);
        assert_eq!(out[0].kind, ProcessKind::Other);
        assert_eq!(out[0].script, "python3 -m http.server 8000");
        assert_eq!(out[0].restart, None);
    }
}
