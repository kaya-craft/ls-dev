//! Services: listening dev servers and Docker containers.

pub mod collect;
pub mod commands;
pub mod compose;
pub mod docker_actions;
pub mod docker_stats;
pub mod logs;
pub mod model;
pub mod open;
pub mod probe;
pub mod processes;
pub mod restart;
pub mod sockets;
pub mod state;
pub mod stops;

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::core::state::CoreState;
use state::ServicesState;

/// Registers the Services state and starts the collection loop.
pub fn setup(app: &AppHandle, core: Arc<CoreState>) -> Arc<ServicesState> {
    let services = Arc::new(ServicesState::new());
    app.manage(services.clone());
    collect::spawn(app.clone(), core, services.clone());
    services
}
