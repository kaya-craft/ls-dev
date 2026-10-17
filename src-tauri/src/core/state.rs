//! State shared by every tool.

use std::sync::{Arc, RwLock};

use bollard::Docker;

use crate::core::settings::Settings;

pub type CoreRef<'a> = tauri::State<'a, Arc<CoreState>>;

pub struct CoreState {
    pub settings: RwLock<Settings>,
    /// Set by the Services collector when Docker answers; cleared when it stops answering.
    pub docker: RwLock<Option<Docker>>,
}

impl CoreState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: RwLock::new(settings),
            docker: RwLock::new(None),
        }
    }

    pub fn docker_handle(&self) -> Result<Docker, String> {
        self.docker
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| "Docker is not reachable".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_handle_errors_when_docker_is_absent() {
        assert_eq!(
            CoreState::new(Settings::default())
                .docker_handle()
                .unwrap_err(),
            "Docker is not reachable"
        );
    }
}
