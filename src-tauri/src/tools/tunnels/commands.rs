//! Tauri commands of the Tunnels tool.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::cloudflared::{find_binary, is_logged_in};
use super::names::{hostname, suggest_subdomain, validate_subdomain};
use super::process::{now_ms, quick_args, stable_args, Launch, Tunnel, TunnelKind};
use super::publish;
use super::stable::{self, CliRunner};
use super::state::{TunnelsRef, TunnelsState};
use super::store::{self, StableStore, StableUrl, UNREADABLE};
use crate::core::notify;
use crate::core::shell::{run_launch, shell_quote, terminal_launch};
use crate::core::state::CoreRef;

const NOT_INSTALLED: &str = "cloudflared is not installed. Run: brew install cloudflared";
const BAD_HOURS: &str = "Choose 1, 4, 8 or 24 hours.";
const BAD_PORT: &str = "Choose a port between 1 and 65535.";
const NEEDS_CWD: &str = "A stable URL needs a project folder.";
const NOT_LOGGED_IN: &str = "Connect cloudflared to a Cloudflare account first.";
const NO_DOMAIN: &str = "Set a tunnel domain in Settings first.";
const BAD_DOMAIN: &str = "Set a valid tunnel domain in Settings (for example dev.example.com).";
const NOT_FOUND: &str = "This tunnel is no longer open.";
const NO_STABLE: &str = "No stable URL is remembered for this folder.";
const BAD_STABLE_URL: &str = "tunnels.json contains an invalid stable URL for this folder.";

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StableEntry {
    pub cwd: String,
    pub hostname: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TunnelsStatus {
    pub binary: Option<String>,
    pub logged_in: bool,
    pub domain: Option<String>,
    pub stable: Vec<StableEntry>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedName {
    pub subdomain: String,
    pub remembered: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub port: u16,
    pub project: String,
    pub cwd: Option<String>,
    pub kind: TunnelKind,
    pub subdomain: Option<String>,
    pub hours: u8,
}

/// Tunnel domain setting: whitespace and edge dots trimmed, lowercase;
/// None when blank.
pub fn normalize_domain(setting: &str) -> Option<String> {
    let domain = setting.trim().trim_matches('.').to_lowercase();
    (!domain.is_empty()).then_some(domain)
}

/// Non-empty, only `[a-z0-9.-]`, no empty label.
pub fn validate_domain(domain: Option<&str>) -> Result<(), String> {
    let domain = domain.unwrap_or_default();
    if domain.is_empty() {
        return Err(NO_DOMAIN.into());
    }
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    let ok = domain
        .split('.')
        .all(|label| !label.is_empty() && label.chars().all(allowed))
        && !domain.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(BAD_DOMAIN.into())
    }
}

pub fn status_view(
    binary: Option<&Path>,
    logged_in: bool,
    domain_setting: &str,
    store: &StableStore,
) -> TunnelsStatus {
    TunnelsStatus {
        binary: binary.map(|b| b.to_string_lossy().into_owned()),
        logged_in,
        domain: normalize_domain(domain_setting),
        stable: store
            .entries
            .iter()
            .map(|(cwd, e)| StableEntry {
                cwd: cwd.clone(),
                hostname: e.hostname.clone(),
            })
            .collect(),
    }
}

/// Subdomain of a remembered hostname: the hostname without `.<domain>`, or
/// its first label when the domain setting changed since.
pub fn remembered_subdomain(entry: &StableUrl, domain: Option<&str>) -> String {
    domain
        .and_then(|d| entry.hostname.strip_suffix(&format!(".{d}")))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| entry.hostname.split('.').next().unwrap_or_default())
        .to_string()
}

pub fn suggest_name(
    store: &StableStore,
    cwd: Option<&str>,
    domain: Option<&str>,
    prefix: &str,
    project: &str,
) -> SuggestedName {
    match cwd.and_then(|c| store.get(c)) {
        Some(entry) => SuggestedName {
            subdomain: remembered_subdomain(entry, domain),
            remembered: true,
        },
        None => SuggestedName {
            subdomain: suggest_subdomain(prefix, project),
            remembered: false,
        },
    }
}

