//! Stable tunnel URLs remembered per project directory (`tunnels.json`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "tunnels.json";
const STORE_KEY: &str = "stable";
pub const UNREADABLE: &str =
    "tunnels.json could not be read; fix or remove it before creating stable URLs.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StableUrl {
    pub hostname: String,
    pub tunnel_name: String,
    pub tunnel_id: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StableStore {
    /// Project directory -> stable URL.
    pub entries: BTreeMap<String, StableUrl>,
    /// The file exists but does not decode: it is shown as empty and must not
    /// be overwritten, or every remembered URL would be lost.
    pub unreadable: bool,
}

impl StableStore {
    pub fn get(&self, cwd: &str) -> Option<&StableUrl> {
        self.entries.get(cwd)
    }

    pub fn put(&mut self, cwd: &str, entry: StableUrl) {
        self.entries.insert(cwd.to_string(), entry);
    }

    pub fn remove(&mut self, cwd: &str) -> Option<StableUrl> {
        self.entries.remove(cwd)
    }
}

pub fn decode(value: Option<serde_json::Value>) -> StableStore {
    let Some(value) = value else {
        return StableStore::default();
    };
    match serde_json::from_value(value) {
        Ok(entries) => StableStore {
            entries,
            unreadable: false,
        },
        Err(e) => {
            eprintln!("LsDev tunnels: stored stable URLs are invalid, using none: {e}");
            StableStore {
                entries: BTreeMap::new(),
                unreadable: true,
            }
        }
    }
}

/// Stored stable URLs; an undecodable value or store loads as empty and is
/// remembered as unreadable.
pub fn load(app: &AppHandle) -> StableStore {
    match app.store(STORE_FILE) {
        Ok(store) => decode(store.get(STORE_KEY)),
        Err(e) => {
            eprintln!("LsDev tunnels: cannot open the store: {e}");
            StableStore {
                entries: BTreeMap::new(),
                unreadable: true,
            }
        }
    }
}

/// Writes the entries, unless the file could not be read at load time.
pub fn save(app: &AppHandle, store: &StableStore) -> Result<(), String> {
    if store.unreadable {
        return Err(UNREADABLE.into());
    }
    let file = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    file.set(
        STORE_KEY,
        serde_json::to_value(&store.entries).map_err(|e| e.to_string())?,
    );
    file.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry() -> StableUrl {
        StableUrl {
            hostname: "shop.dev.example.com".into(),
            tunnel_name: "lsdev-shop".into(),
            tunnel_id: "6ff42ae2-765d-4adf-8112-31c55c1551ef".into(),
        }
    }

    #[test]
    fn decode_valid_value() {
        let s = decode(Some(json!({"/p": {
            "hostname": "h", "tunnelName": "n", "tunnelId": "i"
        }})));
        assert!(!s.unreadable);
        assert_eq!(s.get("/p").unwrap().tunnel_id, "i");
    }

    #[test]
    fn decode_missing_is_empty_and_readable() {
        let s = decode(None);
        assert!(s.entries.is_empty());
        assert!(!s.unreadable);
    }

    #[test]
    fn decode_invalid_is_unreadable() {
        let s = decode(Some(json!(["not", "a", "map"])));
        assert!(s.entries.is_empty());
        assert!(s.unreadable);
        let s = decode(Some(json!({"/p": {"hostname": 3}})));
        assert!(s.unreadable);
    }

    #[test]
    fn put_get_remove() {
        let mut s = StableStore::default();
        assert!(s.get("/p").is_none());
        s.put("/p", entry());
        assert_eq!(s.get("/p"), Some(&entry()));
        assert_eq!(s.remove("/p"), Some(entry()));
        assert!(s.remove("/p").is_none());
        assert!(s.get("/p").is_none());
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(entry()).unwrap();
        assert_eq!(v["tunnelName"], "lsdev-shop");
        assert_eq!(v["tunnelId"], "6ff42ae2-765d-4adf-8112-31c55c1551ef");
        assert_eq!(v["hostname"], "shop.dev.example.com");
    }
}
