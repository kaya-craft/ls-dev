//! Named Cloudflare tunnels: creation, DNS route, deletion.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::cloudflared::strip_ansi;
use super::names::{hostname, tunnel_name};
use super::store::StableUrl;

const TIMEOUT: Duration = Duration::from_secs(30);
const TIMEOUT_MESSAGE: &str = "cloudflared did not answer within 30 s.";
const DNS_CONFLICT_MESSAGE: &str = "This name is already used in DNS. Choose another one.";

pub struct CmdOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait Runner: Send + Sync {
    fn run(&self, args: &[String]) -> Result<CmdOutput, String>;
}

pub struct CliRunner {
    pub binary: PathBuf,
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// Runs `cmd` with stdin closed; stdout and stderr are drained while it runs.
/// The child is killed and reaped when it does not end within `timeout`.
fn run_command(mut cmd: Command, timeout: Duration) -> Result<CmdOutput, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot start cloudflared: {e}"))?;
    let out = drain(
        child
            .stdout
            .take()
            .ok_or("Cannot read cloudflared output.")?,
    );
    let err = drain(
        child
            .stderr
            .take()
            .ok_or("Cannot read cloudflared output.")?,
    );
    // Both pipes reach EOF when the child exits; one deadline covers both.
    let deadline = Instant::now() + timeout;
    let left = || deadline.saturating_duration_since(Instant::now());
    let (Ok(stdout), Ok(stderr)) = (out.recv_timeout(left()), err.recv_timeout(left())) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(TIMEOUT_MESSAGE.into());
    };
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(CmdOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

impl Runner for CliRunner {
    fn run(&self, args: &[String]) -> Result<CmdOutput, String> {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args);
        run_command(cmd, TIMEOUT)
    }
}

/// `8-4-4-4-12` hexadecimal digits.
fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.matches('-').count() == 4
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// UUID after `with id ` in `Created tunnel <name> with id <uuid>`.
pub fn parse_created_id(output: &str) -> Option<String> {
    let rest = output.split("with id ").nth(1)?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    is_uuid(&id).then_some(id)
}

/// Hostname that `route dns` reports, from `Added CNAME <fqdn> which will
/// route to this tunnel` or `<fqdn> is already configured to route to your
/// tunnel`; lowercase, without a final dot. A log prefix is allowed.
pub fn parse_routed_hostname(output: &str) -> Option<String> {
    const ADDED: &str = "added cname ";
    const CONFIGURED: &str = " is already configured to route to your tunnel";
    strip_ansi(output).lines().find_map(|line| {
        let lower = line.to_lowercase();
        let fqdn = if let Some(i) = lower.find(ADDED) {
            lower[i + ADDED.len()..].split_whitespace().next()
        } else if let Some(i) = lower.find(CONFIGURED) {
            lower[..i].split_whitespace().last()
        } else {
            None
        }?;
        let fqdn = fqdn.trim_end_matches('.');
        (!fqdn.is_empty()).then(|| fqdn.to_string())
    })
}

/// True when `<home>/.cloudflared/<id>.json` exists: the credentials of the
/// tunnel are on this Mac. Metadata only, the file is not read.
pub fn has_credentials(home: &Path, id: &str) -> bool {
    is_uuid(id)
        && std::fs::metadata(home.join(".cloudflared").join(format!("{id}.json")))
            .is_ok_and(|m| m.is_file())
}

/// Id of the tunnel called `name` in `cloudflared tunnel list -o json`.
pub fn find_id_in_list(json: &str, name: &str) -> Option<String> {
    let list: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    list.iter()
        .find(|t| t["name"].as_str() == Some(name))
        .and_then(|t| t["id"].as_str())
        .map(str::to_string)
}

/// True when `route dns` failed because a record already exists.
pub fn is_dns_conflict(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    (lower.contains("already exists") && lower.contains("record")) || lower.contains("code: 1003")
}

fn args(rest: &[&str]) -> Vec<String> {
    ["tunnel", "--no-autoupdate"]
        .iter()
        .chain(rest)
        .map(|s| s.to_string())
        .collect()
}

fn last_line(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("unknown error")
        .to_string()
}

fn delete_id(runner: &dyn Runner, id: &str) {
    let _ = runner.run(&args(&["delete", "-f", id]));
}

