//! Tauri commands of the Databases tool. The command bodies delegate to the
//! helpers below, which do not need an `AppHandle` and are tested.

use std::sync::Arc;

use bollard::container::InspectContainerOptions;
use serde::Serialize;
use tauri::AppHandle;

use super::connections::{
    self, credentials_from_env, detect, ConnectionDraft, ContainerCredentials, DetectedConnection,
    SavedConnection,
};
use super::results::RunOutput;
use super::session::{
    connect, connect_message, RunError, Session, Target, CANCEL_GRACE, TRANSACTION_OPEN,
};
use super::state::{lock, DatabasesRef, DatabasesState};
use super::url::{parse_url, ParsedUrl, SslMode};
use crate::core::state::{CoreRef, CoreState};
use crate::tools::services::docker_stats::DockerCollector;

const GONE: &str = "This connection no longer exists.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsView {
    pub saved: Vec<SavedConnection>,
    pub detected: Vec<DetectedConnection>,
    pub docker_available: bool,
}

pub fn view(
    saved: Vec<SavedConnection>,
    detected: Vec<DetectedConnection>,
    docker_available: bool,
) -> ConnectionsView {
    ConnectionsView {
        saved,
        detected,
        docker_available,
    }
}

/// A non-empty password typed in the form wins over the saved one.
pub fn resolve_password(saved: Option<String>, given: Option<String>) -> Option<String> {
    given.filter(|p| !p.is_empty()).or(saved)
}

/// Target of a detected container, reached on the loopback through its
/// published port. Local containers do not use TLS.
pub fn detected_target(
    d: &DetectedConnection,
    creds: ContainerCredentials,
) -> Result<Target, RunError> {
    if !d.running {
        return Err(other("Start the container first."));
    }
    let port = d
        .port
        .ok_or_else(|| other("This container does not publish port 5432."))?;
    Ok(Target {
        host: "127.0.0.1".to_string(),
        port,
        user: creds.user,
        password: creds.password,
        database: creds.database,
        ssl_mode: SslMode::Disable,
    })
}

/// An authentication failure on a saved connection with no keychain
/// password is explained instead of showing the raw error. Two shapes: the
/// server rejects the login (`28P01`, `28000`), or the server asks for a
/// password and tokio-postgres fails client-side with "password missing",
/// which `connect` reports as a `Connect` error.
pub fn without_password_error(e: RunError, password_sent: bool) -> RunError {
    if password_sent {
        return e;
    }
    let missing = match &e {
        RunError::Db { code, .. } => code == "28P01" || code == "28000",
        RunError::Connect { cause, .. } => cause == "Password missing.",
        _ => false,
    };
    if missing {
        other("No password saved for this connection. Edit it to add one.")
    } else {
        e
    }
}

/// Password for `databases_test`: the keychain password of the saved
/// connection is used only when host, port and user are unchanged, so it is
/// never sent to another server or account.
pub fn test_password(
    state: &DatabasesState,
    draft: &ConnectionDraft,
    given: Option<String>,
) -> Result<Option<String>, String> {
    let same_server = draft
        .id
        .as_deref()
        .and_then(|id| state.saved(id))
        .filter(|c| {
            c.host == draft.host.trim() && c.port == draft.port && c.user == draft.user.trim()
        });
    let saved = match same_server {
        Some(c) => state.secrets.get(&c.id)?,
        None => None,
    };
    Ok(resolve_password(saved, given))
}

/// Text of a `RunError` for commands that return a `String` error.
pub fn error_text(e: &RunError) -> String {
    match e {
        RunError::Connect { host, port, cause } => connect_message(host, *port, cause),
        RunError::Closed => "The connection was closed.".to_string(),
        RunError::Db { message, .. } => message.clone(),
        RunError::Cancelled => "Query cancelled.".to_string(),
        RunError::Other { message } => message.clone(),
    }
}

fn other(message: &str) -> RunError {
    RunError::Other {
        message: message.to_string(),
    }
}

