//! Tauri commands shared by every tool: settings.

use tauri::AppHandle;

use crate::core::settings::{self, Settings};
use crate::core::state::CoreRef;
use crate::tools::services::state::ServicesRef;

#[tauri::command]
pub fn get_settings(core: CoreRef<'_>) -> Settings {
    core.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    core: CoreRef<'_>,
    services: ServicesRef<'_>,
    settings: Settings,
) -> Result<Settings, String> {
    use tauri_plugin_autostart::ManagerExt;

    let settings = settings.normalized()?;
    let autolaunch = app.autolaunch();
    let enabled = autolaunch.is_enabled().unwrap_or(false);
    if settings.launch_at_login && !enabled {
        autolaunch.enable().map_err(|e| e.to_string())?;
    } else if !settings.launch_at_login && enabled {
        autolaunch.disable().map_err(|e| e.to_string())?;
    }
    settings::save(&app, &settings)?;
    app.set_theme(settings.theme.tauri_theme());
    *core.settings.write().unwrap() = settings.clone();
    services.refresh.notify_one();
    Ok(settings)
}
