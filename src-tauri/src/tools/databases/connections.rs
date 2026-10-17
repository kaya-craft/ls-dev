//! Saved connections (no password), Docker Postgres detection.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use super::url::SslMode;
use crate::core::docker::RawContainer;

const STORE_FILE: &str = "databases.json";
const STORE_KEY: &str = "connections";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedConnection {
    pub id: String,
    pub name: String,
    pub env: Option<String>,
    pub color: Option<String>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    pub ssl_mode: SslMode,
    pub read_only: bool,
}

/// What the connection dialog sends; `id` None means a new connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDraft {
    pub id: Option<String>,
    pub name: String,
    pub env: Option<String>,
    pub color: Option<String>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    pub ssl_mode: SslMode,
    pub read_only: bool,
}

/// Default of the read-only switch for a new connection. The connection form
/// applies this rule; the backend only stores the value it receives.
#[cfg(test)]
pub fn default_read_only(env: Option<&str>) -> bool {
    matches!(
        env.map(|e| e.trim().to_lowercase()).as_deref(),
        Some("staging" | "production")
    )
}

pub fn upsert(
    list: &mut Vec<SavedConnection>,
    draft: ConnectionDraft,
) -> Result<SavedConnection, String> {
    if draft.name.trim().is_empty() {
        return Err("Name is required.".into());
    }
    if draft.host.trim().is_empty() {
        return Err("Host is required.".into());
    }
    if draft.user.trim().is_empty() {
        return Err("User is required.".into());
    }
    if draft.port == 0 {
        return Err("Port must be between 1 and 65535.".into());
    }
    let position = match &draft.id {
        Some(id) => Some(
            list.iter()
                .position(|c| &c.id == id)
                .ok_or("This connection no longer exists.")?,
        ),
        None => None,
    };
    let saved = SavedConnection {
        id: draft.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: draft.name.trim().to_string(),
        env: draft.env,
        color: draft.color,
        host: draft.host.trim().to_string(),
        port: draft.port,
        user: draft.user.trim().to_string(),
        database: draft.database,
        ssl_mode: draft.ssl_mode,
        read_only: draft.read_only,
    };
    match position {
        Some(i) => list[i] = saved.clone(),
        None => list.push(saved.clone()),
    }
    Ok(saved)
}

pub fn remove(list: &mut Vec<SavedConnection>, id: &str) -> bool {
    let before = list.len();
    list.retain(|c| c.id != id);
    list.len() != before
}

const UNREADABLE: &str =
    "databases.json could not be read; fix or remove it before saving connections.";

/// Stored connections, and whether the stored value could not be read.
pub struct Loaded {
    pub list: Vec<SavedConnection>,
    /// The file exists but does not decode: it is shown as empty and must not
    /// be overwritten, or every saved connection would be lost.
    pub unreadable: bool,
}

pub fn decode(value: Option<serde_json::Value>) -> Loaded {
    let Some(value) = value else {
        return Loaded {
            list: Vec::new(),
            unreadable: false,
        };
    };
    match serde_json::from_value(value) {
        Ok(list) => Loaded {
            list,
            unreadable: false,
        },
        Err(e) => {
            eprintln!("LsDev databases: stored connections are invalid, using none: {e}");
            Loaded {
                list: Vec::new(),
                unreadable: true,
            }
        }
    }
}

/// Stored connections; an undecodable value or store loads as empty and is
/// remembered as unreadable.
pub fn load(app: &AppHandle) -> Loaded {
    match app.store(STORE_FILE) {
        Ok(store) => decode(store.get(STORE_KEY)),
        Err(e) => {
            eprintln!("LsDev databases: cannot open the store: {e}");
            Loaded {
                list: Vec::new(),
                unreadable: true,
            }
        }
    }
}

/// Refuses to overwrite a file that could not be read at startup.
pub fn writable(unreadable: bool) -> Result<(), String> {
    if unreadable {
        Err(UNREADABLE.into())
    } else {
        Ok(())
    }
}