/// Validates and stores the connection, then its password when one is given.
/// An absent or empty password keeps the saved one, unless host, port or
/// user changed: the saved password is then deleted, so it is never sent to
/// another server or account. The in-memory list changes only once `persist`
/// succeeded.
pub fn save_connection(
    state: &DatabasesState,
    draft: ConnectionDraft,
    password: Option<String>,
    persist: impl FnOnce(&[SavedConnection]) -> Result<(), String>,
) -> Result<SavedConnection, String> {
    let is_new = draft.id.is_none();
    let mut list = lock(&state.connections);
    let previous = draft
        .id
        .as_deref()
        .and_then(|id| list.iter().find(|c| c.id == id).cloned());
    let mut next = list.clone();
    let saved = connections::upsert(&mut next, draft)?;
    let password = password.filter(|p| !p.is_empty());
    let server_changed = previous
        .is_some_and(|p| p.host != saved.host || p.port != saved.port || p.user != saved.user);
    match &password {
        Some(p) => state.secrets.set(&saved.id, p)?,
        // Deleted before saving: if the deletion fails, nothing is saved and
        // the old password cannot reach the new server.
        None if server_changed => state.secrets.delete(&saved.id)?,
        None => {}
    }
    if let Err(e) = persist(&next) {
        if is_new && password.is_some() {
            let _ = state.secrets.delete(&saved.id);
        }
        return Err(e);
    }
    *list = next;
    Ok(saved)
}

/// Removes the connection, its keychain entry and its history.
pub fn delete_connection(
    state: &DatabasesState,
    id: &str,
    persist: impl FnOnce(&[SavedConnection]) -> Result<(), String>,
) -> Result<(), String> {
    {
        let mut list = lock(&state.connections);
        let mut next = list.clone();
        if !connections::remove(&mut next, id) {
            return Err(GONE.into());
        }
        persist(&next)?;
        *list = next;
    }
    state.secrets.delete(id)?;
    lock(&state.history).clear(id);
    state.save_history()
}

/// Saved connection: the `read_only` field, persisted. Detected connection:
/// a flag kept in memory only.
pub fn set_read_only_flag(
    state: &DatabasesState,
    id: &str,
    on: bool,
    persist: impl FnOnce(&[SavedConnection]) -> Result<(), String>,
) -> Result<(), String> {
    {
        let mut list = lock(&state.connections);
        if let Some(i) = list.iter().position(|c| c.id == id) {
            let mut next = list.clone();
            next[i].read_only = on;
            persist(&next)?;
            *list = next;
            return Ok(());
        }
    }
    if !id.starts_with("docker:") {
        return Err(GONE.into());
    }
    let mut flags = lock(&state.detected_read_only);
    if on {
        flags.insert(id.to_string());
    } else {
        flags.remove(id);
    }
    Ok(())
}

/// Result of applying the read-only flag to the open session.
#[derive(Debug)]
pub enum SessionToggle {
    NoSession,
    Applied,
    /// A transaction is open: the session kept its previous value.
    Refused(String),
    Failed(RunError),
}

impl SessionToggle {
    fn from_result(r: Result<(), RunError>) -> Self {
        match r {
            Ok(()) => Self::Applied,
            Err(RunError::Other { message }) if message == TRANSACTION_OPEN => {
                Self::Refused(message)
            }
            Err(e) => Self::Failed(e),
        }
    }
}

/// Saves the flag once the open session took it (R13). A refused toggle
/// saves nothing. Returns the command result and whether the session must
/// be dropped: after a failure, so the next run reconnects with the saved
/// flag, and when saving fails after the session took the new value.
pub fn finish_read_only(
    state: &DatabasesState,
    id: &str,
    on: bool,
    toggle: SessionToggle,
    persist: impl FnOnce(&[SavedConnection]) -> Result<(), String>,
) -> (Result<(), String>, bool) {
    match toggle {
        SessionToggle::Refused(message) => (Err(message), false),
        SessionToggle::NoSession => (set_read_only_flag(state, id, on, persist), false),
        SessionToggle::Applied => {
            let r = set_read_only_flag(state, id, on, persist);
            let forget = r.is_err();
            (r, forget)
        }
        SessionToggle::Failed(e) => {
            eprintln!(
                "LsDev databases: read-only not applied, session closed: {}",
                error_text(&e)
            );
            (set_read_only_flag(state, id, on, persist), true)
        }
    }
}

