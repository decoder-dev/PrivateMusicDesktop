use pm_engine::update::{Snapshot, Updater};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
pub struct UpdateState(pub Arc<Updater>);
#[tauri::command]
pub fn update_state(state: State<'_, UpdateState>) -> Snapshot {
    state.0.snapshot()
}
#[tauri::command]
pub async fn update_check(
    handle: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        updater.check(|s| {
            let _ = handle.emit("pm-update", s);
        })
    })
    .await
    .map_err(|_| "worker")?
    .map_err(String::from)
}
#[tauri::command]
pub async fn update_download(
    handle: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        updater.download(|s| {
            let _ = handle.emit("pm-update", s);
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
pub fn update_access(_token: String) -> Result<Snapshot, String> {
    Err("secret".into())
}
#[tauri::command]
pub async fn update_open(
    handle: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<(), String> {
    let updater = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = updater.verified_file().map_err(String::from)?;
        if let Some(state) = handle.try_state::<crate::AppState>() {
            state.app.retry_session_storage();
            state.app.flush_history();
        }
        #[cfg(target_os = "android")]
        {
            use tauri_plugin_vk_login::VkLoginExt;
            handle.vk_login().install_update(&path).map_err(|e| {
                if e.to_string().contains("install_permission") {
                    "install_permission".into()
                } else {
                    "installer".into()
                }
            })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (handle, path);
            Err("platform".into())
        }
    })
    .await
    .map_err(|_| "worker")?
}
