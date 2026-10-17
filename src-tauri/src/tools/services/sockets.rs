//! Listening TCP sockets with their owning pid.
//! netstat2 first; `lsof` when netstat2 fails or hides pids.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};

use super::model::{Listener, ProcTable};

/// Port of an lsof address: `*:4000`, `127.0.0.1:5173`, `[::1]:3001`.
/// None for wildcard ports (`*:*`).
pub fn port_from_address(address: &str) -> Option<u16> {
    let (_, port) = address.rsplit_once(':')?;
    port.parse::<u16>().ok().filter(|p| *p > 0)
}

/// Parse `lsof -nP -iTCP -sTCP:LISTEN -FpcnPT`: one field per line,
/// `p` opens a process block, `c` names it, `n` carries an address.
pub fn parse_lsof_listeners(text: &str) -> Vec<Listener> {
    let mut procs: BTreeMap<u32, (String, BTreeSet<u16>)> = BTreeMap::new();
    let mut current: Option<u32> = None;

    for line in text.lines() {
        let mut chars = line.chars();
        let Some(tag) = chars.next() else { continue };
        let value = chars.as_str();
        match tag {
            'p' => {
                current = value.parse().ok();
                if let Some(pid) = current {
                    procs.entry(pid).or_default();
                }
            }
            'c' => {
                if let Some(pid) = current {
                    procs.entry(pid).or_default().0 = value.to_string();
                }
            }
            'n' => {
                if let (Some(pid), Some(port)) = (current, port_from_address(value)) {
                    procs.entry(pid).or_default().1.insert(port);
                }
            }
            _ => {}
        }
    }

    procs
        .into_iter()
        .map(|(pid, (name, ports))| Listener {
            pid,
            name,
            ports: ports.into_iter().collect(),
        })
        .collect()
}

/// Every process with a listening TCP socket.
pub fn list_listeners(procs: &ProcTable) -> Result<Vec<Listener>, String> {
    match from_netstat(procs) {
        Some(listeners) => Ok(listeners),
        None => from_lsof(),
    }
}

/// None when netstat2 fails or returns a listening socket without pid.
fn from_netstat(procs: &ProcTable) -> Option<Vec<Listener>> {
    use netstat2::{
        get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState,
    };

    let sockets = get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP,
    )
    .ok()?;

    let mut by_pid: BTreeMap<u32, BTreeSet<u16>> = BTreeMap::new();
    for socket in sockets {
        let ProtocolSocketInfo::Tcp(tcp) = socket.protocol_socket_info else {
            continue;
        };
        if !matches!(tcp.state, TcpState::Listen) || tcp.local_port == 0 {
            continue;
        }
        if socket.associated_pids.is_empty() {
            return None;
        }
        for pid in socket.associated_pids {
            by_pid.entry(pid).or_default().insert(tcp.local_port);
        }
    }

    Some(
        by_pid
            .into_iter()
            .map(|(pid, ports)| Listener {
                pid,
                name: procs.get(&pid).map(|p| p.name.clone()).unwrap_or_default(),
                ports: ports.into_iter().collect(),
            })
            .collect(),
    )
}

fn from_lsof() -> Result<Vec<Listener>, String> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-FpcnPT"])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("lsof is unavailable: {e}"))?;
    lsof_result(output.status.code(), &output.stdout, &output.stderr)
}

/// Decide what an lsof run means. Exit 1 with empty stdout is lsof's
/// "no match" (nothing listens); any other failure is an error, so a failed
/// pass is never mistaken for "every service stopped".
fn lsof_result(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<Vec<Listener>, String> {
    match code {
        Some(0) => Ok(parse_lsof_listeners(&String::from_utf8_lossy(stdout))),
        Some(1) if stdout.is_empty() => Ok(Vec::new()),
        _ => {
            let first_line = String::from_utf8_lossy(stderr)
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string);
            let code = code.map_or("a signal".to_string(), |c| format!("code {c}"));
            Err(match first_line {
                Some(line) => format!("lsof failed ({code}): {line}"),
                None => format!("lsof failed ({code})"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSOF: &str = include_str!("../../../fixtures/lsof-listen.txt");

    fn by_pid(pid: u32) -> Listener {
        parse_lsof_listeners(LSOF)
            .into_iter()
            .find(|l| l.pid == pid)
            .unwrap()
    }

    #[test]
    fn keeps_full_command_names() {
        assert_eq!(by_pid(74407).name, "com.docker.backend");
        assert_eq!(by_pid(33890).name, "Code Helper");
        assert_eq!(by_pid(46048).name, "node");
    }

    #[test]
    fn merges_ipv4_and_ipv6_sockets_of_one_port() {
        assert_eq!(by_pid(56867).ports, vec![7000]);
    }

    #[test]
    fn groups_ports_of_one_process_sorted() {
        assert_eq!(by_pid(88001).ports, vec![5173, 24678]);
    }

    #[test]
    fn port_from_address_handles_every_shape() {
        assert_eq!(port_from_address("*:4000"), Some(4000));
        assert_eq!(port_from_address("127.0.0.1:5173"), Some(5173));
        assert_eq!(port_from_address("[::1]:3001"), Some(3001));
        assert_eq!(port_from_address("[::]:7000"), Some(7000));
        assert_eq!(port_from_address("*:*"), None);
        assert_eq!(port_from_address("no-colon"), None);
    }

    #[test]
    fn lsof_success_is_parsed() {
        let got = lsof_result(Some(0), b"p10\ncnode\nn*:3000\n", b"").unwrap();
        assert_eq!(
            got,
            vec![Listener {
                pid: 10,
                name: "node".into(),
                ports: vec![3000]
            }]
        );
    }

    #[test]
    fn lsof_exit_1_with_empty_stdout_means_nothing_listens() {
        assert_eq!(lsof_result(Some(1), b"", b""), Ok(Vec::new()));
    }

    #[test]
    fn lsof_exit_1_with_output_is_an_error() {
        assert!(lsof_result(Some(1), b"p10\ncnode\nn*:3000\n", b"warning\n").is_err());
    }

    #[test]
    fn lsof_exit_2_reports_first_stderr_line() {
        let err = lsof_result(Some(2), b"", b"\nlsof: permission denied\nsecond\n").unwrap_err();
        assert!(err.contains("lsof: permission denied"), "{err}");
        assert!(!err.contains("second"), "{err}");
    }

    #[test]
    fn lsof_killed_by_signal_is_an_error() {
        assert!(lsof_result(None, b"", b"").is_err());
    }

    #[test]
    fn empty_output_gives_no_listener() {
        assert!(parse_lsof_listeners("").is_empty());
    }

    /// Spike: run with `cargo test --manifest-path src-tauri/Cargo.toml netstat_matches_lsof -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the live machine"]
    fn netstat_matches_lsof() {
        let pairs = |ls: &[Listener]| -> BTreeSet<(u32, u16)> {
            ls.iter()
                .flat_map(|l| l.ports.iter().map(move |p| (l.pid, *p)))
                .collect()
        };
        let lsof = pairs(&from_lsof().expect("lsof"));
        let netstat = from_netstat(&ProcTable::new());
        println!("lsof:    {lsof:?}");
        println!("netstat: {:?}", netstat.as_ref().map(|n| pairs(n)));
        assert_eq!(netstat.map(|n| pairs(&n)), Some(lsof));
    }
}
