//! Locating cloudflared and reading its output.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::core::git::run_with_timeout_status;

const KNOWN_PATHS: [&str; 2] = [
    "/opt/homebrew/bin/cloudflared",
    "/usr/local/bin/cloudflared",
];
const SHELL_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
const URL_SUFFIX: &str = ".trycloudflare.com";

/// The cloudflared binary: Homebrew locations first, then the login shell's PATH.
pub fn find_binary() -> Option<PathBuf> {
    KNOWN_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .or_else(|| find_in_login_shell(&crate::core::settings::shell()))
}

/// `command -v cloudflared` in a login shell, 5 s at most (also when a
/// background process keeps stdout open); absolute existing path only.
fn find_in_login_shell(shell: &str) -> Option<PathBuf> {
    let mut cmd = Command::new(shell);
    cmd.args(["-lc", "command -v cloudflared"]);
    let (status, out) = run_with_timeout_status(cmd, SHELL_LOOKUP_TIMEOUT)?;
    if !status.success() {
        return None;
    }
    out.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
}

/// True when `home/.cloudflared/cert.pem` is a regular file. The file is never read.
pub fn is_logged_in(home: &Path) -> bool {
    std::fs::metadata(home.join(".cloudflared/cert.pem")).is_ok_and(|m| m.is_file())
}

/// Removes ANSI CSI sequences (`ESC [ ... final byte`).
pub(crate) fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if ('@'..='~').contains(&n) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True when `rest` ends the hostname: end of line or a character that cannot
/// continue it (letters, digits, `-`, `.` and `/` can).
fn ends_host(rest: &str) -> bool {
    rest.chars()
        .next()
        .is_none_or(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '/')))
}

/// First `https://<label>.trycloudflare.com` of the line that ends there.
/// `<label>` = `[a-z0-9-]+` with at least one dash: quick tunnel names have
/// several words, while `api.trycloudflare.com` appears in network errors.
pub fn quick_url(line: &str) -> Option<String> {
    let clean = strip_ansi(line);
    clean.match_indices("https://").find_map(|(i, scheme)| {
        let rest = &clean[i + scheme.len()..];
        let label_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
            .count();
        let label = &rest[..label_len];
        let after = rest[label_len..].strip_prefix(URL_SUFFIX)?;
        (label.contains('-') && label != "api" && ends_host(after))
            .then(|| format!("https://{label}{URL_SUFFIX}"))
    })
}

/// A named tunnel reports each edge connection with this phrase.
pub fn is_registered(line: &str) -> bool {
    line.contains("Registered tunnel connection")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fake_shell(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("lsdev-cf-shell-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn login_shell_lookup_returns_an_existing_absolute_path() {
        let sh = fake_shell("ok", "echo motd; echo /bin/ls");
        assert_eq!(
            find_in_login_shell(sh.to_str().unwrap()),
            Some(PathBuf::from("/bin/ls"))
        );
        let sh = fake_shell("relative", "echo cloudflared");
        assert_eq!(find_in_login_shell(sh.to_str().unwrap()), None);
        let sh = fake_shell("missing", "echo /nonexistent/cloudflared");
        assert_eq!(find_in_login_shell(sh.to_str().unwrap()), None);
    }

    #[test]
    fn login_shell_lookup_is_bounded_when_a_background_process_holds_stdout() {
        let sh = fake_shell("bg", "(sleep 30 &) ; echo /bin/ls");
        let start = Instant::now();
        assert_eq!(find_in_login_shell(sh.to_str().unwrap()), None);
        assert!(start.elapsed() < Duration::from_secs(6));
    }

    #[test]
    fn quick_url_reads_the_banner_line() {
        let line = "2026-10-10T09:00:00Z INF |  https://shiny-fox-river.trycloudflare.com                                                  |";
        assert_eq!(
            quick_url(line).as_deref(),
            Some("https://shiny-fox-river.trycloudflare.com")
        );
    }

    #[test]
    fn quick_url_ignores_ansi_sequences() {
        let line = "\x1b[90m2026-10-10T09:00:00Z\x1b[0m \x1b[32mINF\x1b[0m |  \x1b[1mhttps://a1-b2.trycloudflare.com\x1b[0m  |";
        assert_eq!(
            quick_url(line).as_deref(),
            Some("https://a1-b2.trycloudflare.com")
        );
        let glued = "\x1b[32mhttps://glued-name.trycloudflare.com\x1b[0m";
        assert_eq!(
            quick_url(glued).as_deref(),
            Some("https://glued-name.trycloudflare.com")
        );
    }

    #[test]
    fn quick_url_needs_a_scheme_and_a_label() {
        assert_eq!(
            quick_url("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
        assert_eq!(quick_url("see https://trycloudflare.com/docs"), None);
        assert_eq!(quick_url("https://.trycloudflare.com"), None);
        assert_eq!(quick_url("https://Upper.trycloudflare.com"), None);
        assert_eq!(quick_url(""), None);
    }

    #[test]
    fn quick_url_skips_a_non_matching_first_candidate() {
        let line = "https://api.example.com then https://ok-1.trycloudflare.com";
        assert_eq!(
            quick_url(line).as_deref(),
            Some("https://ok-1.trycloudflare.com")
        );
    }

    #[test]
    fn quick_url_ignores_the_api_host_of_a_network_error() {
        let line = r#"2026-10-10T09:00:00Z ERR failed to request quick Tunnel: Post "https://api.trycloudflare.com/tunnel": dial tcp: lookup api.trycloudflare.com: no such host"#;
        assert_eq!(quick_url(line), None);
        assert_eq!(quick_url("https://api.trycloudflare.com"), None);
        assert_eq!(quick_url("https://single.trycloudflare.com"), None);
    }

    #[test]
    fn quick_url_needs_a_boundary_after_the_suffix() {
        assert_eq!(quick_url("https://x-y.trycloudflare.com.evil.com"), None);
        assert_eq!(quick_url("https://x-y.trycloudflare.com/path"), None);
        assert_eq!(quick_url("https://x-y.trycloudflare.comx"), None);
        assert_eq!(quick_url("https://x-y.trycloudflare.com-z"), None);
        for ok in [
            "https://x-y.trycloudflare.com",
            "https://x-y.trycloudflare.com |",
            "https://x-y.trycloudflare.com|",
            "url=\"https://x-y.trycloudflare.com\"",
        ] {
            assert_eq!(
                quick_url(ok).as_deref(),
                Some("https://x-y.trycloudflare.com"),
                "{ok}"
            );
        }
    }

    #[test]
    fn is_registered_matches_the_connection_line() {
        assert!(is_registered("2026-10-10T09:00:01Z INF Registered tunnel connection connIndex=0 connection=abc event=0 ip=1.2.3.4 location=cdg01 protocol=quic"));
        assert!(!is_registered("2026-10-10T09:00:01Z INF Starting tunnel"));
    }

    #[test]
    fn is_logged_in_checks_the_certificate_file() {
        let home = std::env::temp_dir().join(format!("lsdev-cf-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        assert!(!is_logged_in(&home));
        std::fs::create_dir_all(home.join(".cloudflared")).unwrap();
        assert!(!is_logged_in(&home));
        std::fs::write(home.join(".cloudflared/cert.pem"), "").unwrap();
        assert!(is_logged_in(&home));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn is_logged_in_rejects_a_directory_named_cert_pem() {
        let home = std::env::temp_dir().join(format!("lsdev-cf-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".cloudflared/cert.pem")).unwrap();
        assert!(!is_logged_in(&home));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
