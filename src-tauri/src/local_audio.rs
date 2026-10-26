//! The WebView can request a system picker, never choose a filesystem path.
#[cfg(mobile)]
use std::path::PathBuf;
use pm_engine::{app::App, offline::OfflineTrack, Error, Result};
use serde::Serialize;
use tauri::AppHandle;
#[cfg(target_os = "android")]
use tauri::Manager;

#[derive(Default, Serialize)]
pub struct ImportResult {
    pub imported: Vec<OfflineTrack>,
    pub failed: Vec<String>,
    pub skipped: usize,
}

pub fn choose(handle: &AppHandle, app: &App) -> Result<ImportResult> {
    #[cfg(desktop)]
    let (selected, failed, skipped) = {
        use tauri_plugin_dialog::DialogExt;
        let picked = handle.dialog().file().set_title("MP3 · M4A · AAC · WAV · FLAC")
            .add_filter("Audio", pm_engine::local_audio::EXTENSIONS).blocking_pick_files().unwrap_or_default();
        let skipped = picked.len().saturating_sub(100);
        let files = picked.into_iter().take(100).map(|file| {
            let path = file.into_path().map_err(|_| Error::InvalidRequest)?;
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            Ok((path, name))
        }).collect::<Result<Vec<_>>>()?;
        (files, Vec::new(), skipped)
    };
    #[cfg(target_os = "android")]
    let (selected, failed, skipped) = {
        use tauri_plugin_vk_login::VkLoginExt;
        let picked = handle.vk_login().pick_local_audio().map_err(|_| Error::InvalidRequest)?;
        let files = picked.files.into_iter().map(|file| (PathBuf::from(file.path), file.name)).collect::<Vec<_>>();
        (files, picked.failed, picked.skipped)
    };
    #[cfg(all(mobile, not(target_os = "android")))]
    let (selected, failed, skipped): (Vec<(PathBuf, String)>, Vec<String>, usize) = (Vec::new(), Vec::new(), 0);
    let mut result = ImportResult { imported: Vec::new(), failed, skipped };
    for (source, name) in selected {
        #[cfg(target_os = "android")]
        let source = {
            let root = handle.path().app_cache_dir().map_err(|_| Error::InvalidRequest)?.join("audio-picker");
            let root = root.canonicalize().map_err(|_| Error::InvalidRequest)?;
            let path = source.canonicalize().map_err(|_| Error::InvalidRequest)?;
            if path.parent() != Some(root.as_path()) { return Err(Error::InvalidRequest); }
            path
        };
        match app.import_local_audio(&source, &name) {
            Ok(entry) => result.imported.push(entry),
            Err(_) => result.failed.push(name.chars().filter(|c| !c.is_control()).take(256).collect()),
        }
        #[cfg(target_os = "android")]
        let _ = std::fs::remove_file(&source);
    }
    Ok(result)
}
