//! Container lifecycle actions through bollard.

use bollard::container::{
    RemoveContainerOptions, RestartContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::Docker;
use serde::Deserialize;

const STOP_TIMEOUT_SECS: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComposeAction {
    Start,
    Stop,
}

pub fn check_remove(state: &str, force: bool) -> Result<(), String> {
    if state == "running" && !force {
        Err("The container is running. Stop it first or check Force.".into())
    } else {
        Ok(())
    }
}

pub async fn run(
    docker: &Docker,
    id: &str,
    action: ContainerAction,
    force: bool,
) -> Result<(), String> {
    let result = match action {
        ContainerAction::Start => {
            docker
                .start_container(id, None::<StartContainerOptions<String>>)
                .await
        }
        ContainerAction::Stop => {
            docker
                .stop_container(
                    id,
                    Some(StopContainerOptions {
                        t: STOP_TIMEOUT_SECS,
                    }),
                )
                .await
        }
        ContainerAction::Restart => {
            docker
                .restart_container(
                    id,
                    Some(RestartContainerOptions {
                        t: STOP_TIMEOUT_SECS as isize,
                    }),
                )
                .await
        }
        ContainerAction::Remove => {
            docker
                .remove_container(
                    id,
                    Some(RemoveContainerOptions {
                        force,
                        ..Default::default()
                    }),
                )
                .await
        }
    };
    result.map_err(docker_error)
}

/// Docker's own message when it has one.
fn docker_error(error: bollard::errors::Error) -> String {
    match error {
        bollard::errors::Error::DockerResponseServerError { message, .. } => message,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_is_refused_on_a_running_container_without_force() {
        assert!(check_remove("running", false).is_err());
        assert!(check_remove("running", true).is_ok());
        assert!(check_remove("exited", false).is_ok());
    }

    #[test]
    fn actions_deserialize_from_lowercase_strings() {
        let a: ContainerAction = serde_json::from_str(r#""remove""#).unwrap();
        assert_eq!(a, ContainerAction::Remove);
        let c: ComposeAction = serde_json::from_str(r#""stop""#).unwrap();
        assert_eq!(c, ComposeAction::Stop);
    }
}
