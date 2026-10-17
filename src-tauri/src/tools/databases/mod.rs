//! Databases tool: Postgres connections, queries and history.

pub mod commands;
pub mod connections;
pub mod history;
pub mod results;
pub mod secrets;
pub mod session;
pub mod state;
pub mod url;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::core::state::CoreState;
use secrets::KeychainStore;
use state::DatabasesState;

const HISTORY_FILE: &str = "databases-history.json";

/// Registers the Databases state: saved connections, keychain, history.
/// Sessions open on the first run.
pub fn setup(app: &AppHandle, _core: Arc<CoreState>) -> Arc<DatabasesState> {
    let history_path = match app.path().app_data_dir() {
        Ok(dir) => dir.join(HISTORY_FILE),
        Err(e) => {
            // A relative path is refused by `save_history`: nothing is written.
            eprintln!("LsDev databases: no app data directory, history is not saved: {e}");
            PathBuf::new()
        }
    };
    let history = if history_path.is_absolute() {
        history::load(&history_path)
    } else {
        Default::default()
    };
    let loaded = connections::load(app);
    let mut state =
        DatabasesState::new(loaded.list, Box::new(KeychainStore), history, history_path);
    state.connections_unreadable = loaded.unreadable;
    let state = Arc::new(state);
    app.manage(state.clone());
    state
}
