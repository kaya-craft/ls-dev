//! Container rows and Compose grouping. Pure.

use std::collections::{BTreeMap, BTreeSet};

use super::model::{ComposeGroup, ComposeInfo, ContainerService, Port, RawContainer, RawPort};

pub const LABEL_PROJECT: &str = "com.docker.compose.project";
pub const LABEL_SERVICE: &str = "com.docker.compose.service";
pub const LABEL_WORKDIR: &str = "com.docker.compose.project.working_dir";

/// Host TCP ports published by a container, sorted and deduplicated.
pub fn published_tcp_ports(ports: &[RawPort]) -> Vec<u16> {
    ports
        .iter()
        .filter(|p| p.tcp)
        .filter_map(|p| p.public_port)
        .filter(|p| *p > 0)
        .collect::<BTreeSet<u16>>()
        .into_iter()
        .collect()
}

/// Cumulative CPU counters of one container at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuSample {
    pub total: u64,
    pub system: u64,
    pub online_cpus: u64,
}

/// Docker's formula, between two samples taken one pass apart.
pub fn cpu_percent(prev: CpuSample, cur: CpuSample) -> Option<f64> {
    if cur.total < prev.total || cur.system <= prev.system {
        return None;
    }
    let cpus = cur.online_cpus.max(1) as f64;
    Some((cur.total - prev.total) as f64 / (cur.system - prev.system) as f64 * cpus * 100.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContainerUsage {
    pub cpu_percent: Option<f64>,
    pub mem_bytes: Option<u64>,
}

/// Approximate uptime in seconds from Docker's human status ("Up 7 weeks").
/// Only running statuses ("Up ...") give a value.
pub fn uptime_from_status(status: &str) -> Option<u64> {
    let rest = status.strip_prefix("Up ")?;
    // Drop a trailing "(healthy)", "(health: starting)", "(Paused)", ...
    let rest = match rest.find(" (") {
        Some(i) => &rest[..i],
        None => rest,
    }
    .trim();
    match rest {
        "Less than a second" => return Some(0),
        "About a minute" => return Some(60),
        "About an hour" => return Some(3_600),
        _ => {}
    }
    let (n, unit) = rest.split_once(' ')?;
    let n: u64 = n.parse().ok()?;
    let unit_sec = match unit {
        "second" | "seconds" => 1,
        "minute" | "minutes" => 60,
        "hour" | "hours" => 3_600,
        "day" | "days" => 86_400,
        "week" | "weeks" => 604_800,
        "month" | "months" => 30 * 86_400,
        "year" | "years" => 365 * 86_400,
        _ => return None,
    };
    n.checked_mul(unit_sec)
}

pub fn to_service(raw: &RawContainer, usage: Option<&ContainerUsage>) -> ContainerService {
    let compose = raw.labels.get(LABEL_PROJECT).map(|project| ComposeInfo {
        project: project.clone(),
        service: raw
            .labels
            .get(LABEL_SERVICE)
            .cloned()
            .unwrap_or_else(|| raw.name.clone()),
        working_dir: raw.labels.get(LABEL_WORKDIR).cloned(),
    });
    ContainerService {
        id: format!("docker:{}", raw.id),
        container_id: raw.id.clone(),
        name: raw.name.clone(),
        image: raw.image.clone(),
        state: raw.state.clone(),
        status: raw.status.clone(),
        uptime_sec: uptime_from_status(&raw.status),
        ports: published_tcp_ports(&raw.ports)
            .into_iter()
            .map(|port| Port {
                port,
                responsive: None,
            })
            .collect(),
        cpu_percent: usage.and_then(|u| u.cpu_percent),
        mem_bytes: usage.and_then(|u| u.mem_bytes),
        compose,
    }
}

/// Compose projects sorted by name (containers by service), then standalone
/// containers sorted by name.
pub fn group(containers: Vec<ContainerService>) -> (Vec<ComposeGroup>, Vec<ContainerService>) {
    let mut groups: BTreeMap<String, ComposeGroup> = BTreeMap::new();
    let mut standalone = Vec::new();
    for c in containers {
        match c.compose.clone() {
            Some(info) => groups
                .entry(info.project.clone())
                .or_insert_with(|| ComposeGroup {
                    project: info.project.clone(),
                    working_dir: info.working_dir.clone(),
                    containers: Vec::new(),
                })
                .containers
                .push(c),
            None => standalone.push(c),
        }
    }
    let mut groups: Vec<ComposeGroup> = groups.into_values().collect();
    for g in &mut groups {
        g.containers.sort_by(|a, b| {
            let key = |c: &ContainerService| c.compose.as_ref().map(|i| i.service.clone());
            key(a).cmp(&key(b))
        });
    }
    standalone.sort_by(|a, b| a.name.cmp(&b.name));
    (groups, standalone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tcp(private_port: u16, public_port: Option<u16>) -> RawPort {
        RawPort {
            private_port,
            public_port,
            tcp: true,
        }
    }

    fn raw(id: &str, name: &str, labels: &[(&str, &str)]) -> RawContainer {
        RawContainer {
            id: id.into(),
            name: name.into(),
            image: "img".into(),
            state: "running".into(),
            status: "Up 2 hours".into(),
            ports: vec![],
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn published_ports_deduplicate_ipv4_and_ipv6_entries() {
        assert_eq!(
            published_tcp_ports(&[tcp(5432, Some(5434)), tcp(5432, Some(5434))]),
            vec![5434]
        );
    }

    #[test]
    fn published_ports_keep_each_port_of_a_range() {
        assert_eq!(
            published_tcp_ports(&[tcp(9001, Some(9001)), tcp(9000, Some(9000))]),
            vec![9000, 9001]
        );
    }

    #[test]
    fn published_ports_ignore_unpublished_and_udp() {
        let udp = RawPort {
            private_port: 5000,
            public_port: Some(5000),
            tcp: false,
        };
        assert_eq!(
            published_tcp_ports(&[tcp(3306, None), udp, tcp(80, Some(0))]),
            Vec::<u16>::new()
        );
    }

    #[test]
    fn cpu_percent_is_the_delta_ratio_times_cpus() {
        let prev = CpuSample {
            total: 1_000,
            system: 10_000,
            online_cpus: 4,
        };
        let cur = CpuSample {
            total: 2_000,
            system: 20_000,
            online_cpus: 4,
        };
        assert_eq!(cpu_percent(prev, cur), Some(40.0));
    }

    #[test]
    fn cpu_percent_is_none_without_progress_or_after_a_restart() {
        let s = CpuSample {
            total: 1_000,
            system: 10_000,
            online_cpus: 4,
        };
        assert_eq!(cpu_percent(s, s), None);
        let restarted = CpuSample {
            total: 10,
            system: 20_000,
            online_cpus: 4,
        };
        assert_eq!(cpu_percent(s, restarted), None);
    }

    #[test]
    fn to_service_reads_compose_labels() {
        let c = to_service(
            &raw(
                "abc",
                "acme-shop-postgres-1",
                &[
                    (LABEL_PROJECT, "acme-shop"),
                    (LABEL_SERVICE, "postgres"),
                    (LABEL_WORKDIR, "/Users/k/shop/infra"),
                ],
            ),
            Some(&ContainerUsage {
                cpu_percent: Some(1.5),
                mem_bytes: Some(1024),
            }),
        );
        assert_eq!(c.id, "docker:abc");
        assert_eq!(
            c.compose,
            Some(ComposeInfo {
                project: "acme-shop".into(),
                service: "postgres".into(),
                working_dir: Some("/Users/k/shop/infra".into()),
            })
        );
        assert_eq!(c.cpu_percent, Some(1.5));
        assert_eq!(c.mem_bytes, Some(1024));
    }

    #[test]
    fn group_sorts_projects_and_services_and_keeps_standalone_apart() {
        let services = vec![
            to_service(
                &raw(
                    "1",
                    "b-redis-1",
                    &[(LABEL_PROJECT, "b"), (LABEL_SERVICE, "redis")],
                ),
                None,
            ),
            to_service(&raw("2", "zzz", &[]), None),
            to_service(
                &raw(
                    "3",
                    "b-db-1",
                    &[(LABEL_PROJECT, "b"), (LABEL_SERVICE, "db")],
                ),
                None,
            ),
            to_service(
                &raw(
                    "4",
                    "a-web-1",
                    &[(LABEL_PROJECT, "a"), (LABEL_SERVICE, "web")],
                ),
                None,
            ),
            to_service(&raw("5", "aaa", &[]), None),
        ];
        let (groups, standalone) = group(services);
        let names: Vec<&str> = groups.iter().map(|g| g.project.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        let b: Vec<&str> = groups[1]
            .containers
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(b, vec!["b-db-1", "b-redis-1"]);
        let alone: Vec<&str> = standalone.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(alone, vec!["aaa", "zzz"]);
    }

    #[test]
    fn uptime_reads_every_go_units_form() {
        let cases: [(&str, u64); 12] = [
            ("Up Less than a second", 0),
            ("Up 1 second", 1),
            ("Up 42 seconds", 42),
            ("Up About a minute", 60),
            ("Up 5 minutes", 300),
            ("Up About an hour", 3_600),
            ("Up 2 hours", 7_200),
            ("Up 3 days", 259_200),
            ("Up 7 weeks", 7 * 604_800),
            ("Up 2 months", 2 * 30 * 86_400),
            ("Up 1 year", 365 * 86_400),
            ("Up 3 years", 3 * 365 * 86_400),
        ];
        for (status, expected) in cases {
            assert_eq!(uptime_from_status(status), Some(expected), "{status}");
        }
    }

    #[test]
    fn uptime_ignores_parenthesised_suffixes() {
        assert_eq!(uptime_from_status("Up 2 hours (healthy)"), Some(7_200));
        assert_eq!(uptime_from_status("Up 2 hours (unhealthy)"), Some(7_200));
        assert_eq!(
            uptime_from_status("Up 5 minutes (health: starting)"),
            Some(300)
        );
        assert_eq!(uptime_from_status("Up About an hour (Paused)"), Some(3_600));
    }

    #[test]
    fn uptime_is_none_on_overflow() {
        assert_eq!(uptime_from_status("Up 99999999999999999 years"), None);
    }

    #[test]
    fn uptime_is_none_unless_running() {
        for status in [
            "Exited (0) 3 days ago",
            "Created",
            "Restarting (1) 5 seconds ago",
            "",
            "Up",
            "Up soon",
        ] {
            assert_eq!(uptime_from_status(status), None, "{status}");
        }
    }

    #[test]
    fn to_service_derives_uptime_from_the_status() {
        let c = to_service(&raw("abc", "web", &[]), None);
        assert_eq!(c.uptime_sec, Some(7_200));
    }
}