/// Creates the tunnel `lsdev-<subdomain>` and routes `<subdomain>.<domain>` to it.
/// A tunnel of that name already in the account is reused only when its
/// credentials are in `<home>/.cloudflared`.
pub fn create(
    runner: &dyn Runner,
    subdomain: &str,
    domain: &str,
    home: &Path,
) -> Result<StableUrl, String> {
    let name = tunnel_name(subdomain);
    let host = hostname(subdomain, domain);
    let out = runner.run(&args(&["create", &name]))?;
    let combined = format!("{}\n{}", out.stdout, out.stderr);
    let (id, created_now) = match parse_created_id(&combined) {
        Some(id) if out.success => (id, true),
        _ if combined.to_lowercase().contains("already exists") => {
            let list = runner.run(&args(&["list", "-o", "json"]))?;
            let id = list
                .success
                .then(|| find_id_in_list(&list.stdout, &name))
                .flatten()
                .ok_or_else(|| format!("cloudflared: tunnel {name} exists but was not found."))?;
            if !has_credentials(home, &id) {
                return Err(format!(
                    "A tunnel named {name} already exists in this Cloudflare account but not on this Mac. Choose another name."
                ));
            }
            (id, false)
        }
        _ if out.success => return Err("cloudflared: tunnel id not found in its output.".into()),
        _ => return Err(format!("cloudflared: {}", last_line(&out.stderr))),
    };
    let route = runner.run(&args(&["route", "dns", &id, &host]))?;
    if !route.success {
        let message = if is_dns_conflict(&format!("{}\n{}", route.stdout, route.stderr)) {
            DNS_CONFLICT_MESSAGE.to_string()
        } else {
            format!("cloudflared: {}", last_line(&route.stderr))
        };
        if created_now {
            delete_id(runner, &id);
        }
        return Err(message);
    }
    // cloudflared creates the record in the zone of cert.pem, whatever the
    // tunnel domain: a domain outside that zone gives another name.
    let routed = parse_routed_hostname(&format!("{}\n{}", route.stdout, route.stderr));
    if routed.as_deref() != Some(host.as_str()) {
        if created_now {
            delete_id(runner, &id);
        }
        return Err(match routed {
            Some(other) => format!(
                "Cloudflare created {other} instead of {host}: the tunnel domain must belong to the Cloudflare zone cloudflared is logged in to."
            ),
            None => format!("Could not confirm the DNS record for {host}."),
        });
    }
    Ok(StableUrl {
        hostname: host,
        tunnel_name: name,
        tunnel_id: id,
    })
}

