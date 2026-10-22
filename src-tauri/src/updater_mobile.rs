//! Android: встроенного обновления нет (приложение обновляется установкой нового APK).
use pm_engine::update::Snapshot;

/// Android: приложение обновляется через установку нового APK, встроенного обновления нет. Команды
/// существуют, чтобы UI не получал «command not found»; ответы — те же коды ошибок, что понимает UI.
#[tauri::command]
pub fn update_state(handle: tauri::AppHandle) -> Snapshot {
    Snapshot { phase: "idle".into(), current: handle.package_info().version.to_string(), ..Default::default() }
}
#[tauri::command]
pub fn update_check() -> Result<Snapshot, String> {
    Err("platform".into())
}
#[tauri::command]
pub fn update_download() -> Result<Snapshot, String> {
    Err("platform".into())
}
#[tauri::command]
pub fn update_cancel() {}
#[tauri::command]
pub fn update_access(token: String) -> Result<Snapshot, String> {
    let _ = token;
    Err("secret".into())
}
#[tauri::command]
pub fn update_open() -> Result<(), String> {
    Err("installer".into())
}