async fn close_session(state: &DatabasesState, id: &str) {
    state.sessions.lock().await.remove(id);
}

#[tauri::command]
pub async fn databases_list(
    core: CoreRef<'_>,
    state: DatabasesRef<'_>,
) -> Result<ConnectionsView, String> {
    let saved = lock(&state.connections).clone();
    let listed = match core.docker_handle() {
        Ok(docker) => DockerCollector::list(&docker).await.ok(),
        Err(_) => None,
    };
    Ok(match listed {
        Some(raws) => view(saved, detect(&raws), true),
        None => view(saved, Vec::new(), false),
    })
}

#[tauri::command]
pub async fn databases_parse_url(url: String) -> Result<ParsedUrl, String> {
    parse_url(&url)
}

#[tauri::command]
pub async fn databases_save(
    app: AppHandle,
    state: DatabasesRef<'_>,
    draft: ConnectionDraft,
    password: Option<String>,
) -> Result<SavedConnection, String> {
    let saved = save_connection(&state, draft, password, |list| {
        connections::save(&app, list, state.connections_unreadable)
    })?;
    close_session(&state, &saved.id).await;
    Ok(saved)
}

#[tauri::command]
pub async fn databases_delete(
    app: AppHandle,
    state: DatabasesRef<'_>,
    id: String,
) -> Result<(), String> {
    let result = delete_connection(&state, &id, |list| {
        connections::save(&app, list, state.connections_unreadable)
    });
    close_session(&state, &id).await;
    result
}

/// Opens a connection with the form's values, runs `select 1`, closes it.
#[tauri::command]
pub async fn databases_test(
    state: DatabasesRef<'_>,
    draft: ConnectionDraft,
    password: Option<String>,
) -> Result<(), String> {
    // Same validation as saving.
    connections::upsert(
        &mut Vec::new(),
        ConnectionDraft {
            id: None,
            ..draft.clone()
        },
    )?;
    let password = test_password(&state, &draft, password)?;
    let target = Target {
        host: draft.host.trim().to_string(),
        port: draft.port,
        user: draft.user.trim().to_string(),
        password,
        database: draft.database,
        ssl_mode: draft.ssl_mode,
    };
    let session = connect(&target, draft.read_only)
        .await
        .map_err(|e| error_text(&e))?;
    session.run("select 1").await.map_err(|e| error_text(&e))?;
    Ok(())
}

#[tauri::command]
pub async fn databases_set_read_only(
    app: AppHandle,
    state: DatabasesRef<'_>,
    id: String,
    on: bool,
) -> Result<(), String> {
    // The open session is updated and checked first; the flag is saved only
    // once it applies.
    let session = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .filter(|s| !s.is_closed());
    let toggle = match &session {
        Some(s) => SessionToggle::from_result(s.set_read_only(on).await),
        None => SessionToggle::NoSession,
    };
    let (result, forget) = finish_read_only(&state, &id, on, toggle, |list| {
        connections::save(&app, list, state.connections_unreadable)
    });
    if let (true, Some(session)) = (forget, &session) {
        forget_session(&state, &id, session).await;
    }
    result
}

/// Removes `session` from the map, unless another one replaced it meanwhile.
async fn forget_session(state: &DatabasesState, id: &str, session: &Arc<Session>) {
    let mut sessions = state.sessions.lock().await;
    if sessions.get(id).is_some_and(|s| Arc::ptr_eq(s, session)) {
        sessions.remove(id);
    }
}

/// Marks a connection busy for the duration of a run.
struct RunSlot<'a> {
    state: &'a DatabasesState,
    id: &'a str,
}

impl Drop for RunSlot<'_> {
    fn drop(&mut self) {
        lock(&self.state.running).remove(self.id);
        lock(&self.state.busy).remove(self.id);
    }
}

