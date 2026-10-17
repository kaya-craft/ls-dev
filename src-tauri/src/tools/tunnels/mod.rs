//! Tunnels tool: shares local dev servers on public URLs with cloudflared.

pub mod cloudflared;
pub mod commands;
pub mod names;
pub mod process;
pub mod stable;
pub mod state;
pub mod store;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::core::notify;
use crate::core::state::CoreState;
use state::TunnelsState;

pub const CHANGED_EVENT: &str = "tunnels:changed";
const EXPIRY_CHECK: Duration = Duration::from_secs(15);

/// Emits the full list of tunnels.
pub fn publish(app: &AppHandle) {
    if let Some(state) = app.try_state::<Arc<TunnelsState>>() {
        let _ = app.emit(CHANGED_EVENT, state.manager.list());
    }
}

/// Registers the Tunnels state (stable URLs from `tunnels.json`) and starts
/// the expiry check, every 15 s.
pub fn setup(app: &AppHandle, _core: Arc<CoreState>) -> Arc<TunnelsState> {
    let state = Arc::new(TunnelsState::new(store::load(app)));
    app.manage(state.clone());
    let (app, worker) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(EXPIRY_CHECK);
        loop {
            tick.tick().await;
            let expired = worker.manager.expire(process::now_ms()).await;
            for notice in state::closed_notices(&expired) {
                notify::send(&app, &notice);
            }
            if !expired.is_empty() {
                publish(&app);
            }
        }
    });
    state
}
