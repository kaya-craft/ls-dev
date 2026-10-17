//! Tunnel, subdomain and hostname names.

const MAX_LABEL: usize = 63;

/// Base letter of a Latin accented character, or None when not covered.
/// Covered: Latin-1 Supplement and Latin Extended-A letters with diacritics,
/// plus ß (ss), æ (ae), œ (oe), ø (o), ð (d), þ (th), ł (l), đ (d).
fn strip_accent(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ţ' | 'ť' | 'ŧ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'þ' => "th",
        _ => return None,
    })
}

/// DNS form of a text: lowercase, accents stripped, anything but `a-z0-9`
/// becomes a dash, repeated dashes collapse, edge dashes are trimmed.
/// May be empty.
pub fn dns_label(text: &str) -> String {
    let mut out = String::new();
    let mut push = |s: &str| {
        for c in s.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        }
    };
    for c in text.chars().flat_map(char::to_lowercase) {
        let mut buf = [0u8; 4];
        push(
            strip_accent(c)
                .unwrap_or_else(|| c.encode_utf8(&mut buf))
                .to_owned()
                .as_str(),
        );
    }
    out.trim_end_matches('-').to_string()
}

fn default_prefix_from(user: &str) -> String {
    let label = dns_label(user);
    if label.is_empty() {
        "dev".into()
    } else {
        label
    }
}

/// DNS form of the macOS user name; "dev" when empty.
pub fn default_prefix() -> String {
    default_prefix_from(&std::env::var("USER").unwrap_or_default())
}

/// `<prefix>-<project>` in DNS form, at most 63 characters.
pub fn suggest_subdomain(prefix: &str, project: &str) -> String {
    let prefix = match dns_label(prefix) {
        p if p.is_empty() => default_prefix(),
        p => p,
    };
    let project = dns_label(project);
    let mut name = if project.is_empty() {
        prefix
    } else {
        format!("{prefix}-{project}")
    };
    name.truncate(MAX_LABEL); // ASCII only
    name.trim_end_matches('-').to_string()
}

/// `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$`
pub fn validate_subdomain(name: &str) -> Result<(), String> {
    let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let b = name.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= MAX_LABEL
        && alnum(b[0])
        && alnum(b[b.len() - 1])
        && b.iter().all(|&c| alnum(c) || c == b'-');
    if ok {
        Ok(())
    } else {
        Err("Use lowercase letters, digits and dashes (63 characters at most).".into())
    }
}

/// Cloudflare tunnel name.
pub fn tunnel_name(subdomain: &str) -> String {
    format!("lsdev-{subdomain}")
}

pub fn hostname(subdomain: &str, domain: &str) -> String {
    format!("{subdomain}.{}", domain.trim_matches('.').to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_label_strips_accents_and_symbols() {
        assert_eq!(dns_label("Éric's App__v2"), "eric-s-app-v2");
        assert_eq!(dns_label("--a--b--"), "a-b");
        assert_eq!(dns_label("a.b/c"), "a-b-c");
        assert_eq!(dns_label("Crème brûlée Straße"), "creme-brulee-strasse");
        assert_eq!(dns_label("日本"), "");
        assert_eq!(dns_label(""), "");
    }

    #[test]
    fn default_prefix_falls_back_to_dev() {
        assert_eq!(default_prefix_from("Khalif Lahbibi"), "khalif-lahbibi");
        assert_eq!(default_prefix_from(""), "dev");
        assert_eq!(default_prefix_from("日本"), "dev");
    }

    #[test]
    fn suggest_subdomain_joins_prefix_and_project() {
        assert_eq!(suggest_subdomain("Khalif", "shop"), "khalif-shop");
        assert_eq!(suggest_subdomain("khalif", ""), "khalif");
        assert_eq!(suggest_subdomain("khalif", "--"), "khalif");
        let empty_prefix = suggest_subdomain("", "shop");
        assert_eq!(empty_prefix, format!("{}-shop", default_prefix()));
    }

    #[test]
    fn suggest_subdomain_is_at_most_63_chars_without_trailing_dash() {
        let long_prefix = suggest_subdomain(&format!("a{}-b", "x".repeat(61)), "");
        assert_eq!(long_prefix.len(), 62);
        assert!(!long_prefix.ends_with('-'));
        let long = "x".repeat(70);
        let name = suggest_subdomain("khalif", &long);
        assert!(name.len() <= 63);
        assert!(!name.ends_with('-'));
        // Cut exactly on the dash between two words.
        let project = format!("{}-tail", "y".repeat(56));
        let name = suggest_subdomain("khalif", &project);
        assert!(name.len() <= 63);
        assert!(!name.ends_with('-'));
        assert!(validate_subdomain(&name).is_ok());
    }

    #[test]
    fn validate_subdomain_applies_the_dns_label_rule() {
        for ok in ["a", "khalif-shop", &"a".repeat(63), "a1-b2"] {
            assert!(validate_subdomain(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-a", "a-", "A", "a.b", "a_b", "é", &"a".repeat(64)] {
            assert_eq!(
                validate_subdomain(bad),
                Err("Use lowercase letters, digits and dashes (63 characters at most).".into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn tunnel_name_and_hostname() {
        assert_eq!(tunnel_name("khalif-shop"), "lsdev-khalif-shop");
        assert_eq!(hostname("x", ".Dev.Example.com."), "x.dev.example.com");
    }
}
