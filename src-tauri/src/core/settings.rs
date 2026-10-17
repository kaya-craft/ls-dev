//! User settings, persisted with tauri-plugin-store.

use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalSetting {
    TerminalApp,
    Iterm,
    /// A new cmux workspace, through the cmux CLI.
    Cmux,
    /// Command template; `{cmd}` is replaced by the shell-quoted command.
    Custom(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeSetting {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeSetting {
    /// None follows the system appearance.
    pub fn tauri_theme(self) -> Option<tauri::Theme> {
        match self {
            ThemeSetting::System => None,
            ThemeSetting::Light => Some(tauri::Theme::Light),
            ThemeSetting::Dark => Some(tauri::Theme::Dark),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub poll_interval_ms: u64,
    pub show_all: bool,
    pub terminal: TerminalSetting,
    pub editor_command: String,
    pub notify_on_stop: bool,
    pub launch_at_login: bool,
    pub theme: ThemeSetting,
    /// Extra folders scanned for git repositories, besides the home directory.
    pub worktree_roots: Vec<String>,
    /// Repositories hidden from the Worktrees tool.
    pub worktree_excludes: Vec<String>,
    /// Domain of the stable tunnel URLs (a zone of the connected Cloudflare account).
    pub tunnel_domain: String,
    /// Prefix of the suggested tunnel subdomains; empty means the macOS user name.
    pub tunnel_prefix: String,
    /// Default tunnel duration in hours: 1, 4, 8 or 24.
    pub tunnel_hours: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_interval_ms: 3000,
            show_all: false,
            terminal: TerminalSetting::TerminalApp,
            editor_command: String::new(),
            notify_on_stop: true,
            launch_at_login: false,
            theme: ThemeSetting::System,
            worktree_roots: Vec::new(),
            worktree_excludes: Vec::new(),
            tunnel_domain: String::new(),
            tunnel_prefix: String::new(),
            tunnel_hours: 4,
        }
    }
}

impl Settings {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.poll_interval_ms = self.poll_interval_ms.clamp(2000, 10_000);
        self.editor_command = self.editor_command.trim().to_string();
        if !matches!(self.tunnel_hours, 1 | 4 | 8 | 24) {
            self.tunnel_hours = 4;
        }
        if let TerminalSetting::Custom(template) = &self.terminal {
            if !template.contains("{cmd}") {
                return Err("The custom terminal command must contain {cmd}.".into());
            }
        }
        Ok(self)
    }
}

pub fn pick_editor(available: impl Fn(&str) -> bool) -> String {
    for candidate in ["cursor", "code"] {
        if available(candidate) {
            return candidate.to_string();
        }
    }
    "open".to_string()
}

/// The user's login shell; zsh when $SHELL is unset.
pub fn shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// `command -v <name>` in a login shell, so that PATH matches the terminal's.
pub fn command_exists(name: &str) -> bool {
    Command::new(shell())
        .args(["-lc", &format!("command -v {name}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Settings from a stored JSON value: deserialize, normalize. An invalid
/// custom terminal falls back to Terminal.app without losing other fields;
/// an undecodable value falls back to the defaults. Failures are logged.
fn from_stored(value: Option<serde_json::Value>) -> Settings {
    let Some(value) = value else {
        return Settings::default();
    };
    let stored = match serde_json::from_value::<Settings>(value) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("LsDev settings: stored value is invalid, using defaults: {e}");
            return Settings::default();
        }
    };
    match stored.clone().normalized() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("LsDev settings: {e} Falling back to Terminal.app.");
            Settings {
                terminal: TerminalSetting::TerminalApp,
                ..stored
            }
            .normalized()
            .unwrap_or_else(|e| {
                eprintln!("LsDev settings: normalization failed, using defaults: {e}");
                Settings::default()
            })
        }
    }
}

/// Stored settings, normalized; detects the editor on first launch.
pub fn load(app: &AppHandle) -> Settings {
    let value = match app.store(STORE_FILE) {
        Ok(store) => store.get(STORE_KEY),
        Err(e) => {
            eprintln!("LsDev settings: cannot open the store: {e}");
            None
        }
    };
    let mut settings = from_stored(value);
    if settings.editor_command.is_empty() {
        settings.editor_command = pick_editor(command_exists);
        if let Err(e) = save(app, &settings) {
            eprintln!("LsDev settings: cannot save the detected editor: {e}");
        }
    }
    settings
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY,
        serde_json::to_value(settings).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let s = Settings::default();
        assert_eq!(s.poll_interval_ms, 3000);
        assert!(!s.show_all);
        assert_eq!(s.terminal, TerminalSetting::TerminalApp);
        assert!(s.notify_on_stop);
        assert!(!s.launch_at_login);
    }

    #[test]
    fn theme_is_kebab_case_and_defaults_to_system() {
        assert_eq!(Settings::default().theme, ThemeSetting::System);
        assert_eq!(
            serde_json::to_string(&ThemeSetting::Dark).unwrap(),
            r#""dark""#
        );
        let s: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(s.theme, ThemeSetting::Light);
        assert_eq!(ThemeSetting::System.tauri_theme(), None);
        assert_eq!(ThemeSetting::Dark.tauri_theme(), Some(tauri::Theme::Dark));
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"showAll":true}"#).unwrap();
        assert!(s.show_all);
        assert_eq!(s.poll_interval_ms, 3000);
    }

    #[test]
    fn terminal_setting_serializes_like_the_frontend_expects() {
        assert_eq!(
            serde_json::to_string(&TerminalSetting::TerminalApp).unwrap(),
            r#""terminal-app""#
        );
        assert_eq!(
            serde_json::to_string(&TerminalSetting::Iterm).unwrap(),
            r#""iterm""#
        );
        assert_eq!(
            serde_json::to_string(&TerminalSetting::Cmux).unwrap(),
            r#""cmux""#
        );
        assert_eq!(
            serde_json::to_string(&TerminalSetting::Custom("cmux {cmd}".into())).unwrap(),
            r#"{"custom":"cmux {cmd}"}"#
        );
    }

    #[test]
    fn normalized_clamps_the_interval() {
        let low = Settings {
            poll_interval_ms: 500,
            ..Settings::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(low.poll_interval_ms, 2000);
        let high = Settings {
            poll_interval_ms: 60_000,
            ..Settings::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(high.poll_interval_ms, 10_000);
    }

    #[test]
    fn normalized_rejects_a_custom_terminal_without_placeholder() {
        let s = Settings {
            terminal: TerminalSetting::Custom("ghostty".into()),
            ..Settings::default()
        };
        assert!(s.normalized().is_err());
    }

    #[test]
    fn pick_editor_prefers_cursor_then_code_then_finder() {
        assert_eq!(pick_editor(|c| c == "cursor" || c == "code"), "cursor");
        assert_eq!(pick_editor(|c| c == "code"), "code");
        assert_eq!(pick_editor(|_| false), "open");
    }

    #[test]
    fn from_stored_keeps_other_fields_when_the_custom_terminal_is_invalid() {
        let v = serde_json::json!({"showAll": true, "terminal": {"custom": "ghostty"}});
        let s = from_stored(Some(v));
        assert!(s.show_all);
        assert_eq!(s.terminal, TerminalSetting::TerminalApp);
    }

    #[test]
    fn from_stored_uses_defaults_on_a_wrong_type() {
        let s = from_stored(Some(serde_json::json!({"pollIntervalMs": "fast"})));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn stored_settings_without_worktree_fields_load_with_empty_vectors() {
        let s = from_stored(Some(serde_json::json!({"showAll": true})));
        assert!(s.show_all);
        assert!(s.worktree_roots.is_empty());
        assert!(s.worktree_excludes.is_empty());
    }

    #[test]
    fn worktree_fields_serialize_in_camel_case() {
        let s = Settings {
            worktree_roots: vec!["/a".into()],
            worktree_excludes: vec!["/b".into()],
            ..Settings::default()
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["worktreeRoots"], serde_json::json!(["/a"]));
        assert_eq!(v["worktreeExcludes"], serde_json::json!(["/b"]));
    }

    #[test]
    fn stored_settings_without_tunnel_fields_load_with_defaults() {
        let s = from_stored(Some(serde_json::json!({"showAll": true})));
        assert_eq!(s.tunnel_domain, "");
        assert_eq!(s.tunnel_prefix, "");
        assert_eq!(s.tunnel_hours, 4);
    }

    #[test]
    fn tunnel_hours_outside_the_allowed_values_become_four() {
        let load = |h: u8| from_stored(Some(serde_json::json!({"tunnelHours": h}))).tunnel_hours;
        assert_eq!(load(5), 4);
        assert_eq!(load(0), 4);
        for h in [1, 4, 8, 24] {
            assert_eq!(load(h), h);
        }
    }

    #[test]
    fn tunnel_fields_serialize_in_camel_case() {
        let s = Settings {
            tunnel_domain: "dev.example.com".into(),
            tunnel_prefix: "khalif".into(),
            tunnel_hours: 8,
            ..Settings::default()
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["tunnelDomain"], "dev.example.com");
        assert_eq!(v["tunnelPrefix"], "khalif");
        assert_eq!(v["tunnelHours"], 8);
    }

    #[test]
    fn from_stored_uses_defaults_when_nothing_is_stored() {
        assert_eq!(from_stored(None), Settings::default());
    }
}
