//! The picker returns a small recompressed image. Uploads use opaque account-scoped handles.
use base64::{engine::general_purpose::STANDARD, Engine};
use pm_engine::{
    app::App,
    vk::{cover::checked_jpeg, Playlist},
    Error, Result,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

const MAX_PICKED_BYTES: usize = 2_000_000;
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Serialize)]
pub struct CoverInfo {
    id: String,
    preview: String,
}
struct PendingCover {
    bytes: Vec<u8>,
    epoch: u64,
    stop: Arc<AtomicBool>,
    uploading: AtomicBool,
}
#[derive(Default)]
pub struct CoverUploads(Mutex<HashMap<String, Arc<PendingCover>>>);
impl CoverUploads {
    fn insert(&self, id: String, pending: Arc<PendingCover>) {
        let mut covers = self.0.lock().unwrap_or_else(|p| p.into_inner());
        for old in covers.values() {
            old.stop.store(true, Ordering::Release);
        }
        covers.clear();
        covers.insert(id, pending);
    }
    fn discard(&self, id: &str) {
        if let Some(pending) = self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(id) {
            pending.stop.store(true, Ordering::Release);
        }
    }
    fn completed(&self, id: &str, result: &Result<()>) {
        if result.is_ok()
            || matches!(
                result,
                Err(Error::Cancelled | Error::Unauthorized | Error::UnconfirmedWrite)
            )
        {
            self.discard(id);
        }
    }
}
fn stage(encoded: String, epoch: u64) -> Result<(CoverInfo, Arc<PendingCover>)> {
    if encoded.len() > 3_000_000 {
        return Err(Error::InvalidRequest);
    }
    let bytes = STANDARD
        .decode(&encoded)
        .map_err(|_| Error::InvalidRequest)?;
    if bytes.len() > MAX_PICKED_BYTES {
        return Err(Error::InvalidRequest);
    }
    checked_jpeg(&bytes)?;
    let id = format!(
        "cover-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidRequest)?
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let preview = format!("data:image/jpeg;base64,{encoded}");
    Ok((
        CoverInfo { id, preview },
        Arc::new(PendingCover {
            bytes,
            epoch,
            stop: Arc::new(AtomicBool::new(false)),
            uploading: AtomicBool::new(false),
        }),
    ))
}
pub fn choose(handle: &AppHandle, app: &App) -> Result<Option<CoverInfo>> {
    let epoch = app.session_epoch()?;
    #[cfg(target_os = "android")]
    let encoded = {
        use tauri_plugin_vk_login::VkLoginExt;
        handle
            .vk_login()
            .pick_cover()
            .map_err(|_| Error::InvalidRequest)?
    };
    #[cfg(not(target_os = "android"))]
    let encoded: Option<String> = None;
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    let (info, pending) = stage(encoded, epoch)?;
    if app.session_epoch()? != epoch {
        return Err(Error::Unauthorized);
    }
    handle
        .state::<CoverUploads>()
        .insert(info.id.clone(), pending);
    Ok(Some(info))
}
pub fn discard(handle: &AppHandle, id: &str) {
    handle.state::<CoverUploads>().discard(id);
}
pub fn upload(handle: &AppHandle, app: &App, id: &str, playlist: &Playlist) -> Result<()> {
    let uploads = handle.state::<CoverUploads>();
    let pending = uploads
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(id)
        .cloned()
        .ok_or(Error::InvalidRequest)?;
    if app.session_epoch().ok() != Some(pending.epoch) || pending.stop.load(Ordering::Acquire) {
        uploads.discard(id);
        return Err(Error::Unauthorized);
    }
    if pending.uploading.swap(true, Ordering::AcqRel) {
        return Err(Error::InvalidRequest);
    }
    let result = app.set_playlist_cover(
        playlist,
        pending.bytes.clone(),
        pending.epoch,
        pending.stop.clone(),
    );
    pending.uploading.store(false, Ordering::Release);
    uploads.completed(id, &result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image() -> String {
        STANDARD.encode([0xff, 0xd8, 0xff, 0xff, 0xd9])
    }
    #[test]
    fn replacement_and_discard_cancel_the_pending_image() {
        let uploads = CoverUploads::default();
        let (first, a) = stage(image(), 1).unwrap();
        uploads.insert(first.id.clone(), a.clone());
        let (second, b) = stage(image(), 1).unwrap();
        uploads.insert(second.id.clone(), b.clone());
        assert!(a.stop.load(Ordering::Acquire));
        assert!(!b.stop.load(Ordering::Acquire));
        uploads.discard(&second.id);
        assert!(b.stop.load(Ordering::Acquire));
        assert!(uploads.0.lock().unwrap().is_empty());
    }
    #[test]
    fn ambiguous_or_terminal_writes_invalidate_the_handle_but_offline_can_retry() {
        let uploads = CoverUploads::default();
        let (info, pending) = stage(image(), 1).unwrap();
        uploads.insert(info.id.clone(), pending.clone());
        uploads.completed(&info.id, &Err(Error::Offline));
        assert!(!pending.stop.load(Ordering::Acquire));
        uploads.completed(&info.id, &Err(Error::UnconfirmedWrite));
        assert!(pending.stop.load(Ordering::Acquire));
        assert!(uploads.0.lock().unwrap().is_empty());
        assert!(stage("not base64".into(), 1).is_err());
        assert!(stage(STANDARD.encode(b"not jpeg"), 1).is_err());
    }
}