#[tauri::command]
pub async fn databases_run(
    core: CoreRef<'_>,
    state: DatabasesRef<'_>,
    id: String,
    sql: String,
) -> Result<RunOutput, RunError> {
    {
        let mut busy = lock(&state.busy);
        if busy.contains(&id) || lock(&state.running).contains_key(&id) {
            return Err(other("A query is already running on this connection."));
        }
        busy.insert(id.clone());
    }
    lock(&state.history).record(&id, &sql);
    if let Err(e) = state.save_history() {
        eprintln!("LsDev databases: cannot save the history: {e}");
    }
    let _slot = RunSlot {
        state: &state,
        id: &id,
    };

    let reused = state.sessions.lock().await.get(&id).cloned();
    let session = match reused.filter(|s| !s.is_closed()) {
        Some(s) => s,
        None => open_session(&core, &state, &id).await?,
    };
    match run_on(&state, &id, &session, &sql).await {
        // Tunnel cut, Postgres restarted: reconnect once and retry.
        Err(RunError::Closed) => {
            let session = open_session(&core, &state, &id).await?;
            run_on(&state, &id, &session, &sql).await
        }
        result => result,
    }
}

async fn run_on(
    state: &DatabasesState,
    id: &str,
    session: &Session,
    sql: &str,
) -> Result<RunOutput, RunError> {
    lock(&state.running).insert(id.to_string(), session.cancel_token());
    session.run(sql).await
}

/// Connects with the current read-only flag and keeps the session.
async fn open_session(
    core: &CoreState,
    state: &DatabasesState,
    id: &str,
) -> Result<Arc<Session>, RunError> {
    let (target, missing_saved_password) = resolve_target(core, state, id).await?;
    let read_only = state.read_only_for(id);
    let session = connect(&target, read_only)
        .await
        .map_err(|e| without_password_error(e, !missing_saved_password))?;
    let session = Arc::new(session);
    state
        .sessions
        .lock()
        .await
        .insert(id.to_string(), session.clone());
    // The switch may have been toggled while connecting: apply the current
    // value now that `databases_set_read_only` can see this session.
    let now = state.read_only_for(id);
    if now != read_only {
        if let Err(e) = session.set_read_only(now).await {
            forget_session(state, id, &session).await;
            return Err(e);
        }
    }
    Ok(session)
}

/// Target of a connection, and whether it is a saved connection without a
/// keychain password.
async fn resolve_target(
    core: &CoreState,
    state: &DatabasesState,
    id: &str,
) -> Result<(Target, bool), RunError> {
    if let Some(c) = state.saved(id) {
        let password = state
            .secrets
            .get(id)
            .map_err(|message| RunError::Other { message })?;
        let missing = password.is_none();
        let target = Target {
            host: c.host,
            port: c.port,
            user: c.user,
            password,
            database: c.database,
            ssl_mode: c.ssl_mode,
        };
        return Ok((target, missing));
    }
    if !id.starts_with("docker:") {
        return Err(other(GONE));
    }
    let docker = core
        .docker_handle()
        .map_err(|message| RunError::Other { message })?;
    let raws = DockerCollector::list(&docker)
        .await
        .map_err(|message| RunError::Other { message })?;
    let d = detect(&raws)
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| other("This container no longer exists."))?;
    let creds = if d.running {
        let info = docker
            .inspect_container(&d.container_id, None::<InspectContainerOptions>)
            .await
            .map_err(|e| RunError::Other {
                message: e.to_string(),
            })?;
        credentials_from_env(&info.config.and_then(|c| c.env).unwrap_or_default())
    } else {
        credentials_from_env(&[])
    };
    Ok((detected_target(&d, creds)?, false))
}

