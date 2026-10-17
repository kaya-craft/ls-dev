//! Hand things over to other apps: browser, editor, terminal.

use crate::core::shell::shell_quote;

pub fn container_shell_command(container_id: &str) -> String {
    format!(
        "docker exec -it {} sh -c {}",
        shell_quote(container_id),
        shell_quote("command -v bash >/dev/null 2>&1 && exec bash || exec sh")
    )
}

pub fn editor_command(editor: &str, path: &str) -> String {
    format!("{editor} {}", shell_quote(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_shell_prefers_bash() {
        assert_eq!(
            container_shell_command("abc123"),
            "docker exec -it abc123 sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'"
        );
    }

    #[test]
    fn editor_command_quotes_the_path() {
        assert_eq!(editor_command("cursor", "/w/my app"), "cursor '/w/my app'");
    }
}
