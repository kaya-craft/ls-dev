//! System notifications.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub fn send(app: &AppHandle, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title("LsDev")
        .body(body)
        .show();
}
