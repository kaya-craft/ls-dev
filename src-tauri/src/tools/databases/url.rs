//! Reading of Postgres / TablePlus connection URLs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedUrl {
    pub name: Option<String>,
    pub env: Option<String>,
    pub color: Option<String>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: String,
    pub ssl_mode: SslMode,
}

const VERIFY_UNSUPPORTED: &str = "Certificate verification (sslmode=verify-ca/verify-full) is not supported yet. Use sslmode=require to connect without verifying the certificate.";

pub fn parse_url(input: &str) -> Result<ParsedUrl, String> {
    let rest = input
        .strip_prefix("postgresql://")
        .or_else(|| input.strip_prefix("postgres://"))
        .ok_or("The URL must start with postgres:// or postgresql://.")?;

    let (authority, tail) = match rest.find(['/', '?']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, q),
        None => (tail, ""),
    };

    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(info) => match info.split_once(':') {
            Some((u, p)) => (decode(u), Some(decode(p))),
            None => (decode(info), None),
        },
        None => (String::new(), None),
    };

    let (host, port_text) = if let Some(after) = hostport.strip_prefix('[') {
        let (h, r) = after.split_once(']').unwrap_or((after, ""));
        (h, r.strip_prefix(':'))
    } else {
        match hostport.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (hostport, None),
        }
    };
    let port = match port_text {
        None => 5432,
        Some(p) => p
            .parse::<u16>()
            .ok()
            .filter(|n| *n != 0)
            .ok_or("The port is not a number.")?,
    };

    let database = decode(path.strip_prefix('/').unwrap_or(path));

    let mut parsed = ParsedUrl {
        name: None,
        env: None,
        color: None,
        host: host.to_string(),
        port,
        user,
        password,
        database,
        ssl_mode: SslMode::Prefer,
    };
    let mut sslmode: Option<SslMode> = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let (key, value) = (decode(key), decode(value));
        match key.as_str() {
            "sslmode" => {
                sslmode = match value.as_str() {
                    "disable" => Some(SslMode::Disable),
                    "prefer" | "allow" => Some(SslMode::Prefer),
                    "require" => Some(SslMode::Require),
                    "verify-ca" | "verify-full" => return Err(VERIFY_UNSUPPORTED.to_string()),
                    _ => sslmode,
                }
            }
            "name" => parsed.name = Some(value),
            "env" => parsed.env = Some(value),
            "statusColor" => {
                if value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
                    parsed.color = Some(format!("#{}", value.to_ascii_lowercase()));
                }
            }
            _ => {}
        }
    }
    // TablePlus's `tLSMode` is ignored: its values are undocumented, and
    // `tLSMode=0` still encrypts in TablePlus.
    parsed.ssl_mode = sslmode.unwrap_or(SslMode::Prefer);
    Ok(parsed)
}

/// Percent-decodes `s`; `+` stays a literal plus. Invalid escapes are kept as is.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tableplus_url_gives_every_field() {
        let p = parse_url("postgresql://app:s3cr%40t@127.0.0.1:5433/app?statusColor=A3793E&env=staging&name=Acme%20Staging&tLSMode=0&usePrivateKey=false&safeModeLevel=0").unwrap();
        assert_eq!(p.name.as_deref(), Some("Acme Staging"));
        assert_eq!(p.env.as_deref(), Some("staging"));
        assert_eq!(p.color.as_deref(), Some("#a3793e"));
        assert_eq!((p.host.as_str(), p.port), ("127.0.0.1", 5433));
        assert_eq!(p.user, "app");
        assert_eq!(p.password.as_deref(), Some("s3cr@t"));
        assert_eq!(p.database, "app");
        // TablePlus connects with encryption under tLSMode=0: the value is not
        // "disabled", so it is ignored.
        assert_eq!(p.ssl_mode, SslMode::Prefer);
    }

    #[test]
    fn minimal_url_uses_defaults() {
        let p = parse_url("postgres://u@h/db").unwrap();
        assert_eq!(p.port, 5432);
        assert_eq!(p.password, None);
        assert_eq!(p.ssl_mode, SslMode::Prefer);
        assert_eq!(
            (p.user.as_str(), p.host.as_str(), p.database.as_str()),
            ("u", "h", "db")
        );
    }

    #[test]
    fn sslmode_values() {
        let m = |q: &str| {
            parse_url(&format!("postgres://u@h/db?{q}"))
                .unwrap()
                .ssl_mode
        };
        assert_eq!(m("sslmode=disable"), SslMode::Disable);
        assert_eq!(m("sslmode=allow"), SslMode::Prefer);
        assert_eq!(m("sslmode=bogus"), SslMode::Prefer);
        assert_eq!(m("sslmode=require&tLSMode=0"), SslMode::Require);
        assert_eq!(m("tLSMode=0&sslmode=prefer"), SslMode::Prefer);
        assert_eq!(m("tLSMode=0"), SslMode::Prefer);
        assert_eq!(m("tLSMode=1"), SslMode::Prefer);
    }

    #[test]
    fn verify_modes_are_rejected() {
        let expected = "Certificate verification (sslmode=verify-ca/verify-full) is not supported yet. Use sslmode=require to connect without verifying the certificate.";
        for mode in ["verify-ca", "verify-full"] {
            assert_eq!(
                parse_url(&format!("postgres://u@h/db?sslmode={mode}")),
                Err(expected.to_string())
            );
        }
    }

    #[test]
    fn ipv6_host() {
        let p = parse_url("postgres://[::1]:6000/x").unwrap();
        assert_eq!((p.host.as_str(), p.port), ("::1", 6000));
        assert_eq!(p.user, "");
    }

    #[test]
    fn no_database_is_empty() {
        assert_eq!(parse_url("postgres://u@h").unwrap().database, "");
        assert_eq!(
            parse_url("postgres://u@h/?sslmode=disable")
                .unwrap()
                .database,
            ""
        );
    }

    #[test]
    fn invalid_color_is_ignored() {
        assert_eq!(
            parse_url("postgres://u@h/d?statusColor=zzz").unwrap().color,
            None
        );
        assert_eq!(
            parse_url("postgres://u@h/d?statusColor=12345")
                .unwrap()
                .color,
            None
        );
    }

    #[test]
    fn plus_is_literal() {
        let p = parse_url("postgres://u:a+b@h/d?name=a+b").unwrap();
        assert_eq!(p.password.as_deref(), Some("a+b"));
        assert_eq!(p.name.as_deref(), Some("a+b"));
    }

    #[test]
    fn wrong_scheme_is_rejected() {
        assert_eq!(
            parse_url("mysql://u:pw@h/db").unwrap_err(),
            "The URL must start with postgres:// or postgresql://."
        );
    }

    #[test]
    fn bad_port_is_rejected_without_leaking_input() {
        for url in ["postgres://u:pw@h:abc/db", "postgres://u:pw@h:0/db"] {
            let e = parse_url(url).unwrap_err();
            assert_eq!(e, "The port is not a number.");
            assert!(!e.contains("pw"));
        }
    }
}
