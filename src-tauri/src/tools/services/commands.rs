//! Tauri commands of the Services tool. Thin: they look things up in the state and delegate.

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use super::collect::now_ms;
use super::docker_actions::{self, ComposeAction, ContainerAction};
use super::logs;
use super::model::Snapshot;
use super::open::{container_shell_command, editor_command};
use super::restart::spawn_checked;
use super::state::ServicesRef;
use crate::core::kill::kill_tree;
use crate::core::shell::{run_launch, terminal_launch, Launch};
use crate::core::state::CoreRef;

#[tauri::command]
pub fn get_snapshot(services: ServicesRef<'_>) -> Snapshot {
    services.snapshot.read().unwrap().clone()
}

#[tauri::command]
pub fn refresh(services: ServicesRef<'_>) {
    services.refresh.notify_one();
}

#[derive(Serialize)]
pub struct KillReport {
    pub survivors: Vec<u32>,
}

#[tauri::command]
pub async fn kill_process(services: ServicesRef<'_>, id: String) -> Result<KillReport, String> {
    let row = services
        .find_process(&id)
        .ok_or("This service is no longer running.")?;
    services.suppress_id(&id);
    let survivors = kill_tree(row.root_pid, &row.command).await?;
    services.refresh.notify_one();
    Ok(KillReport { survivors })
}

#[tauri::command]
pub async fn restart_process(
    app: AppHandle,
    services: ServicesRef<'_>,
    id: String,
) -> Result<(), String> {
    let row = services
        .find_process(&id)
        .ok_or("This service is no longer running.")?;
    let spec = row
        .restart
        .clone()
        .ok_or("This service cannot be restarted: its working directory or command is unknown.")?;
    services.suppress_id(&id);
    let survivors = kill_tree(row.root_pid, &row.command).await?;
    if !survivors.is_empty() {
        return Err(format!(
            "Could not stop process(es) {survivors:?}; not restarting."
        ));
    }
    let log_dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    let log_path = log_dir.join(format!("managed-{}.log", now_ms()));
    let pid = spawn_checked(&spec, &crate::core::settings::shell(), &log_path).await?;
    services.managed.lock().unwrap().insert(pid, log_path);
    services.refresh.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn container_action(
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    id: String,
    action: ContainerAction,
    force: Option<bool>,
) -> Result<(), String> {
    let container = services
        .find_container(&id)
        .ok_or("This container no longer exists.")?;
    let force = force.unwrap_or(false);
    if action == ContainerAction::Remove {
        docker_actions::check_remove(&container.state, force)?;
    }
    let docker = core.docker_handle()?;
    services.suppress_id(&id);
    docker_actions::run(&docker, &container.container_id, action, force).await?;
    services.refresh.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn compose_action(
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    project: String,
    action: ComposeAction,
) -> Result<(), String> {
    let group = services
        .compose_group(&project)
        .ok_or("This Compose project no longer exists.")?;
    let docker = core.docker_handle()?;
    let single = match action {
        ComposeAction::Start => ContainerAction::Start,
        ComposeAction::Stop => ContainerAction::Stop,
    };
    let mut errors = Vec::new();
    for c in &group.containers {
        services.suppress_id(&c.id);
        if let Err(e) = docker_actions::run(&docker, &c.container_id, single, false).await {
            errors.push(format!("{}: {e}", c.name));
        }
    }
    services.refresh.notify_one();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[tauri::command]
pub fn logs_open(
    app: AppHandle,
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    id: String,
) -> Result<(), String> {
    let mut session = services.logs.lock().unwrap();
    *session = None;
    if let Some(container) = services.find_container(&id) {
        let docker = core.docker_handle()?;
        *session = Some(logs::open_docker(app, docker, id, container.container_id));
        return Ok(());
    }
    if let Some(process) = services.find_process(&id) {
        let path = process
            .managed_pid
            .and_then(|pid| services.managed.lock().unwrap().log_path(pid))
            .ok_or("Started outside LsDev, no logs available.")?;
        *session = Some(logs::open_file(app, id, path));
        return Ok(());
    }
    Err("This service no longer exists.".into())
}

#[tauri::command]
pub fn logs_close(services: ServicesRef<'_>) {
    *services.logs.lock().unwrap() = None;
}

#[tauri::command]
pub fn open_browser(app: AppHandle, port: u16) -> Result<(), String> {
    app.opener()
        .open_url(format!("http://localhost:{port}"), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_editor(
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    id: String,
) -> Result<(), String> {
    let path = if let Some(p) = services.find_process(&id) {
        p.cwd
    } else {
        services
            .find_container(&id)
            .and_then(|c| c.compose)
            .and_then(|c| c.working_dir)
    }
    .ok_or("No directory is known for this service.")?;
    let editor = core.settings.read().unwrap().editor_command.clone();
    let line = editor_command(&editor, &path);
    tauri::async_runtime::spawn_blocking(move || run_launch(Launch::Shell(line)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("The editor command failed ({e}). Check Settings."))
}

#[tauri::command]
pub async fn open_terminal(
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    id: String,
) -> Result<(), String> {
    let container = services
        .find_container(&id)
        .ok_or("Terminals are only available for containers.")?;
    if container.state != "running" {
        return Err("Start the container first.".into());
    }
    let terminal = core.settings.read().unwrap().terminal.clone();
    let launch = terminal_launch(&terminal, &container_shell_command(&container.container_id));
    tauri::async_runtime::spawn_blocking(move || run_launch(launch))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Could not open the terminal ({e}). Check Settings."))
}
