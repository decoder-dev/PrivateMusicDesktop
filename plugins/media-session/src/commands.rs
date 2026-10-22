//! Commands the webview may call (see `permissions/default.toml`).

use tauri::{command, AppHandle, Runtime};

use crate::{MediaSessionExt, Status};

/// Notification permission and battery-optimisation state.
#[command]
pub(crate) async fn status<R: Runtime>(app: AppHandle<R>) -> std::result::Result<Status, String> {
    app.media_session().status().await.map_err(|e| e.to_string())
}

/// Opens the system "ignore battery optimisations" screen. Never asks for the
/// exemption by itself.
#[command]
pub(crate) async fn open_battery_settings<R: Runtime>(app: AppHandle<R>) -> std::result::Result<(), String> {
    app.media_session().open_battery_settings().await.map_err(|e| e.to_string())
}
