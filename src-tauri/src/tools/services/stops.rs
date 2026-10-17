//! Which services stopped between two snapshots, as notification texts. Pure.

use std::collections::HashSet;

use crate::tools::services::model::{ContainerService, ProcessKind, Snapshot};

const COLLAPSE_ABOVE: usize = 3;

fn containers(s: &Snapshot) -> impl Iterator<Item = &ContainerService> {
    s.compose
        .iter()
        .flat_map(|g| g.containers.iter())
        .chain(s.standalone.iter())
}

/// A service seen running in one snapshot and gone in the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub id: String,
    pub message: String,
    pub container: bool,
}

pub fn stops(prev: &Snapshot, next: &Snapshot, suppressed: &HashSet<String>) -> Vec<Stop> {
    if prev.taken_at == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();

    let next_ids: HashSet<&str> = next.processes.iter().map(|p| p.id.as_str()).collect();
    for p in prev.processes.iter().filter(|p| p.kind == ProcessKind::Dev) {
        if next_ids.contains(p.id.as_str()) || suppressed.contains(&p.id) {
            continue;
        }
        out.push(Stop {
            id: p.id.clone(),
            message: match p.ports.first() {
                Some(port) => format!("{} on :{} stopped", p.project, port.port),
                None => format!("{} stopped", p.project),
            },
            container: false,
        });
    }

    if prev.docker_available && next.docker_available {
        let running = running_containers(next);
        for c in containers(prev).filter(|c| c.state == "running") {
            if !running.contains(c.id.as_str()) && !suppressed.contains(&c.id) {
                out.push(Stop {
                    id: c.id.clone(),
                    message: format!("{} stopped", c.name),
                    container: true,
                });
            }
        }
    }
    out
}

fn running_containers(s: &Snapshot) -> HashSet<&str> {
    containers(s)
        .filter(|c| c.state == "running")
        .map(|c| c.id.as_str())
        .collect()
}

/// Reports a stop only when the service is still missing one pass later: a
/// watcher (`tsx watch`) restarting its child releases the socket for a
/// moment, and a single pass in that window must not notify.
#[derive(Debug, Default)]
pub struct StopTracker {
    /// Missing in the last pass; message built from the last snapshot that had them.
    pending: Vec<Stop>,
}

impl StopTracker {
    pub fn step(
        &mut self,
        prev: &Snapshot,
        next: &Snapshot,
        suppressed: &HashSet<String>,
    ) -> Vec<String> {
        let process_ids: HashSet<&str> = next.processes.iter().map(|p| p.id.as_str()).collect();
        let running = running_containers(next);
        let confirmed = std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|stop| !suppressed.contains(&stop.id))
            .filter(|stop| {
                if stop.container {
                    // Docker unreachable: unknown, and never a reason to notify.
                    next.docker_available && !running.contains(stop.id.as_str())
                } else {
                    !process_ids.contains(stop.id.as_str())
                }
            })
            .map(|stop| stop.message)
            .collect();
        self.pending = stops(prev, next, suppressed);
        confirmed
    }
}

