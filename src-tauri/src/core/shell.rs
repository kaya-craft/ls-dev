//! Shell helpers: quoting, `exec` lines, hand-over to osascript or the login shell.

use std::process::{Command, Stdio};

use super::settings::{shell, TerminalSetting};

/// POSIX single-quote unless the word only has safe characters.
pub fn shell_quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@=+,%".contains(c));
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// `exec` so that the shell is replaced: the spawned pid is the launcher.
pub fn exec_line(argv: &[String]) -> String {
    let words: Vec<String> = argv.iter().map(|a| shell_quote(a)).collect();
    format!("exec {}", words.join(" "))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// One `-e` argument per line.
    Osascript(Vec<String>),
    /// Run through the user's login shell.
    Shell(String),
}

pub fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Shell line that leaves an interactive login shell in `dir`: the `exec`
/// keeps the window open with a custom terminal template too.
pub fn shell_in_dir(dir: &str) -> String {
    format!(
        "cd {} && exec \"${{SHELL:-/bin/zsh}}\" -l",
        shell_quote(dir)
    )
}

/// The cmux CLI is only on the PATH inside cmux terminals.
const CMUX_CLI: &str = "/Applications/cmux.app/Contents/Resources/bin/cmux";

/// Opens `cmd` in the terminal chosen in Settings.
pub fn terminal_launch(setting: &TerminalSetting, cmd: &str) -> Launch {
    match setting {
        TerminalSetting::TerminalApp => Launch::Osascript(vec![
            format!(
                "tell application \"Terminal\" to do script {}",
                applescript_string(cmd)
            ),
            "tell application \"Terminal\" to activate".into(),
        ]),
        TerminalSetting::Iterm => Launch::Osascript(vec![
            "tell application \"iTerm\"".into(),
            "set w to (create window with default profile)".into(),
            format!(
                "tell current session of w to write text {}",
                applescript_string(cmd)
            ),
            "activate".into(),
            "end tell".into(),
        ]),
        TerminalSetting::Cmux => Launch::Shell(format!(
            "{CMUX_CLI} new-workspace --command {}",
            shell_quote(cmd)
        )),
        TerminalSetting::Custom(template) => {
            Launch::Shell(template.replace("{cmd}", &shell_quote(cmd)))
        }
    }
}

pub fn run_launch(launch: Launch) -> Result<(), String> {
    let output = match launch {
        Launch::Osascript(lines) => {
            let mut cmd = Command::new("osascript");
            for line in &lines {
                cmd.arg("-e").arg(line);
            }
            cmd.stdin(Stdio::null()).output()
        }
        Launch::Shell(line) => Command::new(shell())
            .arg("-lc")
            .arg(line)
            .stdin(Stdio::null())
            .output(),
    }
    .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_leaves_plain_words_and_quotes_the_rest() {
        assert_eq!(shell_quote("pnpm"), "pnpm");
        assert_eq!(shell_quote("@acme/backend"), "@acme/backend");
        assert_eq!(shell_quote("--filter=a b"), "'--filter=a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn exec_line_quotes_arguments() {
        let argv = vec![
            "pnpm".to_string(),
            "--filter".into(),
            "@acme/backend".into(),
            "dev".into(),
        ];
        assert_eq!(exec_line(&argv), "exec pnpm --filter @acme/backend dev");
    }

    #[test]
    fn shell_in_dir_quotes_the_directory() {
        assert_eq!(
            shell_in_dir("/w/my repo"),
            r#"cd '/w/my repo' && exec "${SHELL:-/bin/zsh}" -l"#
        );
    }

    #[test]
    fn applescript_string_escapes_quotes_and_backslashes() {
        assert_eq!(
            applescript_string(r#"say "hi" \o/"#),
            r#""say \"hi\" \\o/""#
        );
    }

    #[test]
    fn terminal_app_uses_do_script() {
        let Launch::Osascript(lines) = terminal_launch(&TerminalSetting::TerminalApp, "echo hi")
        else {
            panic!()
        };
        assert_eq!(
            lines[0],
            r#"tell application "Terminal" to do script "echo hi""#
        );
        assert_eq!(lines[1], r#"tell application "Terminal" to activate"#);
    }

    #[test]
    fn iterm_writes_the_command_in_a_new_window() {
        let Launch::Osascript(lines) = terminal_launch(&TerminalSetting::Iterm, "echo hi") else {
            panic!()
        };
        assert!(lines.iter().any(|l| l.contains(r#"write text "echo hi""#)));
    }

    #[test]
    fn cmux_opens_a_new_workspace_running_the_command() {
        assert_eq!(
            terminal_launch(&TerminalSetting::Cmux, "docker exec -it a sh"),
            Launch::Shell(
                "/Applications/cmux.app/Contents/Resources/bin/cmux new-workspace --command 'docker exec -it a sh'"
                    .into()
            )
        );
    }

    #[test]
    fn custom_template_receives_one_quoted_argument() {
        let launch = terminal_launch(
            &TerminalSetting::Custom("cmux run {cmd}".into()),
            "docker exec -it a sh",
        );
        assert_eq!(
            launch,
            Launch::Shell("cmux run 'docker exec -it a sh'".into())
        );
    }
}
