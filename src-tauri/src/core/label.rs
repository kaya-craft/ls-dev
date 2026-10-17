//! Human labels for paths and launcher commands. Pure.

/// Last path segment.
pub fn basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

/// `pnpm.cjs` -> `pnpm`.
pub fn strip_js_ext(name: &str) -> &str {
    for ext in [".cjs", ".mjs", ".js"] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem;
        }
    }
    name
}

/// `~/Developer/@acme/shop/packages/backend` -> `shop/packages/backend`;
/// other paths under $HOME get `~/`; anything else is returned as is.
pub fn project_label(cwd: Option<&str>, home: &str) -> Option<String> {
    let cwd = cwd?;
    let dev_root = format!("{home}/Developer/");
    if let Some(rest) = cwd.strip_prefix(&dev_root) {
        let mut segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        if segments.first().is_some_and(|s| s.starts_with('@')) {
            segments.remove(0);
        }
        let label = segments.join("/");
        return Some(if label.is_empty() {
            basename(cwd).to_string()
        } else {
            label
        });
    }
    if let Some(rest) = cwd.strip_prefix(&format!("{home}/")) {
        return Some(format!("~/{rest}"));
    }
    Some(cwd.to_string())
}

/// Launcher argv without the interpreter and the absolute path of the binary:
/// `node /…/bin/pnpm --filter x dev` -> `["pnpm", "--filter", "x", "dev"]`.
pub fn readable_argv(argv: &[String]) -> Vec<String> {
    if argv.is_empty() {
        return Vec::new();
    }
    let interpreter = matches!(basename(&argv[0]), "node" | "bun" | "deno");
    let start = if interpreter && argv.get(1).is_some_and(|a| a.contains('/')) {
        1
    } else {
        0
    };
    let mut rest = argv[start..].to_vec();
    rest[0] = strip_js_ext(basename(&rest[0])).to_string();
    rest
}

pub fn format_script(argv: &[String]) -> String {
    readable_argv(argv).join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::HOME;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn project_label_shortens_developer_paths_and_drops_org() {
        let l = |p: &str| project_label(Some(p), HOME);
        assert_eq!(
            l("/Users/klahbibi/Developer/@acme/shop/packages/backend").as_deref(),
            Some("shop/packages/backend")
        );
        assert_eq!(
            l("/Users/klahbibi/Developer/@acme/blog").as_deref(),
            Some("blog")
        );
        assert_eq!(
            l("/Users/klahbibi/Developer/plain/app").as_deref(),
            Some("plain/app")
        );
        assert_eq!(
            l("/Users/klahbibi/tmp/thing").as_deref(),
            Some("~/tmp/thing")
        );
        assert_eq!(l("/opt/service").as_deref(), Some("/opt/service"));
        assert_eq!(project_label(None, HOME), None);
    }

    #[test]
    fn format_script_drops_interpreter_and_absolute_path() {
        assert_eq!(
            format_script(&argv("node /Users/klahbibi/.local/state/fnm_multishells/84858/bin/pnpm --filter @acme/backend dev")),
            "pnpm --filter @acme/backend dev"
        );
        assert_eq!(
            format_script(&argv(
                "node /Users/k/store/node_modules/pnpm/bin/pnpm.cjs dev"
            )),
            "pnpm dev"
        );
        assert_eq!(format_script(&argv("/sbin/launchd")), "launchd");
        assert_eq!(format_script(&[]), "");
    }

    #[test]
    fn readable_argv_keeps_arguments_as_separate_tokens() {
        assert_eq!(
            readable_argv(&argv("node /x/bin/pnpm --filter @acme/backend dev")),
            vec!["pnpm", "--filter", "@acme/backend", "dev"]
        );
    }
}
