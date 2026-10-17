//! Docker client: socket discovery, connection, container mapping.

use bollard::models::{ContainerSummary, PortTypeEnum};
use bollard::{Docker, API_DEFAULT_VERSION};

use std::collections::HashMap;

/// One port entry of a container as returned by Docker.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawPort {
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub tcp: bool,
}

/// One container as returned by Docker. Pure input of `compose.rs`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: Vec<RawPort>,
    pub labels: HashMap<String, String>,
}

/// $DOCKER_HOST (unix only), then Docker Desktop's user socket, then the system one.
pub fn socket_candidates(docker_host: Option<&str>, home: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(path) = docker_host.and_then(|h| h.strip_prefix("unix://")) {
        out.push(path.to_string());
    }
    out.push(format!("{home}/.docker/run/docker.sock"));
    out.push("/var/run/docker.sock".to_string());
    out.dedup();
    out
}

/// First socket that answers a ping.
pub async fn connect(home: &str) -> Option<Docker> {
    let host = std::env::var("DOCKER_HOST").ok();
    for path in socket_candidates(host.as_deref(), home) {
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        let Ok(docker) = Docker::connect_with_unix(&path, 10, API_DEFAULT_VERSION) else {
            continue;
        };
        if docker.ping().await.is_ok() {
            return Some(docker);
        }
    }
    None
}

pub fn to_raw(summary: &ContainerSummary) -> RawContainer {
    RawContainer {
        id: summary.id.clone().unwrap_or_default(),
        name: summary
            .names
            .as_ref()
            .and_then(|n| n.first())
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_default(),
        image: summary.image.clone().unwrap_or_default(),
        state: summary.state.clone().unwrap_or_default(),
        status: summary.status.clone().unwrap_or_default(),
        ports: summary
            .ports
            .as_ref()
            .map(|ports| {
                ports
                    .iter()
                    .map(|p| RawPort {
                        private_port: p.private_port,
                        public_port: p.public_port,
                        tcp: matches!(p.typ, Some(PortTypeEnum::TCP)),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        labels: summary.labels.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::Port;
    use std::collections::HashMap;

    #[test]
    fn socket_candidates_put_docker_host_first() {
        assert_eq!(
            socket_candidates(Some("unix:///custom/docker.sock"), "/Users/k"),
            vec![
                "/custom/docker.sock",
                "/Users/k/.docker/run/docker.sock",
                "/var/run/docker.sock"
            ]
        );
    }

    #[test]
    fn socket_candidates_ignore_tcp_hosts() {
        assert_eq!(
            socket_candidates(Some("tcp://10.0.0.1:2375"), "/Users/k"),
            vec!["/Users/k/.docker/run/docker.sock", "/var/run/docker.sock"]
        );
        assert_eq!(socket_candidates(None, "/Users/k").len(), 2);
    }

    #[test]
    fn to_raw_strips_the_name_slash_and_maps_ports() {
        let summary = ContainerSummary {
            id: Some("abc".into()),
            names: Some(vec!["/blog-db-1".into()]),
            image: Some("postgres:17-alpine".into()),
            state: Some("running".into()),
            status: Some("Up 10 days".into()),
            ports: Some(vec![
                Port {
                    private_port: 5432,
                    public_port: Some(5434),
                    typ: Some(PortTypeEnum::TCP),
                    ..Default::default()
                },
                Port {
                    private_port: 53,
                    public_port: Some(53),
                    typ: Some(PortTypeEnum::UDP),
                    ..Default::default()
                },
            ]),
            labels: Some(HashMap::from([("k".to_string(), "v".to_string())])),
            ..Default::default()
        };
        let raw = to_raw(&summary);
        assert_eq!(raw.name, "blog-db-1");
        assert_eq!(raw.state, "running");
        assert_eq!(
            raw.ports,
            vec![
                RawPort {
                    private_port: 5432,
                    public_port: Some(5434),
                    tcp: true
                },
                RawPort {
                    private_port: 53,
                    public_port: Some(53),
                    tcp: false
                },
            ]
        );
        assert_eq!(raw.labels.get("k").map(String::as_str), Some("v"));
    }
}
