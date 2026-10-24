//! Signed desktop updates, downloaded and installed outside the UI thread.
use pm_engine::update::Snapshot;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tauri::{Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
struct Inner {
    snapshot: Snapshot,
    update: Option<Update>,
    bytes: Option<Arc<Vec<u8>>>,
}
pub struct DesktopUpdater {
    inner: Mutex<Inner>,
    busy: AtomicBool,
    cancelled: AtomicBool,
}
pub struct UpdateState(pub Arc<DesktopUpdater>);
impl DesktopUpdater {
    pub fn new(current: &str) -> Self {
        Self {
            inner: Mutex::new(Inner {
                snapshot: Snapshot {
                    phase: "idle".into(),
                    current: current.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            busy: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }
    fn snapshot(&self) -> Snapshot {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .snapshot
            .clone()
    }
    fn change(&self, f: impl FnOnce(&mut Inner)) {
        f(&mut self.inner.lock().unwrap_or_else(|p| p.into_inner()));
    }
    fn start(&self) -> Result<Running<'_>, String> {
        self.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "busy")?;
        self.cancelled.store(false, Ordering::SeqCst);
        Ok(Running(self))
    }
    fn finish(&self, handle: &tauri::AppHandle, error: Option<&str>) -> Snapshot {
        if let Some(code) = error {
            self.change(|i| {
                i.snapshot.phase = if code == "cancelled" {
                    "cancelled"
                } else {
                    "error"
                }
                .into();
                i.snapshot.error = if code == "cancelled" {
                    String::new()
                } else {
                    code.into()
                };
            });
        }
        let s = self.snapshot();
        let _ = handle.emit("pm-update", s.clone());
        s
    }
}
struct Running<'a>(&'a DesktopUpdater);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}
#[tauri::command]
pub async fn update_state(state: State<'_, UpdateState>) -> Result<Snapshot, String> {
    let saved = tauri::async_runtime::spawn_blocking(|| {
        pm_engine::store::load_update_token()
            .ok()
            .flatten()
            .is_some()
    })
    .await
    .map_err(|_| "worker")?;
    state.0.change(|i| i.snapshot.access_saved = saved);
    Ok(state.0.snapshot())
}
#[tauri::command]
pub async fn update_check(
    handle: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    let _guard = updater.start()?;
    updater.change(|i| {
        i.update = None;
        i.bytes = None;
        let current = i.snapshot.current.clone();
        let access_saved = i.snapshot.access_saved;
        i.snapshot = Snapshot {
            phase: "checking".into(),
            current,
            access_saved,
            ..Default::default()
        };
    });
    updater.finish(&handle, None);
    let token = tauri::async_runtime::spawn_blocking(|| {
        pm_engine::store::load_update_token().ok().flatten()
    })
    .await
    .map_err(|_| "worker")?;
    updater.change(|i| i.snapshot.access_saved = token.is_some());
    let before_exit = handle.clone();
    let mut builder = handle
        .updater_builder()
        .on_before_exit(move || {
            if let Some(state) = before_exit.try_state::<crate::AppState>() {
                state.app.retry_session_storage();
                state.app.flush_history();
            }
        })
        .timeout(Duration::from_secs(300))
        .version_comparator(|current, release| {
            release.version.pre.is_empty() && release.version > current
        });
    if let Some(token) = token {
        builder = builder
            .header("Authorization", format!("Bearer {token}"))
            .map_err(|_| "token")?;
    }
    let native = match builder.build() {
        Ok(native) => native,
        Err(_) => return Ok(updater.finish(&handle, Some("platform"))),
    };
    let check = native.check();
    tokio::pin!(check);
    let result = loop {
        tokio::select! {
            result=&mut check=>break result,
            _=tokio::time::sleep(Duration::from_millis(100))=>if updater.cancelled.load(Ordering::SeqCst){return Ok(updater.finish(&handle,Some("cancelled")));}
        }
    };
    if updater.cancelled.load(Ordering::SeqCst) {
        return Ok(updater.finish(&handle, Some("cancelled")));
    }
    match result {
        Ok(Some(update)) if update.download_url.scheme() == "https" => {
            updater.change(|i| {
                i.snapshot.phase = "available".into();
                i.snapshot.version = update.version.clone();
                i.snapshot.notes = update
                    .body
                    .clone()
                    .unwrap_or_default()
                    .chars()
                    .take(12000)
                    .collect();
                i.snapshot.name = update
                    .download_url
                    .path_segments()
                    .and_then(|mut p| p.next_back())
                    .unwrap_or_default()
                    .into();
                i.update = Some(update);
            });
            Ok(updater.finish(&handle, None))
        }
        Ok(None) => {
            updater.change(|i| i.snapshot.phase = "current".into());
            Ok(updater.finish(&handle, None))
        }
        Ok(Some(_)) => Ok(updater.finish(&handle, Some("release"))),
        Err(_) => Ok(updater.finish(&handle, Some("network"))),
    }
}
#[tauri::command]
pub async fn update_download(
    handle: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    let _guard = updater.start()?;
    let update = updater
        .inner
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .update
        .clone()
        .ok_or("release")?;
    updater.change(|i| {
        i.bytes = None;
        i.snapshot.phase = "downloading".into();
        i.snapshot.error.clear();
        i.snapshot.downloaded = 0;
    });
    updater.finish(&handle, None);
    let mut last = std::time::Instant::now() - Duration::from_millis(100);
    let download = update.download(
        |chunk, total| {
            updater.change(|i| {
                i.snapshot.downloaded += chunk as u64;
                i.snapshot.total = total.unwrap_or(0);
            });
            if last.elapsed() >= Duration::from_millis(100) {
                updater.finish(&handle, None);
                last = std::time::Instant::now();
            }
        },
        || {
            updater.change(|i| i.snapshot.phase = "verifying".into());
            updater.finish(&handle, None);
        },
    );
    tokio::pin!(download);
    let result = loop {
        tokio::select! {
            result=&mut download=>break result,
            _=tokio::time::sleep(Duration::from_millis(100))=>{
                if updater.cancelled.load(Ordering::SeqCst){return Ok(updater.finish(&handle,Some("cancelled")));}
                if updater.snapshot().downloaded>512*1024*1024{return Ok(updater.finish(&handle,Some("size")));}
            }
        }
    };
    match result {
        Ok(bytes)
            if !updater.cancelled.load(Ordering::SeqCst) && bytes.len() <= 512 * 1024 * 1024 =>
        {
            updater.change(|i| {
                i.bytes = Some(Arc::new(bytes));
                i.snapshot.phase = "ready".into();
            });
            Ok(updater.finish(&handle, None))
        }
        Ok(_) => Ok(updater.finish(
            &handle,
            Some(if updater.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "size"
            }),
        )),
        Err(error) => {
            log::debug!("update download failed: {error}");
            Ok(updater.finish(&handle, Some("verification")))
        }
    }
}
#[tauri::command]
pub fn update_cancel(state: State<'_, UpdateState>) {
    state.0.cancelled.store(true, Ordering::SeqCst);
}
#[tauri::command]
pub async fn update_access(
    state: State<'_, UpdateState>,
    token: String,
) -> Result<Snapshot, String> {
    let updater = state.0.clone();
    let _guard = updater.start()?;
    let saved = tauri::async_runtime::spawn_blocking(move || {
        let token = token.trim();
        if token.len() > 512
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("token");
        }
        pm_engine::store::update_token(Some(token)).map_err(|_| "secret")?;
        Ok(!token.is_empty())
    })
    .await
    .map_err(|_| "worker")??;
    updater.change(|i| i.snapshot.access_saved = saved);
    Ok(updater.snapshot())
}
#[tauri::command]
pub async fn update_open(
    handle: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<(), String> {
    let updater = state.0.clone();
    // Keep the operation guard inside the worker, including permission prompts.
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = updater.start()?;
        let (update, bytes) = {
            let i = updater.inner.lock().unwrap_or_else(|p| p.into_inner());
            (
                i.update.clone().ok_or("release")?,
                i.bytes.clone().ok_or("release")?,
            )
        };
        update.verify(bytes.as_ref()).map_err(|_| "verification")?;
        updater.change(|i| i.snapshot.phase = "installing".into());
        updater.finish(&handle, None);
        if let Err(error) = update.install(bytes.as_ref()) {
            log::warn!("update installation failed: {error}");
            updater.finish(&handle, Some("installer"));
            return Err("installer".into());
        }
        updater.change(|i| {
            i.bytes = None;
            i.snapshot.phase = "installed".into();
        });
        updater.finish(&handle, None);
        // Windows' installer restarts the app itself; Unix bundles need relaunching.
        #[cfg(not(target_os = "windows"))]
        handle.restart();
        #[cfg(target_os = "windows")]
        Ok(())
    })
    .await
    .map_err(|_| "worker")?
}
