//! Per-connection query history, persisted in `databases-history.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 50;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct History(HashMap<String, Vec<String>>);

impl History {
    pub fn record(&mut self, id: &str, sql: &str) {
        if sql.trim().is_empty() {
            return;
        }
        let list = self.0.entry(id.to_string()).or_default();
        if list.first().map(String::as_str) == Some(sql) {
            return;
        }
        list.insert(0, sql.to_string());
        list.truncate(MAX_ENTRIES);
    }

    pub fn entries(&self, id: &str) -> Vec<String> {
        self.0.get(id).cloned().unwrap_or_default()
    }

    pub fn clear(&mut self, id: &str) {
        self.0.remove(id);
    }
}

pub fn load(path: &Path) -> History {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, h: &History) -> Result<(), String> {
    let json = serde_json::to_string(h).map_err(|e| e.to_string())?;
    let mut tmp: PathBuf = path.as_os_str().into();
    tmp.as_mut_os_string().push(".tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_most_recent_first() {
        let mut h = History::default();
        h.record("c", "select 1");
        h.record("c", "select 2");
        assert_eq!(h.entries("c"), vec!["select 2", "select 1"]);
    }

    #[test]
    fn skips_consecutive_duplicate_keeps_non_consecutive() {
        let mut h = History::default();
        h.record("c", "a");
        h.record("c", "a");
        h.record("c", "b");
        h.record("c", "a");
        assert_eq!(h.entries("c"), vec!["a", "b", "a"]);
    }

    #[test]
    fn ignores_blank() {
        let mut h = History::default();
        h.record("c", "  \n\t ");
        assert!(h.entries("c").is_empty());
    }

    #[test]
    fn keeps_fifty_entries() {
        let mut h = History::default();
        for i in 0..51 {
            h.record("c", &format!("q{i}"));
        }
        let e = h.entries("c");
        assert_eq!(e.len(), MAX_ENTRIES);
        assert_eq!(e[0], "q50");
        assert_eq!(e[49], "q1");
    }

    #[test]
    fn clear_only_affects_one_connection() {
        let mut h = History::default();
        h.record("c", "a");
        h.record("d", "b");
        h.clear("c");
        assert!(h.entries("c").is_empty());
        assert_eq!(h.entries("d"), vec!["b"]);
    }

    #[test]
    fn save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("databases-history.json");
        let mut h = History::default();
        h.record("c", "select 1");
        save(&path, &h).unwrap();
        assert!(!dir.path().join("databases-history.json.tmp").exists());
        assert_eq!(load(&path).entries("c"), vec!["select 1"]);
    }

    #[test]
    fn missing_or_invalid_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("none.json")).entries("c").is_empty());
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        assert!(load(&bad).entries("c").is_empty());
    }
}