#[tauri::command]
pub async fn databases_cancel(state: DatabasesRef<'_>, id: String) -> Result<(), String> {
    let handle = lock(&state.running).remove(&id);
    if let Some(handle) = handle {
        // Armed first: through a half-open tunnel the cancel itself may
        // never get an answer.
        handle.arm_watchdog(CANCEL_GRACE);
        let _ = tokio::time::timeout(CANCEL_GRACE, handle.cancel()).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn databases_history(state: DatabasesRef<'_>, id: String) -> Result<Vec<String>, String> {
    Ok(lock(&state.history).entries(&id))
}

#[tauri::command]
pub async fn databases_clear_history(state: DatabasesRef<'_>, id: String) -> Result<(), String> {
    lock(&state.history).clear(&id);
    state.save_history()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::databases::history::History;
    use crate::tools::databases::secrets::MemoryStore;
    use crate::tools::databases::state::lock;
    use crate::tools::databases::url::SslMode;
    use std::cell::RefCell;

    fn draft(name: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: None,
            name: name.into(),
            env: Some("local".into()),
            color: None,
            host: "localhost".into(),
            port: 5432,
            user: "app".into(),
            database: "app".into(),
            ssl_mode: SslMode::Prefer,
            read_only: false,
        }
    }

    fn detected(running: bool, port: Option<u16>) -> DetectedConnection {
        DetectedConnection {
            id: "docker:pg".into(),
            container_id: "abc".into(),
            name: "pg".into(),
            project: None,
            port,
            running,
        }
    }

    fn creds() -> ContainerCredentials {
        ContainerCredentials {
            user: "u".into(),
            password: Some("secret".into()),
            database: "d".into(),
        }
    }

    fn state(dir: &tempfile::TempDir) -> DatabasesState {
        DatabasesState::new(
            Vec::new(),
            Box::new(MemoryStore::default()),
            History::default(),
            dir.path().join("data").join("databases-history.json"),
        )
    }

    fn ok(_: &[SavedConnection]) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn resolve_password_given_non_empty_wins() {
        assert_eq!(
            resolve_password(Some("saved".into()), Some("given".into())).as_deref(),
            Some("given")
        );
        assert_eq!(
            resolve_password(Some("saved".into()), Some(String::new())).as_deref(),
            Some("saved")
        );
        assert_eq!(
            resolve_password(Some("saved".into()), None).as_deref(),
            Some("saved")
        );
        assert_eq!(resolve_password(None, None), None);
    }

    #[test]
    fn detected_target_requires_running_container() {
        let e = detected_target(&detected(false, Some(5432)), creds()).unwrap_err();
        assert_eq!(
            e,
            RunError::Other {
                message: "Start the container first.".into()
            }
        );
    }

    #[test]
    fn detected_target_requires_published_port() {
        let e = detected_target(&detected(true, None), creds()).unwrap_err();
        assert_eq!(
            e,
            RunError::Other {
                message: "This container does not publish port 5432.".into()
            }
        );
    }

    #[test]
    fn detected_target_uses_loopback_and_credentials() {
        let t = detected_target(&detected(true, Some(55432)), creds()).unwrap();
        assert_eq!(t.host, "127.0.0.1");
        assert_eq!(t.port, 55432);
        assert_eq!(t.user, "u");
        assert_eq!(t.password.as_deref(), Some("secret"));
        assert_eq!(t.database, "d");
    }

    #[test]
    fn view_json_has_no_password_key() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let saved = save_connection(&s, draft("A"), Some("pw".into()), ok).unwrap();
        let json = serde_json::to_value(view(vec![saved], vec![detected(true, Some(5432))], true))
            .unwrap();
        fn has_password(v: &serde_json::Value) -> bool {
            match v {
                serde_json::Value::Object(m) => m
                    .iter()
                    .any(|(k, v)| k.to_lowercase().contains("password") || has_password(v)),
                serde_json::Value::Array(a) => a.iter().any(has_password),
                _ => false,
            }
        }
        assert!(!has_password(&json));
        assert!(!json.to_string().contains("pw\""));
        assert_eq!(json["dockerAvailable"], true);
        assert_eq!(json["saved"][0]["name"], "A");
        assert_eq!(json["detected"][0]["id"], "docker:pg");
    }

    #[test]
    fn save_stores_password_and_keeps_it_on_edit_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let persisted = RefCell::new(Vec::new());
        let persist = |list: &[SavedConnection]| {
            *persisted.borrow_mut() = list.to_vec();
            Ok(())
        };
        let a = save_connection(&s, draft("A"), Some("pw".into()), persist).unwrap();
        assert_eq!(s.secrets.get(&a.id), Ok(Some("pw".into())));
        assert_eq!(persisted.borrow().len(), 1);

        let mut edit = draft("A2");
        edit.id = Some(a.id.clone());
        let persist = |list: &[SavedConnection]| {
            *persisted.borrow_mut() = list.to_vec();
            Ok(())
        };
        save_connection(&s, edit, None, persist).unwrap();
        assert_eq!(s.secrets.get(&a.id), Ok(Some("pw".into())));
        assert_eq!(persisted.borrow()[0].name, "A2");
        assert_eq!(lock(&s.connections)[0].name, "A2");

        let mut edit = draft("A3");
        edit.id = Some(a.id.clone());
        save_connection(&s, edit, Some(String::new()), ok).unwrap();
        assert_eq!(s.secrets.get(&a.id), Ok(Some("pw".into())));
    }

    #[test]
    fn save_with_new_server_and_no_password_drops_the_saved_one() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), Some("pw".into()), ok).unwrap();
        let edited = |f: &dyn Fn(&mut ConnectionDraft)| {
            let mut d = draft("A");
            d.id = Some(a.id.clone());
            f(&mut d);
            d
        };
        let changes: [&dyn Fn(&mut ConnectionDraft); 3] = [
            &|d| d.host = "db.example.com".into(),
            &|d| d.port = 6543,
            &|d| d.user = "admin".into(),
        ];
        for change in changes {
            s.secrets.set(&a.id, "pw").unwrap();
            save_connection(&s, edited(change), None, ok).unwrap();
            assert_eq!(s.secrets.get(&a.id), Ok(None));
            // Back to the original server, an empty password keeps none.
            save_connection(&s, edited(&|_| {}), Some(String::new()), ok).unwrap();
            s.secrets.set(&a.id, "pw").unwrap();
            save_connection(&s, edited(change), Some(String::new()), ok).unwrap();
            assert_eq!(s.secrets.get(&a.id), Ok(None));
            save_connection(&s, edited(&|_| {}), None, ok).unwrap();
        }
        // A new password for the new server is stored.
        save_connection(&s, edited(changes[0]), Some("new".into()), ok).unwrap();
        assert_eq!(s.secrets.get(&a.id), Ok(Some("new".into())));
        // Same server with spaces, other fields changed: the password stays.
        let same = edited(&|d| {
            d.host = " db.example.com ".into();
            d.name = "Renamed".into();
            d.database = "other".into();
        });
        save_connection(&s, same, None, ok).unwrap();
        assert_eq!(s.secrets.get(&a.id), Ok(Some("new".into())));
        // The password is deleted before saving: a failed save leaves the
        // connection unchanged, without its password.
        let failed = save_connection(&s, edited(changes[2]), None, |_| Err("disk full".into()));
        assert_eq!(failed, Err("disk full".into()));
        assert_eq!(s.secrets.get(&a.id), Ok(None));
        assert_eq!(lock(&s.connections)[0].host, "db.example.com");
    }

    #[test]
    fn save_without_password_stores_none() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), None, ok).unwrap();
        assert_eq!(s.secrets.get(&a.id), Ok(None));
    }

    #[test]
    fn save_invalid_or_unpersisted_leaves_state_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        assert_eq!(
            save_connection(&s, draft(" "), Some("pw".into()), ok),
            Err("Name is required.".into())
        );
        let failed = save_connection(&s, draft("A"), None, |_| Err("disk full".into()));
        assert_eq!(failed, Err("disk full".into()));
        assert!(lock(&s.connections).is_empty());
    }

    #[test]
    fn delete_removes_connection_password_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), Some("pw".into()), ok).unwrap();
        lock(&s.history).record(&a.id, "select 1");
        lock(&s.history).record("other", "select 2");
        delete_connection(&s, &a.id, ok).unwrap();
        assert!(lock(&s.connections).is_empty());
        assert_eq!(s.secrets.get(&a.id), Ok(None));
        assert!(lock(&s.history).entries(&a.id).is_empty());
        // The history file is written, its directory created.
        let on_disk = crate::tools::databases::history::load(&s.history_path);
        assert!(on_disk.entries(&a.id).is_empty());
        assert_eq!(on_disk.entries("other"), vec!["select 2"]);
    }

    #[test]
    fn delete_unknown_fails() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        assert_eq!(
            delete_connection(&s, "ghost", ok),
            Err("This connection no longer exists.".into())
        );
    }

    #[test]
    fn read_only_saved_is_persisted_detected_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), None, ok).unwrap();
        let persisted = RefCell::new(Vec::new());
        set_read_only_flag(&s, &a.id, true, |list| {
            *persisted.borrow_mut() = list.to_vec();
            Ok(())
        })
        .unwrap();
        assert!(persisted.borrow()[0].read_only);
        assert!(s.read_only_for(&a.id));

        assert!(!s.read_only_for("docker:pg"));
        set_read_only_flag(&s, "docker:pg", true, |_| panic!("not persisted")).unwrap();
        assert!(s.read_only_for("docker:pg"));
        set_read_only_flag(&s, "docker:pg", false, |_| panic!("not persisted")).unwrap();
        assert!(!s.read_only_for("docker:pg"));

        assert_eq!(
            set_read_only_flag(&s, "ghost", true, ok),
            Err("This connection no longer exists.".into())
        );
    }

    #[test]
    fn read_only_toggle_saved_only_once_the_session_took_it() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), None, ok).unwrap();

        // Refused: nothing saved, session kept.
        let (r, forget) = finish_read_only(
            &s,
            &a.id,
            true,
            SessionToggle::from_result(Err(other(TRANSACTION_OPEN))),
            |_| panic!("not persisted"),
        );
        assert_eq!(r, Err(TRANSACTION_OPEN.into()));
        assert!(!forget);
        assert!(!s.read_only_for(&a.id));

        // No session, or applied: saved, session kept.
        for toggle in [SessionToggle::NoSession, SessionToggle::from_result(Ok(()))] {
            let on = !s.read_only_for(&a.id);
            let (r, forget) = finish_read_only(&s, &a.id, on, toggle, ok);
            assert_eq!(r, Ok(()));
            assert!(!forget);
            assert_eq!(s.read_only_for(&a.id), on);
        }

        // Applied but not saved: error, session dropped so it reconnects
        // with the saved flag.
        let before = s.read_only_for(&a.id);
        let (r, forget) = finish_read_only(&s, &a.id, !before, SessionToggle::Applied, |_| {
            Err("disk full".into())
        });
        assert_eq!(r, Err("disk full".into()));
        assert!(forget);
        assert_eq!(s.read_only_for(&a.id), before);

        // Session failure: saved, session dropped.
        let (r, forget) = finish_read_only(
            &s,
            &a.id,
            !before,
            SessionToggle::from_result(Err(RunError::Closed)),
            ok,
        );
        assert_eq!(r, Ok(()));
        assert!(forget);
        assert_eq!(s.read_only_for(&a.id), !before);
    }

    /// Runs only when LSDEV_TEST_PG_URL is set.
    #[tokio::test]
    async fn read_only_toggle_in_open_transaction_keeps_the_flag() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let p = parse_url(&url).unwrap();
        let target = Target {
            host: p.host,
            port: p.port,
            user: p.user,
            password: p.password,
            database: p.database,
            ssl_mode: p.ssl_mode,
        };
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), None, ok).unwrap();
        let session = connect(&target, false).await.unwrap();
        session.run("begin; select 1").await.unwrap();
        let toggle = SessionToggle::from_result(session.set_read_only(true).await);
        let (r, forget) = finish_read_only(&s, &a.id, true, toggle, |_| panic!("not persisted"));
        assert_eq!(r, Err(TRANSACTION_OPEN.into()));
        assert!(!forget);
        assert!(!s.read_only_for(&a.id));
        session.run("rollback").await.unwrap();
    }

    #[test]
    fn missing_password_is_explained_only_when_none_was_sent() {
        let auth = |code: &str| RunError::Db {
            message: "password authentication failed".into(),
            code: code.into(),
            hint: None,
            position: None,
        };
        let expected = RunError::Other {
            message: "No password saved for this connection. Edit it to add one.".into(),
        };
        assert_eq!(without_password_error(auth("28P01"), false), expected);
        assert_eq!(without_password_error(auth("28000"), false), expected);
        assert_eq!(without_password_error(auth("28P01"), true), auth("28P01"));
        assert_eq!(without_password_error(auth("42601"), false), auth("42601"));
        assert_eq!(
            without_password_error(RunError::Closed, false),
            RunError::Closed
        );
    }

    const NO_PASSWORD: &str = "No password saved for this connection. Edit it to add one.";

    /// A local fake server that asks for a cleartext password: tokio-postgres
    /// then fails client-side with "password missing", the real shape of a
    /// saved connection without a keychain password.
    #[tokio::test]
    async fn password_requested_but_none_sent_is_explained() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            // AuthenticationCleartextPassword
            sock.write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 3])
                .await
                .unwrap();
            let _ = sock.read(&mut buf).await;
        });
        let target = Target {
            host: "127.0.0.1".into(),
            port,
            user: "app".into(),
            password: None,
            database: "app".into(),
            ssl_mode: SslMode::Disable,
        };
        let e = connect(&target, false).await.err().unwrap();
        assert_eq!(
            e,
            RunError::Connect {
                host: "127.0.0.1".into(),
                port,
                cause: "Password missing.".into()
            }
        );
        assert_eq!(without_password_error(e.clone(), false), other(NO_PASSWORD));
        assert_eq!(without_password_error(e.clone(), true), e);
        let refused = RunError::Connect {
            host: "127.0.0.1".into(),
            port,
            cause: "Connection refused.".into(),
        };
        assert_eq!(without_password_error(refused.clone(), false), refused);
    }

    #[test]
    fn test_password_uses_keychain_only_for_the_same_server_and_user() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(&dir);
        let a = save_connection(&s, draft("A"), Some("pw".into()), ok).unwrap();
        let same = || {
            let mut d = draft("A");
            d.id = Some(a.id.clone());
            d
        };
        assert_eq!(test_password(&s, &same(), None), Ok(Some("pw".into())));
        assert_eq!(
            test_password(&s, &same(), Some(String::new())),
            Ok(Some("pw".into()))
        );
        assert_eq!(
            test_password(&s, &same(), Some("typed".into())),
            Ok(Some("typed".into()))
        );
        let mut host = same();
        host.host = "db.example.com".into();
        let mut port = same();
        port.port = 6543;
        let mut user = same();
        user.user = "admin".into();
        for changed in [host, port, user] {
            assert_eq!(test_password(&s, &changed, None), Ok(None));
            assert_eq!(test_password(&s, &changed, Some(String::new())), Ok(None));
            assert_eq!(
                test_password(&s, &changed, Some("typed".into())),
                Ok(Some("typed".into()))
            );
        }
        // Whitespace around host and user is ignored, as when saving.
        let mut spaced = same();
        spaced.host = " localhost ".into();
        spaced.user = " app ".into();
        assert_eq!(test_password(&s, &spaced, None), Ok(Some("pw".into())));
        assert_eq!(test_password(&s, &draft("A"), None), Ok(None));
    }

    #[test]
    fn error_text_for_each_kind() {
        assert_eq!(
            error_text(&RunError::Connect {
                host: "127.0.0.1".into(),
                port: 6543,
                cause: "Connection refused.".into()
            }),
            "Could not connect to 127.0.0.1:6543. Connection refused. Is the tunnel running?"
        );
        assert_eq!(
            error_text(&RunError::Db {
                message: "syntax error".into(),
                code: "42601".into(),
                hint: None,
                position: Some(3)
            }),
            "syntax error"
        );
        assert_eq!(error_text(&RunError::Cancelled), "Query cancelled.");
        assert_eq!(error_text(&RunError::Closed), "The connection was closed.");
        assert_eq!(
            error_text(&RunError::Other {
                message: "x".into()
            }),
            "x"
        );
    }
}
