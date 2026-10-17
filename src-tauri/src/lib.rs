mod commands;
mod core;
#[cfg(test)]
mod testutil;
mod tools;

use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let settings = core::settings::load(app.handle());
            app.set_theme(settings.theme.tauri_theme());
            let core = Arc::new(core::state::CoreState::new(settings));
            app.manage(core.clone());
            let services = tools::services::setup(app.handle(), core.clone());
            tools::worktrees::setup(app.handle(), core.clone(), services);
            tools::databases::setup(app.handle(), core.clone());
            tools::tunnels::setup(app.handle(), core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            tools::services::commands::get_snapshot,
            tools::services::commands::refresh,
            tools::services::commands::kill_process,
            tools::services::commands::restart_process,
            tools::services::commands::container_action,
            tools::services::commands::compose_action,
            tools::services::commands::logs_open,
            tools::services::commands::logs_close,
            tools::services::commands::open_browser,
            tools::services::commands::open_editor,
            tools::services::commands::open_terminal,
            tools::worktrees::commands::remove_worktree,
            tools::worktrees::commands::open_worktree_terminal,
            tools::worktrees::commands::open_worktree_shell,
            tools::worktrees::commands::worktrees_get,
            tools::worktrees::commands::worktrees_scan,
            tools::worktrees::commands::worktrees_remove_merged,
            tools::databases::commands::databases_list,
            tools::databases::commands::databases_parse_url,
            tools::databases::commands::databases_save,
            tools::databases::commands::databases_delete,
            tools::databases::commands::databases_test,
            tools::databases::commands::databases_set_read_only,
            tools::databases::commands::databases_run,
            tools::databases::commands::databases_cancel,
            tools::databases::commands::databases_history,
            tools::databases::commands::databases_clear_history,
            tools::tunnels::commands::tunnels_status,
            tools::tunnels::commands::tunnels_list,
            tools::tunnels::commands::tunnels_suggest_name,
            tools::tunnels::commands::tunnels_open,
            tools::tunnels::commands::tunnels_extend,
            tools::tunnels::commands::tunnels_stop,
            tools::tunnels::commands::tunnels_output,
            tools::tunnels::commands::tunnels_delete_stable,
            tools::tunnels::commands::tunnels_login,
        ])
        .build(tauri::generate_context!())
        .expect("error while building LsDev")
        .run(|app, event| {
            // Every cloudflared started by LsDev ends with it.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Arc<tools::tunnels::state::TunnelsState>>() {
                    state.manager.kill_all();
                }
            }
        });
}