/// Every refusal that does not need cloudflared. The subdomain is checked
/// only for a folder without a remembered stable URL (a new name).
pub fn validate_open(request: &OpenRequest, status: &TunnelsStatus) -> Result<(), String> {
    if !matches!(request.hours, 1 | 4 | 8 | 24) {
        return Err(BAD_HOURS.into());
    }
    if request.port == 0 {
        return Err(BAD_PORT.into());
    }
    if status.binary.is_none() {
        return Err(NOT_INSTALLED.into());
    }
    if request.kind == TunnelKind::Quick {
        return Ok(());
    }
    let Some(cwd) = request.cwd.as_deref() else {
        return Err(NEEDS_CWD.into());
    };
    if !status.logged_in {
        return Err(NOT_LOGGED_IN.into());
    }
    validate_domain(status.domain.as_deref())?;
    if status.stable.iter().any(|e| e.cwd == cwd) {
        return Ok(());
    }
    validate_subdomain(request.subdomain.as_deref().unwrap_or_default())
}

/// A tunnel id from `tunnels.json` passed as a cloudflared argument.
fn valid_tunnel_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && !id.starts_with('-')
}

/// A remembered entry before its use as `known_url` and cloudflared
/// argument: a hostname under the domain rules whose first label follows the
/// subdomain rule, and a valid tunnel id.
fn check_entry(entry: &StableUrl) -> Result<(), String> {
    let host = entry.hostname.as_str();
    let first = host.split('.').next().unwrap_or_default();
    let ok = host.contains('.')
        && validate_domain(Some(host)).is_ok()
        && validate_subdomain(first).is_ok()
        && valid_tunnel_id(&entry.tunnel_id);
    if ok {
        Ok(())
    } else {
        Err(BAD_STABLE_URL.into())
    }
}

pub fn login_command(binary: &Path) -> String {
    format!(
        "{} tunnel --no-autoupdate login",
        shell_quote(&binary.to_string_lossy())
    )
}

async fn locate_binary() -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(find_binary)
        .await
        .map_err(|e| e.to_string())
}

fn logged_in(app: &AppHandle) -> bool {
    app.path().home_dir().is_ok_and(|home| is_logged_in(&home))
}

/// Domain and prefix settings.
fn tunnel_settings(core: &CoreRef<'_>) -> (String, String) {
    let settings = core.settings.read().unwrap_or_else(|e| e.into_inner());
    (
        settings.tunnel_domain.clone(),
        settings.tunnel_prefix.clone(),
    )
}

async fn current_status(
    app: &AppHandle,
    core: &CoreRef<'_>,
    state: &TunnelsState,
) -> Result<(TunnelsStatus, Option<PathBuf>), String> {
    let binary = locate_binary().await?;
    let (domain, _) = tunnel_settings(core);
    let status = status_view(
        binary.as_deref(),
        logged_in(app),
        &domain,
        &lock(&state.stable),
    );
    Ok((status, binary))
}

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
pub async fn tunnels_status(
    app: AppHandle,
    core: CoreRef<'_>,
    state: TunnelsRef<'_>,
) -> Result<TunnelsStatus, String> {
    Ok(current_status(&app, &core, &state).await?.0)
}

#[tauri::command]
pub fn tunnels_list(state: TunnelsRef<'_>) -> Vec<Tunnel> {
    state.manager.list()
}

#[tauri::command]
pub fn tunnels_suggest_name(
    core: CoreRef<'_>,
    state: TunnelsRef<'_>,
    cwd: Option<String>,
    project: String,
) -> SuggestedName {
    let (domain, prefix) = tunnel_settings(&core);
    suggest_name(
        &lock(&state.stable),
        cwd.as_deref(),
        normalize_domain(&domain).as_deref(),
        &prefix,
        &project,
    )
}