pub fn delete(runner: &dyn Runner, entry: &StableUrl) -> Result<(), String> {
    let out = runner.run(&args(&["delete", "-f", &entry.tunnel_id]))?;
    if out.success {
        Ok(())
    } else {
        Err(format!("cloudflared: {}", last_line(&out.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const ID: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

    struct Fake {
        calls: Mutex<Vec<Vec<String>>>,
        script: Mutex<Vec<CmdOutput>>,
    }

    impl Fake {
        fn new(script: Vec<CmdOutput>) -> Self {
            Fake {
                calls: Mutex::new(Vec::new()),
                script: Mutex::new(script),
            }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Runner for Fake {
        fn run(&self, args: &[String]) -> Result<CmdOutput, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            let mut s = self.script.lock().unwrap();
            assert!(!s.is_empty(), "unexpected call {args:?}");
            Ok(s.remove(0))
        }
    }

    fn ok(stdout: &str, stderr: &str) -> CmdOutput {
        CmdOutput {
            success: true,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
    fn ko(stderr: &str) -> CmdOutput {
        CmdOutput {
            success: false,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }
    fn created() -> CmdOutput {
        ok("", &format!("Created tunnel lsdev-shop with id {ID}\n"))
    }
    const HOST: &str = "shop.dev.example.com";
    fn routed() -> CmdOutput {
        ok(
            "",
            &format!("2026-10-10T09:00:00Z INF Added CNAME {HOST} which will route to this tunnel tunnelID={ID}\n"),
        )
    }
    /// A home directory; with `credentials`, `.cloudflared/<ID>.json` exists.
    fn home(credentials: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if credentials {
            std::fs::create_dir_all(dir.path().join(".cloudflared")).unwrap();
            std::fs::write(dir.path().join(format!(".cloudflared/{ID}.json")), "{}").unwrap();
        }
        dir
    }
    fn listed() -> CmdOutput {
        ok(&format!(r#"[{{"id":"{ID}","name":"lsdev-shop"}}]"#), "")
    }

    #[test]
    fn create_happy_path() {
        let fake = Fake::new(vec![created(), routed()]);
        let home = home(false);
        let got = create(&fake, "shop", "dev.example.com", home.path()).unwrap();
        assert_eq!(
            got,
            StableUrl {
                hostname: "shop.dev.example.com".into(),
                tunnel_name: "lsdev-shop".into(),
                tunnel_id: ID.into()
            }
        );
        assert_eq!(
            fake.calls(),
            vec![
                v(&["tunnel", "--no-autoupdate", "create", "lsdev-shop"]),
                v(&[
                    "tunnel",
                    "--no-autoupdate",
                    "route",
                    "dns",
                    ID,
                    "shop.dev.example.com"
                ]),
            ]
        );
    }

    #[test]
    fn existing_tunnel_is_found_with_list() {
        let fake = Fake::new(vec![
            ko("failed to create tunnel: tunnel with name lsdev-shop already exists"),
            listed(),
            ok(
                &format!("{HOST} is already configured to route to your tunnel"),
                "",
            ),
        ]);
        let home = home(true);
        let got = create(&fake, "shop", "dev.example.com", home.path()).unwrap();
        assert_eq!(got.tunnel_id, ID);
        assert_eq!(
            fake.calls()[1],
            v(&["tunnel", "--no-autoupdate", "list", "-o", "json"])
        );
        assert_eq!(fake.calls().len(), 3);
    }

    #[test]
    fn dns_conflict_deletes_the_new_tunnel() {
        let fake = Fake::new(vec![
            created(),
            ko("Failed to add route: record with that host already exists."),
            ok("", ""),
        ]);
        let err = create(&fake, "shop", "dev.example.com", home(false).path()).unwrap_err();
        assert_eq!(err, "This name is already used in DNS. Choose another one.");
        assert_eq!(
            fake.calls()[2],
            v(&["tunnel", "--no-autoupdate", "delete", "-f", ID])
        );
    }

    #[test]
    fn dns_conflict_on_a_found_tunnel_does_not_delete() {
        let fake = Fake::new(vec![
            ko("tunnel with name lsdev-shop already exists"),
            listed(),
            ko("code: 1003, reason: An A, AAAA, or CNAME record already exists"),
        ]);
        let home = home(true);
        let err = create(&fake, "shop", "dev.example.com", home.path()).unwrap_err();
        assert_eq!(err, "This name is already used in DNS. Choose another one.");
        assert_eq!(fake.calls().len(), 3);
    }

    #[test]
    fn other_route_failure_deletes_and_reports_last_line() {
        let fake = Fake::new(vec![
            created(),
            ko("first line\nzone not found\n\n"),
            ok("", ""),
        ]);
        let err = create(&fake, "shop", "dev.example.com", home(false).path()).unwrap_err();
        assert_eq!(err, "cloudflared: zone not found");
        assert_eq!(
            fake.calls()[2],
            v(&["tunnel", "--no-autoupdate", "delete", "-f", ID])
        );
    }

    #[test]
    fn a_found_tunnel_without_local_credentials_is_refused() {
        let fake = Fake::new(vec![
            ko("tunnel with name lsdev-shop already exists"),
            listed(),
        ]);
        let err = create(&fake, "shop", "dev.example.com", home(false).path()).unwrap_err();
        assert_eq!(
            err,
            "A tunnel named lsdev-shop already exists in this Cloudflare account but not on this Mac. Choose another name."
        );
        assert_eq!(fake.calls().len(), 2, "no route, no delete");
    }

    #[test]
    fn a_record_in_another_zone_deletes_the_new_tunnel() {
        let other = "shop.dev.other.com.example.org";
        let fake = Fake::new(vec![
            created(),
            ok(
                "",
                &format!("2026-10-10T09:00:00Z INF Added CNAME {other} which will route to this tunnel tunnelID={ID}"),
            ),
            ok("", ""),
        ]);
        let err = create(&fake, "shop", "dev.example.com", home(false).path()).unwrap_err();
        assert_eq!(
            err,
            format!("Cloudflare created {other} instead of {HOST}: the tunnel domain must belong to the Cloudflare zone cloudflared is logged in to.")
        );
        assert_eq!(
            fake.calls()[2],
            v(&["tunnel", "--no-autoupdate", "delete", "-f", ID])
        );
    }

    #[test]
    fn a_mismatch_on_a_found_tunnel_does_not_delete_it() {
        let fake = Fake::new(vec![
            ko("tunnel with name lsdev-shop already exists"),
            listed(),
            ok(
                "shop.example.com.example.com is already configured to route to your tunnel",
                "",
            ),
        ]);
        let home = home(true);
        let err = create(&fake, "shop", "dev.example.com", home.path()).unwrap_err();
        assert!(err.starts_with("Cloudflare created shop.example.com.example.com instead of"));
        assert_eq!(fake.calls().len(), 3);
    }

    #[test]
    fn an_unconfirmed_record_deletes_the_new_tunnel() {
        let fake = Fake::new(vec![created(), ok("", "done"), ok("", "")]);
        let err = create(&fake, "shop", "dev.example.com", home(false).path()).unwrap_err();
        assert_eq!(err, format!("Could not confirm the DNS record for {HOST}."));
        assert_eq!(
            fake.calls()[2],
            v(&["tunnel", "--no-autoupdate", "delete", "-f", ID])
        );
    }

    #[test]
    fn parses_the_routed_hostname() {
        assert_eq!(
            parse_routed_hostname(&format!("2026-10-10T09:00:00Z INF Added CNAME {HOST} which will route to this tunnel tunnelID={ID}")).as_deref(),
            Some(HOST)
        );
        assert_eq!(
            parse_routed_hostname(
                "noise\nINF added cname Shop.Dev.Example.com. which will route to this tunnel"
            )
            .as_deref(),
            Some(HOST)
        );
        assert_eq!(
            parse_routed_hostname(&format!("2026-10-10T09:00:00Z INF {HOST} is already configured to route to your tunnel tunnelID={ID}")).as_deref(),
            Some(HOST)
        );
        assert_eq!(
            parse_routed_hostname(
                "\x1b[32mINF\x1b[0m Added CNAME a-b.example.com which will route"
            )
            .as_deref(),
            Some("a-b.example.com")
        );
        assert_eq!(parse_routed_hostname("Added CNAME"), None);
        assert_eq!(parse_routed_hostname(""), None);
    }

    #[test]
    fn credentials_are_checked_by_metadata_only() {
        assert!(has_credentials(home(true).path(), ID));
        assert!(!has_credentials(home(false).path(), ID));
        let dir = home(false);
        std::fs::create_dir_all(dir.path().join(format!(".cloudflared/{ID}.json"))).unwrap();
        assert!(
            !has_credentials(dir.path(), ID),
            "a directory is not a file"
        );
        assert!(!has_credentials(home(true).path(), "../../etc/passwd"));
    }

    #[test]
    fn delete_uses_the_tunnel_id() {
        let fake = Fake::new(vec![ok("", "")]);
        let entry = StableUrl {
            hostname: "h".into(),
            tunnel_name: "n".into(),
            tunnel_id: ID.into(),
        };
        delete(&fake, &entry).unwrap();
        assert_eq!(
            fake.calls()[0],
            v(&["tunnel", "--no-autoupdate", "delete", "-f", ID])
        );
        let fake = Fake::new(vec![ko("boom")]);
        assert_eq!(delete(&fake, &entry).unwrap_err(), "cloudflared: boom");
    }

    #[test]
    fn parses_created_id() {
        let out = format!("Created tunnel lsdev-khalif-shop with id {ID}");
        assert_eq!(parse_created_id(&out).as_deref(), Some(ID));
        assert_eq!(parse_created_id("Created tunnel x"), None);
    }

    #[test]
    fn finds_id_in_list() {
        let json = format!(
            r#"[{{"id":"a","name":"other"}},{{"id":"{ID}","name":"lsdev-x","created_at":"t"}}]"#
        );
        assert_eq!(find_id_in_list(&json, "lsdev-x").as_deref(), Some(ID));
        assert_eq!(find_id_in_list(&json, "missing"), None);
        assert_eq!(find_id_in_list("not json", "lsdev-x"), None);
    }

    #[test]
    fn detects_dns_conflicts() {
        assert!(is_dns_conflict("An A record with that host Already Exists"));
        assert!(is_dns_conflict("code: 1003"));
        assert!(is_dns_conflict("Code: 1003, reason: exists"));
        assert!(!is_dns_conflict("request id 10031 failed"));
        assert!(!is_dns_conflict("error 1003"));
        assert!(!is_dns_conflict("zone not found"));
        assert!(!is_dns_conflict("tunnel already exists"));
    }

    #[test]
    fn cli_runner_captures_both_streams_and_status() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 3"]);
        let out = run_command(cmd, Duration::from_secs(5)).unwrap();
        assert!(!out.success);
        assert_eq!(out.stdout.trim(), "out");
        assert_eq!(out.stderr.trim(), "err");
    }

    #[test]
    fn cli_runner_uses_one_deadline_for_both_pipes() {
        // stdout closes at 0.6 s; a background sleep keeps stderr open
        // until 1.6 s. Two sequential 1 s waits would succeed at 1.6 s.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "(sleep 1.6 >/dev/null &); sleep 0.6"]);
        let start = Instant::now();
        let err = run_command(cmd, Duration::from_secs(1)).err();
        assert_eq!(
            err.as_deref(),
            Some("cloudflared did not answer within 30 s.")
        );
        assert!(
            start.elapsed() < Duration::from_millis(1400),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn cli_runner_times_out() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("5");
        let err = run_command(cmd, Duration::from_millis(200)).err().unwrap();
        assert_eq!(err, "cloudflared did not answer within 30 s.");
    }
}
