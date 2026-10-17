//! Worktrees: linked git worktrees of local repositories.

pub mod commands;
pub mod discover;
pub mod inspect;
pub mod remove;
pub mod scan;
pub mod size;
pub mod state;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::core::state::CoreState;
use crate::tools::services::state::ServicesState;
use state::WorktreesState;

const DU_TIMEOUT: Duration = Duration::from_secs(60);

/// Registers the Worktrees state and starts the size worker. Scans are
/// started by the frontend (`worktrees_scan`).
pub fn setup(
    app: &AppHandle,
    _core: Arc<CoreState>,
    _services: Arc<ServicesState>,
) -> Arc<WorktreesState> {
    let state = Arc::new(WorktreesState::new());
    app.manage(state.clone());
    let (app, worker_state) = (app.clone(), state.clone());
    let spawned = std::thread::Builder::new()
        .name("worktrees-sizes".into())
        .spawn(move || size_worker(app, worker_state));
    if let Err(e) = spawned {
        eprintln!("LsDev worktrees: cannot start the size worker: {e}");
    }
    state
}

/// One `du` at a time, on its own thread: nothing here runs on the async runtime.
fn size_worker(app: AppHandle, state: Arc<WorktreesState>) {
    loop {
        let path = state.next_size_job();
        let measured = size::du(Path::new(&path), DU_TIMEOUT);
        {
            let mut sizes = state.sizes.lock().unwrap_or_else(|e| e.into_inner());
            match measured {
                Some(bytes) => sizes.put(&path, bytes, Instant::now()),
                // Cached as a failure: the next scans do not queue it again.
                None => sizes.put_failure(&path, Instant::now()),
            }
        }
        state.finish_size_job();
        if let Some(bytes) = measured {
            if state.apply_size(&path, bytes) {
                scan::publish(&app, &state);
            }
        }
    }
}
