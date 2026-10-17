//! Native update commands. All HTTP, hashing and secret-store work stays off the UI thread.
use pm_engine::update::{Snapshot, Updater};
use std::sync::Arc;
use tauri::{Emitter, State};
pub struct UpdateState(pub Arc<Updater>);
#[tauri::command]
pub async fn update_state(state: State<'_, UpdateState>) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || updater.refresh_access()).await.map_err(|_| "worker".into())
}
#[tauri::command]
pub async fn update_check(handle: tauri::AppHandle, state: State<'_, UpdateState>) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        updater.check(|snapshot| {
            let _ = handle.emit("pm-update", snapshot);
        })
    })
    .await
    .map_err(|_| "worker")?
    .map_err(String::from)
}
#[tauri::command]
pub async fn update_download(handle: tauri::AppHandle, state: State<'_, UpdateState>) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        updater.download(|snapshot| {
            let _ = handle.emit("pm-update", snapshot);
        })
    })
    .await
    .map_err(|_| "worker")?
    .map_err(String::from)
}
#[tauri::command]
pub fn update_cancel(state: State<'_, UpdateState>) {
    state.0.cancel();
}
#[tauri::command]
pub async fn update_access(state: State<'_, UpdateState>, token: String) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || updater.set_token(&token))
        .await
        .map_err(|_| "worker")?
        .map_err(String::from)
}
#[tauri::command]
pub async fn update_open(state: State<'_, UpdateState>) -> Result<(), String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = updater.verified_file().map_err(String::from)?;
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new(path).spawn();
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open").arg(path).spawn();
        // Linux opens the containing folder: package installation belongs to the system.
        #[cfg(target_os = "linux")]
        let result = std::process::Command::new("xdg-open").arg(path.parent().ok_or("disk")?).spawn();
        result.map(|_| ()).map_err(|_| "installer".into())
    })
    .await
    .map_err(|_| "worker")?
}
