//! File paths stay native. The WebView only receives an opaque, account-scoped handle.
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pm_engine::app::App;
use pm_engine::vk::{
    upload::{checked_mp3, MAX_AUDIO_BYTES},
    Track,
};
use pm_engine::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize)]
pub struct AudioFileInfo {
    id: String,
    name: String,
    bytes: u64,
}
struct PendingAudio {
    info: AudioFileInfo,
    path: PathBuf,
    epoch: u64,
    stop: Arc<AtomicBool>,
    uploading: AtomicBool,
}
impl Drop for PendingAudio {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
#[derive(Default)]
pub struct AudioUploads(Mutex<HashMap<String, Arc<PendingAudio>>>);
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

fn new_id() -> String {
    format!(
        "{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    )
}
fn cleanup(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("stage-") {
            continue;
        }
        let old = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(86_400));
        if old && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}
fn stage(source: &Path, name: &str, folder: &Path, epoch: u64) -> Result<Arc<PendingAudio>> {
    let source = source.canonicalize().map_err(|_| Error::InvalidRequest)?;
    let (mut input, bytes) = checked_mp3(&source)?;
    fs::create_dir_all(folder).map_err(|_| Error::InvalidRequest)?;
    cleanup(folder);
    let id = new_id();
    let path = folder.join(format!("stage-{id}.mp3"));
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(&path).map_err(|_| Error::InvalidRequest)?;
    let copied = std::io::copy(&mut input.by_ref().take(MAX_AUDIO_BYTES), &mut output);
    drop(output);
    if copied.ok() != Some(bytes) {
        let _ = fs::remove_file(&path);
        return Err(Error::InvalidRequest);
    }
    let name: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("audio.mp3")
        .chars()
        .filter(|ch| !ch.is_control())
        .take(256)
        .collect();
    Ok(Arc::new(PendingAudio {
        info: AudioFileInfo {
            id,
            name: if name.is_empty() {
                "audio.mp3".into()
            } else {
                name
            },
            bytes,
        },
        path,
        epoch,
        stop: Arc::new(AtomicBool::new(false)),
        uploading: AtomicBool::new(false),
    }))
}

pub fn choose(handle: &AppHandle, app: &App) -> Result<Option<AudioFileInfo>> {
    let epoch = app.session_epoch()?;
    let cache = handle
        .path()
        .app_cache_dir()
        .map_err(|_| Error::InvalidRequest)?;
    #[cfg(desktop)]
    let selected = {
        use tauri_plugin_dialog::DialogExt;
        handle
            .dialog()
            .file()
            .set_title("MP3")
            .add_filter("MP3", &["mp3"])
            .blocking_pick_file()
            .map(|file| file.into_path().map_err(|_| Error::InvalidRequest))
            .transpose()?
            .map(|path| {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                (path, name)
            })
    };
    #[cfg(target_os = "android")]
    let selected = {
        use tauri_plugin_vk_login::VkLoginExt;
        let reply = handle
            .vk_login()
            .pick_audio()
            .map_err(|_| Error::InvalidRequest)?;
        reply.map(|file| (PathBuf::from(file.path), file.name))
    };
    #[cfg(all(mobile, not(target_os = "android")))]
    let selected: Option<(PathBuf, String)> = None;
    let Some((source, name)) = selected else {
        return Ok(None);
    };
    #[cfg(target_os = "android")]
    let source = {
        let root = cache
            .join("audio-picker")
            .canonicalize()
            .map_err(|_| Error::InvalidRequest)?;
        let source = source.canonicalize().map_err(|_| Error::InvalidRequest)?;
        if !source.starts_with(root) {
            return Err(Error::InvalidRequest);
        }
        source
    };
    let staged = stage(&source, &name, &cache.join("audio-upload"), epoch);
    #[cfg(target_os = "android")]
    let _ = fs::remove_file(&source);
    let staged = staged?;
    if app.session_epoch()? != epoch {
        return Err(Error::Unauthorized);
    }
    let info = staged.info.clone();
    let state = handle.state::<AudioUploads>();
    let mut pending = state.0.lock().unwrap_or_else(|p| p.into_inner());
    for entry in pending.values() {
        entry.stop.store(true, Ordering::Release);
    }
    pending.clear();
    pending.insert(info.id.clone(), staged);
    Ok(Some(info))
}

impl AudioUploads {
    fn discard(&self, id: &str) {
        if let Some(entry) = self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(id) {
            entry.stop.store(true, Ordering::Release);
        }
    }

    fn completed(&self, id: &str, entry: &PendingAudio, answer: &Result<Track>) {
        entry.uploading.store(false, Ordering::Release);
        if answer.is_ok()
            || matches!(
                answer,
                Err(Error::UnconfirmedWrite | Error::Unauthorized | Error::Cancelled)
            )
            || entry.stop.load(Ordering::Acquire)
        {
            self.discard(id);
        }
    }
}

pub fn discard(handle: &AppHandle, id: &str) {
    handle.state::<AudioUploads>().discard(id);
}

pub fn upload(handle: &AppHandle, app: &App, id: &str, artist: &str, title: &str) -> Result<Track> {
    let entry = handle
        .state::<AudioUploads>()
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(id)
        .cloned()
        .ok_or(Error::InvalidRequest)?;
    if app.session_epoch()? != entry.epoch {
        discard(handle, id);
        return Err(Error::Unauthorized);
    }
    if entry
        .uploading
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(Error::InvalidRequest);
    }
    let answer = app.upload_audio(&entry.path, artist, title, entry.epoch, entry.stop.clone());
    handle
        .state::<AudioUploads>()
        .completed(id, &entry, &answer);
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn staged_handles_own_only_private_copies_and_clean_them_on_drop() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(b"ID3some audio").unwrap();
        let folder = tempfile::tempdir().unwrap();
        let first = stage(source.path(), "../../song.mp3", folder.path(), 7).unwrap();
        let second = stage(source.path(), "other.mp3", folder.path(), 8).unwrap();
        assert_eq!(first.info.name, "song.mp3");
        assert_ne!(first.info.id, second.info.id);
        assert_eq!(first.epoch, 7);
        assert_eq!(first.info.bytes, 13);
        let path = first.path.clone();
        assert!(path.starts_with(folder.path()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(first);
        assert!(!path.exists());
        assert!(source.path().exists());
        assert!(second.path.exists());
    }
    #[test]
    fn transient_upload_failure_can_retry_but_ambiguous_or_revoked_handles_are_destroyed() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(b"ID3some audio").unwrap();
        let folder = tempfile::tempdir().unwrap();
        for terminal in [
            Error::UnconfirmedWrite,
            Error::Unauthorized,
            Error::Cancelled,
        ] {
            let state = AudioUploads::default();
            let entry = stage(source.path(), "song.mp3", folder.path(), 7).unwrap();
            let id = entry.info.id.clone();
            let path = entry.path.clone();
            state.0.lock().unwrap().insert(id.clone(), entry.clone());
            entry.uploading.store(true, Ordering::Release);
            state.completed(&id, &entry, &Err(Error::Offline));
            assert!(state.0.lock().unwrap().contains_key(&id));
            assert!(path.exists());
            assert!(!entry.uploading.load(Ordering::Acquire));
            state.completed(&id, &entry, &Err(terminal));
            assert!(!state.0.lock().unwrap().contains_key(&id));
            drop(entry);
            assert!(!path.exists());
        }
    }
}
