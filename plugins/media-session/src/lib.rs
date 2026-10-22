//! Android background playback for Private Music.
//!
//! The audio is produced by the Rust engine, not by an Android player, so
//! Android has to be told about it: a foreground service keeps the process
//! alive with the screen off, a `MediaSession` feeds the notification, the lock
//! screen and Bluetooth, and audio focus makes calls and other apps interrupt
//! correctly. This crate is the Rust half of that; the Kotlin half lives in
//! `android/`. On every other platform the plugin is a harmless no-op.
//!
//! Flow: the host forwards the engine's state to `bridge::Bridge`, which turns
//! it into [`Update`]s for the [`MediaSession`]; button presses, focus changes
//! and headset events come back as [`MediaAction`]s through the handler given to
//! [`MediaSession::set_action_handler`]; `bridge::dispatch` applies them to the
//! engine. See `README.md` for the full integration steps.
//!
//! The wire types below are shared with Kotlin (`Model.kt`): names and JSON
//! shapes must stay in step.

use serde::{Deserialize, Serialize};
use tauri::{plugin::TauriPlugin, AppHandle, Manager, Runtime};

#[cfg(feature = "engine")]
pub mod bridge;
mod commands;
#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

#[cfg(not(target_os = "android"))]
pub use desktop::MediaSession;
#[cfg(target_os = "android")]
pub use mobile::MediaSession;

/// Failures of the plugin. None of them should stop playback: the host logs
/// them (without URLs) and carries on.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[cfg(target_os = "android")]
    #[error("android plugin call failed: {0}")]
    Plugin(#[from] tauri::plugin::mobile::PluginInvokeError),
    #[error("event channel setup failed: {0}")]
    Channel(String),
    #[error("media session worker is gone")]
    WorkerGone,
}

pub type Result<T> = std::result::Result<T, Error>;

/// What is playing. Mirrors `pm_engine::media_sync::Metadata`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Artwork the plugin downloads itself (https only, size-limited, never
    /// logged). `None` shows the placeholder.
    pub artwork_url: Option<String>,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackKind {
    Playing,
    Paused,
    Buffering,
    Stopped,
}

/// The playback state at one instant. The system extrapolates the position
/// from `position_secs` and `rate` while `Playing`, so this is sent on state,
/// seek and track changes only, never every second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaybackInfo {
    pub state: PlaybackKind,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub rate: f64,
}

/// One `update` call. `metadata: None` with `clear_metadata: false` means
/// "unchanged"; `clear_metadata: true` means nothing is loaded any more.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Update {
    pub metadata: Option<TrackMetadata>,
    pub clear_metadata: bool,
    pub playback: Option<PlaybackInfo>,
}

impl Update {
    pub fn is_empty(&self) -> bool {
        self.metadata.is_none() && !self.clear_metadata && self.playback.is_none()
    }
}

/// Requests from the system, sent from Kotlin to Rust.
///
/// JSON: `{"action":"play"}`, `{"action":"seek_to","value":42.5}`,
/// `{"action":"duck","value":true}`.
///
/// `Duck(true)` means "another app asked us to be quieter": lower the volume
/// (see `bridge::Ducker`; the audio never passes through the JVM, so only the
/// host can); `Duck(false)` restores it. Everything else maps onto
/// `pm_engine::media_sync::MediaAction`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "snake_case")]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    SeekTo(f64),
    SeekBy(f64),
    Duck(bool),
}

/// State of the background-playback prerequisites, for a settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Status {
    /// `POST_NOTIFICATIONS` granted (always true below Android 13). Without it
    /// playback still works, only the notification is hidden.
    pub notifications_enabled: bool,
    /// The app is exempt from battery optimisation (Doze/app standby).
    pub ignoring_battery_optimizations: bool,
    /// True when the Android half is present (false on desktop and iOS).
    pub available: bool,
}

/// Anything that accepts [`Update`]s. The real one is [`MediaSession`]; tests
/// use a fake.
pub trait MediaSink {
    fn update(&self, update: &Update) -> Result<()>;
}

/// A [`MediaSink`] that reaches the plugin through the app handle, so a
/// `bridge::Bridge` can live inside the engine's event closure (the handle is
/// `Clone + Send + Sync`, the session itself is owned by Tauri's state).
pub struct AppSink<R: Runtime>(pub AppHandle<R>);

impl<R: Runtime> MediaSink for AppSink<R> {
    fn update(&self, update: &Update) -> Result<()> {
        self.0.media_session().update(update)
    }
}

/// Access to the plugin from any manager (`app.media_session()`).
pub trait MediaSessionExt<R: Runtime> {
    fn media_session(&self) -> &MediaSession<R>;
}

impl<R: Runtime, T: Manager<R>> MediaSessionExt<R> for T {
    fn media_session(&self) -> &MediaSession<R> {
        self.state::<MediaSession<R>>().inner()
    }
}

/// Registers the plugin: `.plugin(tauri_plugin_media_session::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("media-session")
        .invoke_handler(tauri::generate_handler![commands::status, commands::open_battery_settings])
        .setup(|app: &AppHandle<R>, api| {
            #[cfg(target_os = "android")]
            let session = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let session = desktop::init(app, api)?;
            app.manage(session);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_use_the_documented_json() {
        assert_eq!(serde_json::to_string(&MediaAction::Play).unwrap(), r#"{"action":"play"}"#);
        assert_eq!(serde_json::to_string(&MediaAction::SeekTo(42.5)).unwrap(), r#"{"action":"seek_to","value":42.5}"#);
        assert_eq!(serde_json::to_string(&MediaAction::Duck(true)).unwrap(), r#"{"action":"duck","value":true}"#);
        for action in [
            MediaAction::Play, MediaAction::Pause, MediaAction::Toggle, MediaAction::Next, MediaAction::Previous,
            MediaAction::Stop, MediaAction::SeekTo(1.0), MediaAction::SeekBy(-10.0), MediaAction::Duck(false),
        ] {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(serde_json::from_str::<MediaAction>(&json).unwrap(), action);
        }
        assert!(serde_json::from_str::<MediaAction>(r#"{"action":"explode"}"#).is_err());
    }

    #[test]
    fn updates_serialize_with_snake_case_states() {
        let update = Update {
            metadata: None,
            clear_metadata: true,
            playback: Some(PlaybackInfo { state: PlaybackKind::Buffering, position_secs: 1.5, duration_secs: 200.0, rate: 1.0 }),
        };
        let value = serde_json::to_value(&update).unwrap();
        assert_eq!(value["playback"]["state"], "buffering");
        assert_eq!(value["clear_metadata"], true);
        assert!(!update.is_empty());
        assert!(Update::default().is_empty());
    }
}