/// Writes the list, unless the file could not be read at startup.
pub fn save(app: &AppHandle, list: &[SavedConnection], unreadable: bool) -> Result<(), String> {
    writable(unreadable)?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY,
        serde_json::to_value(list).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedConnection {
    pub id: String,
    pub container_id: String,
    pub name: String,
    pub project: Option<String>,
    pub port: Option<u16>,
    pub running: bool,
}

pub fn detect(containers: &[RawContainer]) -> Vec<DetectedConnection> {
    let mut out: Vec<DetectedConnection> = containers
        .iter()
        .filter(|c| {
            let image = c.image.to_lowercase();
            ["postgres", "pgvector", "postgis", "timescale"]
                .iter()
                .any(|k| image.contains(k))
        })
        .map(|c| DetectedConnection {
            id: format!("docker:{}", c.name),
            container_id: c.id.clone(),
            name: c.name.clone(),
            project: c.labels.get("com.docker.compose.project").cloned(),
            port: c
                .ports
                .iter()
                .find(|p| p.tcp && p.private_port == 5432)
                .and_then(|p| p.public_port),
            running: c.state == "running",
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContainerCredentials {
    pub user: String,
    pub password: Option<String>,
    pub database: String,
}

pub fn credentials_from_env(env: &[String]) -> ContainerCredentials {
    let get = |key: &str| {
        env.iter()
            .find_map(|e| e.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
            .map(str::to_string)
    };
    // As in the official image, an empty user or database means the default.
    let non_empty = |key: &str| get(key).filter(|v| !v.is_empty());
    let user = non_empty("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
    ContainerCredentials {
        password: get("POSTGRES_PASSWORD"),
        database: non_empty("POSTGRES_DB").unwrap_or_else(|| user.clone()),
        user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::docker::{RawContainer, RawPort};
    use std::collections::HashMap;

    fn draft(name: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: None,
            name: name.into(),
            env: None,
            color: None,
            host: "localhost".into(),
            port: 5432,
            user: "app".into(),
            database: "app".into(),
            ssl_mode: SslMode::Prefer,
            read_only: false,
        }
    }

    #[test]
    fn default_read_only_for_staging_and_production() {
        assert!(default_read_only(Some("staging")));
        assert!(default_read_only(Some(" Production ")));
        assert!(!default_read_only(Some("local")));
        assert!(!default_read_only(None));
    }

    #[test]
    fn upsert_appends_new_with_uuid() {
        let mut list = Vec::new();
        let a = upsert(&mut list, draft("A")).unwrap();
        let b = upsert(&mut list, draft("B")).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(a.id.len(), 36);
        assert_ne!(a.id, b.id);
        assert_eq!(list[1], b);
    }

    #[test]
    fn upsert_edit_keeps_position_and_id() {
        let mut list = Vec::new();
        let a = upsert(&mut list, draft("A")).unwrap();
        upsert(&mut list, draft("B")).unwrap();
        let mut d = draft("A2");
        d.id = Some(a.id.clone());
        let edited = upsert(&mut list, d).unwrap();
        assert_eq!(edited.id, a.id);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "A2");
    }

    #[test]
    fn upsert_validates() {
        let mut list = Vec::new();
        assert_eq!(
            upsert(&mut list, draft("  ")),
            Err("Name is required.".into())
        );
        let mut d = draft("A");
        d.host = " ".into();
        assert_eq!(upsert(&mut list, d), Err("Host is required.".into()));
        let mut d = draft("A");
        d.user = "".into();
        assert_eq!(upsert(&mut list, d), Err("User is required.".into()));
        let mut d = draft("A");
        d.port = 0;
        assert!(upsert(&mut list, d).is_err());
        assert!(list.is_empty());
    }

    #[test]
    fn upsert_unknown_id_fails() {
        let mut list = Vec::new();
        let mut d = draft("A");
        d.id = Some("ghost".into());
        assert_eq!(
            upsert(&mut list, d),
            Err("This connection no longer exists.".into())
        );
    }

    #[test]
    fn decode_absent_valid_and_invalid_values() {
        let absent = decode(None);
        assert!(absent.list.is_empty());
        assert!(!absent.unreadable);

        let mut list = Vec::new();
        upsert(&mut list, draft("A")).unwrap();
        let valid = decode(Some(serde_json::to_value(&list).unwrap()));
        assert_eq!(valid.list, list);
        assert!(!valid.unreadable);

        // One undecodable entry: shown as empty, remembered as unreadable.
        let mut value = serde_json::to_value(&list).unwrap();
        value[0]["port"] = serde_json::json!("not a port");
        let invalid = decode(Some(value));
        assert!(invalid.list.is_empty());
        assert!(invalid.unreadable);
    }

    #[test]
    fn writable_refuses_an_unreadable_file() {
        assert_eq!(writable(false), Ok(()));
        assert_eq!(
            writable(true),
            Err(
                "databases.json could not be read; fix or remove it before saving connections."
                    .into()
            )
        );
    }

    #[test]
    fn remove_by_id() {
        let mut list = Vec::new();
        let a = upsert(&mut list, draft("A")).unwrap();
        assert!(remove(&mut list, &a.id));
        assert!(!remove(&mut list, &a.id));
    }

    #[test]
    fn saved_connection_has_no_password_key() {
        let mut list = Vec::new();
        let a = upsert(&mut list, draft("A")).unwrap();
        let json = serde_json::to_string(&a).unwrap();
        assert!(!json.to_lowercase().contains("password"));
        assert!(json.contains("sslMode"));
    }

    fn container(name: &str, image: &str, state: &str, ports: Vec<RawPort>) -> RawContainer {
        RawContainer {
            id: format!("id-{name}"),
            name: name.into(),
            image: image.into(),
            state: state.into(),
            ports,
            ..Default::default()
        }
    }

    #[test]
    fn detect_postgres_containers() {
        let mut vec_c = container(
            "vec",
            "pgvector/pgvector:pg17",
            "running",
            vec![RawPort {
                private_port: 5432,
                public_port: Some(5432),
                tcp: true,
            }],
        );
        vec_c.labels =
            HashMap::from([("com.docker.compose.project".to_string(), "shop".to_string())]);
        let list = detect(&[
            container("zeta", "timescale/timescaledb:latest", "running", vec![]),
            container("pg", "postgres:16", "exited", vec![]),
            container("cache", "redis:7", "running", vec![]),
            vec_c,
        ]);
        let names: Vec<_> = list.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["pg", "vec", "zeta"]);
        assert_eq!(list[0].port, None);
        assert!(!list[0].running);
        assert_eq!(list[0].id, "docker:pg");
        assert_eq!(list[0].container_id, "id-pg");
        assert_eq!(list[1].port, Some(5432));
        assert_eq!(list[1].project.as_deref(), Some("shop"));
        assert!(list[1].running);
    }

    #[test]
    fn credentials_from_env_reads_all() {
        let c = credentials_from_env(&[
            "POSTGRES_USER=u".into(),
            "POSTGRES_PASSWORD=p".into(),
            "POSTGRES_DB=d".into(),
            "PATH=/bin".into(),
        ]);
        assert_eq!(c.user, "u");
        assert_eq!(c.password.as_deref(), Some("p"));
        assert_eq!(c.database, "d");
    }

    #[test]
    fn credentials_from_env_defaults() {
        let c = credentials_from_env(&[]);
        assert_eq!(c.user, "postgres");
        assert_eq!(c.password, None);
        assert_eq!(c.database, "postgres");
        let c = credentials_from_env(&["POSTGRES_USER=app".into()]);
        assert_eq!(c.database, "app");
    }

    #[test]
    fn credentials_empty_user_or_db_fall_back_to_defaults() {
        let c = credentials_from_env(&["POSTGRES_USER=".into(), "POSTGRES_DB=".into()]);
        assert_eq!(c.user, "postgres");
        assert_eq!(c.database, "postgres");
        let c = credentials_from_env(&["POSTGRES_USER=app".into(), "POSTGRES_DB=".into()]);
        assert_eq!(c.database, "app");
    }

    #[test]
    fn credentials_value_with_equals_kept_whole() {
        let c = credentials_from_env(&["POSTGRES_PASSWORD=a=b=c".into()]);
        assert_eq!(c.password.as_deref(), Some("a=b=c"));
    }
}
