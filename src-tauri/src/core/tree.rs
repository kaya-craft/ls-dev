//! Process tree walks. Pure.

use std::collections::{HashMap, HashSet};

use super::label::{basename, strip_js_ext};
use super::procs::{ProcInfo, ProcTable};

const PACKAGE_MANAGERS: [&str; 5] = ["pnpm", "npm", "yarn", "bun", "npx"];

/// `[self, parent, grandparent, ...]`, stopping at pid 1, an unknown pid or a cycle.
pub fn ancestors(pid: u32, procs: &ProcTable) -> Vec<&ProcInfo> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut current = procs.get(&pid);
    while let Some(p) = current {
        if !seen.insert(p.pid) {
            break;
        }
        chain.push(p);
        if p.ppid <= 1 {
            break;
        }
        current = procs.get(&p.ppid);
    }
    chain
}

/// Descendants of `pid` (excluded) in depth-first pre-order, children at depth 1.
pub fn subtree(pid: u32, procs: &ProcTable) -> Vec<(&ProcInfo, u32)> {
    let mut children: HashMap<u32, Vec<&ProcInfo>> = HashMap::new();
    for p in procs.values() {
        children.entry(p.ppid).or_default().push(p);
    }
    for list in children.values_mut() {
        list.sort_by_key(|p| p.pid);
    }

    fn walk<'a>(
        id: u32,
        depth: u32,
        children: &HashMap<u32, Vec<&'a ProcInfo>>,
        seen: &mut HashSet<u32>,
        out: &mut Vec<(&'a ProcInfo, u32)>,
    ) {
        for child in children.get(&id).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !seen.insert(child.pid) {
                continue;
            }
            out.push((*child, depth));
            walk(child.pid, depth + 1, children, seen, out);
        }
    }

    let mut out = Vec::new();
    let mut seen = HashSet::from([pid]);
    walk(pid, 1, &children, &mut seen, &mut out);
    out
}

/// Descendants deepest first: the safe order to signal them.
pub fn descendants_kill_order(pid: u32, procs: &ProcTable) -> Vec<u32> {
    let mut nodes = subtree(pid, procs);
    nodes.sort_by(|a, b| b.1.cmp(&a.1));
    nodes.into_iter().map(|(p, _)| p.pid).collect()
}

/// True when the first or second argv token is a package manager binary.
pub fn is_package_manager(argv: &[String]) -> bool {
    argv.iter()
        .take(2)
        .any(|t| PACKAGE_MANAGERS.contains(&strip_js_ext(basename(t))))
}

/// The outermost package-manager ancestor: the `pnpm run dev` that started
/// everything. Watchers restart the leaf, so the launcher's uptime is the
/// meaningful one. Falls back to the process itself.
pub fn root_launcher(pid: u32, procs: &ProcTable) -> Option<&ProcInfo> {
    let chain = ancestors(pid, procs);
    chain
        .iter()
        .rev()
        .find(|p| is_package_manager(&p.argv))
        .copied()
        .or_else(|| chain.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::proc_table;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn ancestors_walk_up_to_the_root() {
        let t = proc_table();
        let pids: Vec<u32> = ancestors(46048, &t).iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![46048, 29482, 29431, 29414]);
    }

    #[test]
    fn ancestors_stop_on_a_cycle() {
        let mut t = ProcTable::new();
        t.insert(
            10,
            ProcInfo {
                pid: 10,
                ppid: 11,
                ..Default::default()
            },
        );
        t.insert(
            11,
            ProcInfo {
                pid: 11,
                ppid: 10,
                ..Default::default()
            },
        );
        assert_eq!(ancestors(10, &t).len(), 2);
    }

    #[test]
    fn descendants_kill_order_is_deepest_first() {
        assert_eq!(
            descendants_kill_order(29414, &proc_table()),
            vec![46048, 29482, 29431]
        );
    }

    #[test]
    fn descendants_kill_order_on_branching_tree() {
        let mut t = ProcTable::new();
        t.insert(
            100,
            ProcInfo {
                pid: 100,
                ppid: 0,
                ..Default::default()
            },
        );
        t.insert(
            101,
            ProcInfo {
                pid: 101,
                ppid: 100,
                ..Default::default()
            },
        );
        t.insert(
            102,
            ProcInfo {
                pid: 102,
                ppid: 101,
                ..Default::default()
            },
        );
        t.insert(
            103,
            ProcInfo {
                pid: 103,
                ppid: 102,
                ..Default::default()
            },
        );
        t.insert(
            104,
            ProcInfo {
                pid: 104,
                ppid: 100,
                ..Default::default()
            },
        );
        assert_eq!(descendants_kill_order(100, &t), vec![103, 102, 101, 104]);
    }

    #[test]
    fn subtree_is_depth_first_with_depths() {
        let t = proc_table();
        let nodes: Vec<(u32, u32)> = subtree(79026, &t)
            .iter()
            .map(|(p, d)| (p.pid, *d))
            .collect();
        assert_eq!(nodes, vec![(79040, 1), (79064, 2), (93994, 3)]);
    }

    #[test]
    fn is_package_manager_recognises_wrappers_and_cjs_entry_points() {
        assert!(is_package_manager(&argv(
            "node /Users/k/.local/state/fnm/bin/pnpm dev"
        )));
        assert!(is_package_manager(&argv(
            "node /Users/k/store/node_modules/pnpm/bin/pnpm.cjs dev"
        )));
        assert!(is_package_manager(&argv("/usr/bin/yarn start")));
        assert!(!is_package_manager(&argv(
            "node /Users/k/app/node_modules/vite/bin/vite.js"
        )));
        assert!(!is_package_manager(&argv("/sbin/launchd")));
    }

    #[test]
    fn root_launcher_is_the_outermost_package_manager() {
        let t = proc_table();
        assert_eq!(root_launcher(46048, &t).unwrap().pid, 29414);
        assert_eq!(root_launcher(93994, &t).unwrap().pid, 79026);
    }

    #[test]
    fn root_launcher_falls_back_to_the_process_itself() {
        assert_eq!(root_launcher(3850, &proc_table()).unwrap().pid, 3850);
    }
}
