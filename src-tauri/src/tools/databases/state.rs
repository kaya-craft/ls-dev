//! State of the Databases tool: saved connections, open sessions, running
//! queries and history.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::connections::SavedConnection;
use super::history::{self, History};
use super::secrets::SecretStore;
use super::session::{CancelHandle, Session};

pub type DatabasesRef<'a> = tauri::State<'a, Arc<DatabasesState>>;

pub struct DatabasesState {
    pub connections: Mutex<Vec<SavedConnection>>,
    pub secrets: Box<dyn SecretStore>,
    /// One session per connection id, kept while the app runs.
    pub sessions: tokio::sync::Mutex<HashMap<String, Arc<Session>>>,
    /// Cancel handle of the query running on a connection.
    pub running: Mutex<HashMap<String, CancelHandle>>,
    pub history: Mutex<History>,
    pub history_path: PathBuf,
    /// Read-only flag of detected connections, in memory only.
    pub detected_read_only: Mutex<HashSet<String>>,
    /// Connections with a run in progress, from its acceptance (before the
    /// session is opened) to its end; `running` only knows the query once it
    /// is sent.
    pub busy: Mutex<HashSet<String>>,
    /// `databases.json` could not be read at startup: saving is refused.
    pub connections_unreadable: bool,
}

impl DatabasesState {
    pub fn new(
        connections: Vec<SavedConnection>,
        secrets: Box<dyn SecretStore>,
        history: History,
        history_path: PathBuf,
    ) -> Self {
        Self {
            connections: Mutex::new(connections),
            secrets,
            sessions: tokio::sync::Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            history: Mutex::new(history),
            history_path,
            detected_read_only: Mutex::new(HashSet::new()),
            busy: Mutex::new(HashSet::new()),
            connections_unreadable: false,
        }
    }

    pub fn saved(&self, id: &str) -> Option<SavedConnection> {
        lock(&self.connections).iter().find(|c| c.id == id).cloned()
    }

    /// Saved connection: its `read_only` field; detected: the in-memory flag.
    pub fn read_only_for(&self, id: &str) -> bool {
        match self.saved(id) {
            Some(c) => c.read_only,
            None => lock(&self.detected_read_only).contains(id),
        }
    }

    /// Writes the history file, creating the app data directory if needed.
    pub fn save_history(&self) -> Result<(), String> {
        if !self.history_path.is_absolute() {
            return Err("The app data directory is unknown; the history is not saved.".into());
        }
        if let Some(dir) = self.history_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let history = lock(&self.history);
        history::save(&self.history_path, &history)
    }
}

/// Locks a std mutex, recovering the data of a poisoned one.
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