pub fn collapse(messages: Vec<String>) -> Vec<String> {
    if messages.len() > COLLAPSE_ABOVE {
        vec![format!("{} services stopped", messages.len())]
    } else {
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{container, process};
    use crate::tools::services::model::ComposeGroup;

    fn stopped_messages(
        prev: &Snapshot,
        next: &Snapshot,
        suppressed: &HashSet<String>,
    ) -> Vec<String> {
        stops(prev, next, suppressed)
            .into_iter()
            .map(|s| s.message)
            .collect()
    }

    fn snap(
        processes: Vec<crate::tools::services::model::ProcessService>,
        containers: Vec<ContainerService>,
        docker: bool,
    ) -> Snapshot {
        Snapshot {
            taken_at: 1,
            docker_available: docker,
            processes,
            compose: vec![ComposeGroup {
                project: "p".into(),
                working_dir: None,
                containers,
            }],
            standalone: vec![],
        }
    }

    #[test]
    fn a_vanished_dev_server_is_reported_with_its_port() {
        let prev = snap(vec![process("a", "shop/packages/app", 3000)], vec![], true);
        let next = snap(vec![], vec![], true);
        assert_eq!(
            stopped_messages(&prev, &next, &HashSet::new()),
            vec!["shop/packages/app on :3000 stopped"]
        );
    }

    #[test]
    fn suppressed_ids_and_other_listeners_are_silent() {
        let mut other = process("b", "Python", 8000);
        other.kind = ProcessKind::Other;
        let prev = snap(vec![process("a", "app", 3000), other], vec![], true);
        let next = snap(vec![], vec![], true);
        assert!(stopped_messages(&prev, &next, &HashSet::from(["a".to_string()])).is_empty());
    }

    #[test]
    fn a_container_leaving_running_is_reported() {
        let prev = snap(
            vec![],
            vec![container("1", "p-db-1", "running", Some("p"))],
            true,
        );
        let next = snap(
            vec![],
            vec![container("1", "p-db-1", "exited", Some("p"))],
            true,
        );
        assert_eq!(
            stopped_messages(&prev, &next, &HashSet::new()),
            vec!["p-db-1 stopped"]
        );
    }

    #[test]
    fn docker_going_away_notifies_nothing() {
        let prev = snap(
            vec![],
            vec![container("1", "p-db-1", "running", Some("p"))],
            true,
        );
        let next = snap(vec![], vec![], false);
        assert!(stopped_messages(&prev, &next, &HashSet::new()).is_empty());
    }

    #[test]
    fn the_first_snapshot_notifies_nothing() {
        let next = snap(vec![process("a", "app", 3000)], vec![], true);
        assert!(stopped_messages(&Snapshot::default(), &next, &HashSet::new()).is_empty());
    }

    fn tracker_steps(snapshots: &[Snapshot], suppressed: &HashSet<String>) -> Vec<Vec<String>> {
        let mut tracker = StopTracker::default();
        snapshots
            .windows(2)
            .map(|w| tracker.step(&w[0], &w[1], suppressed))
            .collect()
    }

    #[test]
    fn a_dev_server_missing_for_one_pass_is_not_reported() {
        let up = snap(vec![process("a", "app", 3000)], vec![], true);
        let down = snap(vec![], vec![], true);
        let steps = tracker_steps(&[up.clone(), down, up], &HashSet::new());
        assert_eq!(steps, vec![Vec::<String>::new(), vec![]]);
    }

    #[test]
    fn a_dev_server_missing_for_two_passes_is_reported_once() {
        let up = snap(vec![process("a", "app", 3000)], vec![], true);
        let down = snap(vec![], vec![], true);
        let steps = tracker_steps(&[up, down.clone(), down.clone(), down], &HashSet::new());
        assert_eq!(
            steps,
            vec![vec![], vec!["app on :3000 stopped".to_string()], vec![]]
        );
    }

    #[test]
    fn a_container_stopped_for_two_passes_is_reported_once() {
        let up = snap(
            vec![],
            vec![container("1", "p-db-1", "running", Some("p"))],
            true,
        );
        let down = snap(
            vec![],
            vec![container("1", "p-db-1", "exited", Some("p"))],
            true,
        );
        let steps = tracker_steps(&[up, down.clone(), down], &HashSet::new());
        assert_eq!(steps, vec![vec![], vec!["p-db-1 stopped".to_string()]]);
    }

    #[test]
    fn docker_going_away_during_confirmation_notifies_nothing() {
        let up = snap(
            vec![],
            vec![container("1", "p-db-1", "running", Some("p"))],
            true,
        );
        let down = snap(
            vec![],
            vec![container("1", "p-db-1", "exited", Some("p"))],
            true,
        );
        let gone = snap(vec![], vec![], false);
        let steps = tracker_steps(&[up, down, gone], &HashSet::new());
        assert_eq!(steps, vec![Vec::<String>::new(), vec![]]);
    }

    #[test]
    fn the_tracker_respects_suppressions() {
        let up = snap(vec![process("a", "app", 3000)], vec![], true);
        let down = snap(vec![], vec![], true);
        let suppressed = HashSet::from(["a".to_string()]);
        let steps = tracker_steps(&[up.clone(), down.clone(), down.clone()], &suppressed);
        assert_eq!(steps, vec![Vec::<String>::new(), vec![]]);

        // Suppressed only when the confirmation happens.
        let mut tracker = StopTracker::default();
        assert!(tracker.step(&up, &down, &HashSet::new()).is_empty());
        assert!(tracker.step(&down, &down, &suppressed).is_empty());
    }

    #[test]
    fn many_stops_collapse_into_one() {
        let four: Vec<String> = (0..4).map(|i| format!("s{i} stopped")).collect();
        assert_eq!(collapse(four), vec!["4 services stopped"]);
        let three: Vec<String> = (0..3).map(|i| format!("s{i} stopped")).collect();
        assert_eq!(collapse(three.clone()), three);
    }
}