/// The remembered stable URL of `cwd`, or a new one created with
/// cloudflared (on a blocking thread) and saved in `tunnels.json`.
async fn stable_url(
    app: &AppHandle,
    state: &TunnelsState,
    binary: PathBuf,
    cwd: &str,
    subdomain: String,
    domain: String,
) -> Result<StableUrl, String> {
    let _ops = state.stable_ops.lock().await;
    {
        let store = lock(&state.stable);
        if let Some(entry) = store.get(cwd) {
            return Ok(entry.clone());
        }
        if store.unreadable {
            return Err(UNREADABLE.into());
        }
    }
    // Checked again here: the entry may have been deleted since validate_open.
    validate_subdomain(&subdomain)?;
    validate_domain(Some(&domain))?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let entry = tauri::async_runtime::spawn_blocking(move || {
        stable::create(&CliRunner { binary }, &subdomain, &domain, &home)
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut store = lock(&state.stable);
    store.put(cwd, entry.clone());
    // Kept in memory on a write failure: a retry reuses the created tunnel.
    store::save(app, &store)?;
    Ok(entry)
}

#[tauri::command]
pub async fn tunnels_open(
    app: AppHandle,
    core: CoreRef<'_>,
    state: TunnelsRef<'_>,
    request: OpenRequest,
) -> Result<Tunnel, String> {
    let (status, binary) = current_status(&app, &core, &state).await?;
    let mut request = request;
    if request.kind == TunnelKind::Stable && request.subdomain.is_none() {
        let (_, prefix) = tunnel_settings(&core);
        let suggested = suggest_name(
            &lock(&state.stable),
            request.cwd.as_deref(),
            status.domain.as_deref(),
            &prefix,
            &request.project,
        );
        request.subdomain = Some(suggested.subdomain);
    }
    validate_open(&request, &status)?;
    let binary = binary.ok_or(NOT_INSTALLED)?;
    let cwd = request.cwd.clone().unwrap_or_default();
    let hostname = match request.kind {
        TunnelKind::Quick => String::new(),
        TunnelKind::Stable => lock(&state.stable)
            .get(&cwd)
            .map(|e| e.hostname.clone())
            .unwrap_or_else(|| {
                hostname(
                    request.subdomain.as_deref().unwrap_or_default(),
                    status.domain.as_deref().unwrap_or_default(),
                )
            }),
    };
    let stable_key =
        (request.kind == TunnelKind::Stable).then_some((cwd.as_str(), hostname.as_str()));
    let _reserved = state.reserve(request.port, stable_key)?;
    let launch = match request.kind {
        TunnelKind::Quick => Launch {
            binary,
            args: quick_args(request.port),
            kind: TunnelKind::Quick,
            known_url: None,
        },
        TunnelKind::Stable => {
            let entry = stable_url(
                &app,
                &state,
                binary.clone(),
                &cwd,
                request.subdomain.clone().unwrap_or_default(),
                status.domain.clone().unwrap_or_default(),
            )
            .await?;
            check_entry(&entry)?;
            Launch {
                binary,
                args: stable_args(request.port, &entry.tunnel_id),
                kind: TunnelKind::Stable,
                known_url: Some(format!("https://{}", entry.hostname)),
            }
        }
    };
    let (on_change_app, on_exit_app) = (app.clone(), app.clone());
    let tunnel = state
        .manager
        .start(
            launch,
            request.port,
            request.project,
            request.cwd,
            request.hours,
            move || publish(&on_change_app),
            move |t: Tunnel| notify::send(&on_exit_app, &format!("Tunnel stopped: {}", t.project)),
        )
        .await;
    publish(&app);
    Ok(tunnel)
}

#[tauri::command]
pub fn tunnels_extend(app: AppHandle, state: TunnelsRef<'_>, id: String) -> Result<Tunnel, String> {
    let tunnel = state.manager.extend(&id, now_ms()).ok_or(NOT_FOUND)?;
    publish(&app);
    Ok(tunnel)
}

#[tauri::command]
pub async fn tunnels_stop(app: AppHandle, state: TunnelsRef<'_>, id: String) -> Result<(), String> {
    state.manager.stop(&id).await;
    publish(&app);
    Ok(())
}

#[tauri::command]
pub fn tunnels_output(state: TunnelsRef<'_>, id: String) -> Result<Vec<String>, String> {
    state.manager.output(&id).ok_or_else(|| NOT_FOUND.into())
}

#[tauri::command]
pub async fn tunnels_delete_stable(
    app: AppHandle,
    state: TunnelsRef<'_>,
    cwd: String,
) -> Result<(), String> {
    let _ops = state.stable_ops.lock().await;
    state.check_deletable(&cwd)?;
    let entry = lock(&state.stable).get(&cwd).cloned().ok_or(NO_STABLE)?;
    if !valid_tunnel_id(&entry.tunnel_id) {
        return Err(BAD_STABLE_URL.into());
    }
    let binary = locate_binary().await?.ok_or(NOT_INSTALLED)?;
    tauri::async_runtime::spawn_blocking(move || stable::delete(&CliRunner { binary }, &entry))
        .await
        .map_err(|e| e.to_string())??;
    let mut store = lock(&state.stable);
    store.remove(&cwd);
    store::save(&app, &store)
}

#[tauri::command]
pub async fn tunnels_login(core: CoreRef<'_>) -> Result<(), String> {
    let binary = locate_binary().await?.ok_or(NOT_INSTALLED)?;
    let terminal = core
        .settings
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .terminal
        .clone();
    let launch = terminal_launch(&terminal, &login_command(&binary));
    tauri::async_runtime::spawn_blocking(move || run_launch(launch))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hostname: &str) -> StableUrl {
        StableUrl {
            hostname: hostname.into(),
            tunnel_name: "lsdev-khalif-shop".into(),
            tunnel_id: "6ff42ae2-765d-4adf-8112-31c55c1551ef".into(),
        }
    }

    fn store_with(cwd: &str, hostname: &str) -> StableStore {
        let mut s = StableStore::default();
        s.put(cwd, entry(hostname));
        s
    }

    fn ready() -> TunnelsStatus {
        TunnelsStatus {
            binary: Some("/opt/homebrew/bin/cloudflared".into()),
            logged_in: true,
            domain: Some("dev.example.com".into()),
            stable: vec![StableEntry {
                cwd: "/w/known".into(),
                hostname: "khalif-known.dev.example.com".into(),
            }],
        }
    }

    fn request(kind: TunnelKind) -> OpenRequest {
        OpenRequest {
            port: 3000,
            project: "shop".into(),
            cwd: Some("/w/shop".into()),
            kind,
            subdomain: Some("khalif-shop".into()),
            hours: 4,
        }
    }

    #[test]
    fn normalize_domain_trims_and_lowercases() {
        assert_eq!(
            normalize_domain("  .Dev.Example.com. ").as_deref(),
            Some("dev.example.com")
        );
        assert_eq!(normalize_domain(""), None);
        assert_eq!(normalize_domain("   "), None);
        assert_eq!(normalize_domain(".."), None);
    }

    #[test]
    fn validate_domain_rejects_blank_and_malformed_domains() {
        assert_eq!(validate_domain(Some("dev.example.com")), Ok(()));
        assert_eq!(validate_domain(Some("a-b.example.com")), Ok(()));
        assert_eq!(validate_domain(None), Err(NO_DOMAIN.into()));
        for bad in [
            "dev..example.com",
            "dev example.com",
            "dev_example.com",
            "ex\u{e9}mple.com",
            "dev.example.com/x",
            "--url",
            "",
        ] {
            let expected = if bad.is_empty() {
                NO_DOMAIN
            } else {
                BAD_DOMAIN
            };
            assert_eq!(validate_domain(Some(bad)), Err(expected.into()), "{bad}");
        }
    }

    #[test]
    fn status_view_reports_a_blank_domain_as_none() {
        let store = store_with("/w/a", "khalif-a.dev.example.com");
        let status = status_view(
            Some(Path::new("/opt/homebrew/bin/cloudflared")),
            true,
            "  ",
            &store,
        );
        assert_eq!(status.domain, None);
        assert_eq!(
            status.binary.as_deref(),
            Some("/opt/homebrew/bin/cloudflared")
        );
        assert!(status.logged_in);
        assert_eq!(
            status.stable,
            vec![StableEntry {
                cwd: "/w/a".into(),
                hostname: "khalif-a.dev.example.com".into()
            }]
        );
        let status = status_view(None, false, "Dev.Example.com", &StableStore::default());
        assert_eq!(status.domain.as_deref(), Some("dev.example.com"));
        assert_eq!(status.binary, None);
        assert!(status.stable.is_empty());
    }

    #[test]
    fn status_serializes_in_camel_case() {
        let v = serde_json::to_value(ready()).unwrap();
        assert_eq!(v["loggedIn"], true);
        assert_eq!(v["stable"][0]["hostname"], "khalif-known.dev.example.com");
    }

    #[test]
    fn remembered_subdomain_strips_the_domain() {
        let e = entry("khalif-shop.dev.example.com");
        assert_eq!(
            remembered_subdomain(&e, Some("dev.example.com")),
            "khalif-shop"
        );
        assert_eq!(
            remembered_subdomain(&e, Some("other.example.com")),
            "khalif-shop"
        );
        assert_eq!(remembered_subdomain(&e, None), "khalif-shop");
        let e = entry("a.b.dev.example.com");
        assert_eq!(remembered_subdomain(&e, Some("dev.example.com")), "a.b");
    }

    #[test]
    fn suggest_name_prefers_the_remembered_entry() {
        let store = store_with("/w/shop", "kept-name.dev.example.com");
        assert_eq!(
            suggest_name(
                &store,
                Some("/w/shop"),
                Some("dev.example.com"),
                "khalif",
                "shop"
            ),
            SuggestedName {
                subdomain: "kept-name".into(),
                remembered: true
            }
        );
        assert_eq!(
            suggest_name(
                &store,
                Some("/w/other"),
                Some("dev.example.com"),
                "khalif",
                "Other App"
            ),
            SuggestedName {
                subdomain: "khalif-other-app".into(),
                remembered: false
            }
        );
        assert_eq!(
            suggest_name(&store, None, None, "khalif", "shop"),
            SuggestedName {
                subdomain: "khalif-shop".into(),
                remembered: false
            }
        );
    }

    #[test]
    fn validate_open_accepts_valid_requests() {
        assert_eq!(validate_open(&request(TunnelKind::Quick), &ready()), Ok(()));
        assert_eq!(
            validate_open(&request(TunnelKind::Stable), &ready()),
            Ok(())
        );
        let quick_without_setup = TunnelsStatus {
            logged_in: false,
            domain: None,
            ..ready()
        };
        let mut r = request(TunnelKind::Quick);
        r.cwd = None;
        r.subdomain = None;
        assert_eq!(validate_open(&r, &quick_without_setup), Ok(()));
        for hours in [1, 4, 8, 24] {
            r.hours = hours;
            assert_eq!(validate_open(&r, &ready()), Ok(()));
        }
    }

    #[test]
    fn validate_open_refuses_bad_hours_port_and_missing_binary() {
        let mut r = request(TunnelKind::Quick);
        for hours in [0, 2, 12, 25, 255] {
            r.hours = hours;
            assert_eq!(validate_open(&r, &ready()), Err(BAD_HOURS.into()));
        }
        let mut r = request(TunnelKind::Quick);
        r.port = 0;
        assert_eq!(validate_open(&r, &ready()), Err(BAD_PORT.into()));
        let no_binary = TunnelsStatus {
            binary: None,
            ..ready()
        };
        assert_eq!(
            validate_open(&request(TunnelKind::Quick), &no_binary),
            Err(NOT_INSTALLED.into())
        );
    }

    #[test]
    fn validate_open_refuses_stable_without_setup() {
        let mut r = request(TunnelKind::Stable);
        r.cwd = None;
        assert_eq!(validate_open(&r, &ready()), Err(NEEDS_CWD.into()));
        let logged_out = TunnelsStatus {
            logged_in: false,
            ..ready()
        };
        assert_eq!(
            validate_open(&request(TunnelKind::Stable), &logged_out),
            Err(NOT_LOGGED_IN.into())
        );
        let no_domain = TunnelsStatus {
            domain: None,
            ..ready()
        };
        assert_eq!(
            validate_open(&request(TunnelKind::Stable), &no_domain),
            Err(NO_DOMAIN.into())
        );
        let bad_domain = TunnelsStatus {
            domain: Some("dev..example.com".into()),
            ..ready()
        };
        assert_eq!(
            validate_open(&request(TunnelKind::Stable), &bad_domain),
            Err(BAD_DOMAIN.into())
        );
    }

    #[test]
    fn validate_open_checks_a_new_subdomain_only() {
        let names_error = "Use lowercase letters, digits and dashes (63 characters at most).";
        let mut r = request(TunnelKind::Stable);
        for bad in [None, Some("Bad_Name".to_string()), Some("-x".to_string())] {
            r.subdomain = bad;
            assert_eq!(validate_open(&r, &ready()), Err(names_error.into()));
        }
        // A remembered folder reuses its entry: the typed name is not used.
        r.cwd = Some("/w/known".into());
        r.subdomain = None;
        assert_eq!(validate_open(&r, &ready()), Ok(()));
    }

    #[test]
    fn tunnel_ids_from_the_store_are_checked() {
        assert!(valid_tunnel_id("6ff42ae2-765d-4adf-8112-31c55c1551ef"));
        assert!(!valid_tunnel_id(""));
        assert!(!valid_tunnel_id("--url"));
        assert!(!valid_tunnel_id("abc def"));
    }

    #[test]
    fn remembered_entries_are_checked_before_use() {
        assert_eq!(check_entry(&entry("khalif-shop.dev.example.com")), Ok(()));
        for host in [
            "",
            "dev.example.com.",
            "-x.dev.example.com",
            "Upper.dev.example.com",
            "a..example.com",
            "a.example.com/path",
            "a b.example.com",
            "localhost",
        ] {
            assert_eq!(
                check_entry(&entry(host)),
                Err(BAD_STABLE_URL.into()),
                "{host}"
            );
        }
        let mut bad_id = entry("khalif-shop.dev.example.com");
        bad_id.tunnel_id = "--url".into();
        assert_eq!(check_entry(&bad_id), Err(BAD_STABLE_URL.into()));
    }

    #[test]
    fn login_command_quotes_the_binary() {
        assert_eq!(
            login_command(Path::new("/opt/homebrew/bin/cloudflared")),
            "/opt/homebrew/bin/cloudflared tunnel --no-autoupdate login"
        );
        assert_eq!(
            login_command(Path::new("/Users/me/my bin/cloudflared")),
            "'/Users/me/my bin/cloudflared' tunnel --no-autoupdate login"
        );
    }

    #[test]
    fn open_request_deserializes_from_camel_case() {
        let r: OpenRequest = serde_json::from_value(serde_json::json!({
            "port": 3000, "project": "shop", "cwd": null,
            "kind": "quick", "subdomain": null, "hours": 4
        }))
        .unwrap();
        assert_eq!(r.kind, TunnelKind::Quick);
        assert_eq!(r.cwd, None);
    }
}
