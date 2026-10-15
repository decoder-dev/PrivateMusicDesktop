//! Media keys and the "Now Playing" surface of each OS (MPRIS on Linux,
//! SMTC on Windows, MediaRemote on macOS), driven by `media_sync`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use pm_engine::controller::UiEvent;
use pm_engine::media_sync::{apply, MediaAction, MediaSync, Metadata, Playback};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection,
};
use tauri::{AppHandle, Manager};

use crate::AppState;

const SEEK_STEP: f64 = 10.0;

pub struct Media {
    controls: Mutex<MediaControls>,
    sync: Mutex<MediaSync>,
    started: Instant,
}

impl Media {
    /// `None` when the OS refuses (no session bus, no window handle): the
    /// player works without media keys.
    pub fn new(handle: &AppHandle) -> Option<Self> {
        #[cfg(target_os = "windows")]
        let hwnd = {
            let window = handle.get_webview_window("main")?;
            Some(window.hwnd().ok()?.0)
        };
        #[cfg(not(target_os = "windows"))]
        let hwnd = None;

        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "com.dec.privatemusic2.desktop",
            display_name: "Private Music",
            hwnd,
        })
        .map_err(|error| log::warn!("media controls unavailable: {error:?}"))
        .ok()?;

        let target = handle.clone();
        controls
            .attach(move |event| {
                if let Some(action) = action_for(&event) {
                    if let Some(state) = target.try_state::<AppState>() {
                        apply(state.app.controller(), action);
                    }
                }
            })
            .map_err(|error| log::warn!("media controls could not attach: {error:?}"))
            .ok()?;
        Some(Self { controls: Mutex::new(controls), sync: Mutex::new(MediaSync::default()), started: Instant::now() })
    }

    pub fn on_event(&self, event: &UiEvent) {
        let UiEvent::State(state) = event else { return };
        let now = self.started.elapsed().as_secs_f64();
        let updates = self.sync.lock().unwrap_or_else(|p| p.into_inner()).update(state, now);
        if updates.metadata.is_none() && updates.playback.is_none() {
            return;
        }
        let mut controls = self.controls.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(metadata) = &updates.metadata {
            let result = match metadata {
                Some(m) => set_metadata(&mut controls, m),
                None => controls.set_metadata(MediaMetadata::default()),
            };
            if let Err(error) = result {
                log::debug!("media metadata: {error:?}");
            }
        }
        if let Some(playback) = updates.playback {
            let position = |seconds: f64| Some(MediaPosition(Duration::from_secs_f64(seconds.max(0.0))));
            let value = match playback {
                Playback::Stopped => MediaPlayback::Stopped,
                Playback::Paused(p) => MediaPlayback::Paused { progress: position(p) },
                Playback::Playing(p) => MediaPlayback::Playing { progress: position(p) },
            };
            if let Err(error) = controls.set_playback(value) {
                log::debug!("media playback: {error:?}");
            }
        }
    }
}

fn set_metadata(controls: &mut MediaControls, m: &Metadata) -> Result<(), souvlaki::Error> {
    controls.set_metadata(MediaMetadata {
        title: Some(&m.title),
        artist: Some(&m.artist),
        album: m.album.as_deref(),
        // Only https artwork: the OS fetches this URL itself.
        cover_url: m.cover_url.as_deref().filter(|u| u.starts_with("https://")),
        duration: (m.duration.is_finite() && m.duration > 0.0).then(|| Duration::from_secs_f64(m.duration)),
    })
}

fn action_for(event: &MediaControlEvent) -> Option<MediaAction> {
    let sign = |d: &SeekDirection| if matches!(d, SeekDirection::Forward) { 1.0 } else { -1.0 };
    Some(match event {
        MediaControlEvent::Play => MediaAction::Play,
        MediaControlEvent::Pause => MediaAction::Pause,
        MediaControlEvent::Toggle => MediaAction::Toggle,
        MediaControlEvent::Next => MediaAction::Next,
        MediaControlEvent::Previous => MediaAction::Previous,
        MediaControlEvent::Stop => MediaAction::Stop,
        MediaControlEvent::Seek(d) => MediaAction::SeekBy(sign(d) * SEEK_STEP),
        MediaControlEvent::SeekBy(d, by) => MediaAction::SeekBy(sign(d) * by.as_secs_f64()),
        MediaControlEvent::SetPosition(MediaPosition(p)) => MediaAction::SeekTo(p.as_secs_f64()),
        MediaControlEvent::SetVolume(_)
        | MediaControlEvent::OpenUri(_)
        | MediaControlEvent::Raise
        | MediaControlEvent::Quit => return None,
    })
}
