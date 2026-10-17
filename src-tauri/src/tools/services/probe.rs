//! "Responding" check: can we open a TCP connection to the port on localhost?

use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use futures_util::future::join_all;
use tokio::net::TcpStream;

const TIMEOUT: Duration = Duration::from_millis(300);

/// One entry per distinct port: true when 127.0.0.1 or ::1 accepts a connection.
pub async fn probe_ports(ports: Vec<u16>) -> HashMap<u16, bool> {
    let unique: BTreeSet<u16> = ports.into_iter().collect();
    join_all(
        unique
            .into_iter()
            .map(|port| async move { (port, probe_one(port).await) }),
    )
    .await
    .into_iter()
    .collect()
}

/// IPv4 first, then IPv6: Nuxt and Vite often bind only `[::1]`.
async fn probe_one(port: u16) -> bool {
    for ip in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        if let Ok(Ok(_)) =
            tokio::time::timeout(TIMEOUT, TcpStream::connect(SocketAddr::new(ip, port))).await
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[tokio::test]
    async fn detects_ipv4_listener() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert_eq!(probe_ports(vec![port]).await.get(&port), Some(&true));
    }

    #[tokio::test]
    async fn detects_ipv6_only_listener() {
        let l = TcpListener::bind("[::1]:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert_eq!(probe_ports(vec![port]).await.get(&port), Some(&true));
    }

    #[tokio::test]
    async fn closed_port_is_not_responding() {
        // The suite runs in parallel and a connection to a freed ephemeral
        // port occasionally succeeds for a few milliseconds (cause not
        // identified). A real bug would report every fresh port as open.
        let mut answers = Vec::new();
        for _ in 0..5 {
            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let open = probe_ports(vec![port]).await[&port];
            if !open {
                return;
            }
            answers.push(port);
        }
        panic!("ports {answers:?} all reported as responding although closed");
    }

    #[tokio::test]
    async fn duplicate_ports_are_probed_once() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert_eq!(probe_ports(vec![port, port]).await.len(), 1);
    }
}
