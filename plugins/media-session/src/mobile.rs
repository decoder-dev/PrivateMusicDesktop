//! Android implementation: forwards to the Kotlin `MediaSessionPlugin`.
//!
//! Playback commands go through one worker thread so they stay in order and
//! the engine's event emitter (which must not block) never waits for the JVM.

use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

use crate::{Error, MediaAction, MediaSink, Result, Status, Update};

const PLUGIN_IDENTIFIER: &str = "com.dec.privatemusic.mediasession";

pub struct MediaSession<R: Runtime> {
    handle: PluginHandle<R>,
    queue: Sender<(&'static str, Value)>,
    /// Keeps the event channel registered for as long as Kotlin may use it.
    channel: Mutex<Option<Channel<Value>>>,
}

pub fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> std::result::Result<MediaSession<R>, Box<dyn std::error::Error>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MediaSessionPlugin")?;
    let (queue, commands) = channel::<(&'static str, Value)>();
    let worker = handle.clone();
    std::thread::Builder::new().name("media-session".into()).spawn(move || {
        while let Ok((command, payload)) = commands.recv() {
            if let Err(error) = worker.run_mobile_plugin::<Value>(command, payload) {
                // Never log the payload: it holds titles and artwork URLs.
                log::debug!("media session {command}: {error}");
            }
        }
    })?;
    Ok(MediaSession { handle, queue, channel: Mutex::new(None) })
}

impl<R: Runtime> MediaSession<R> {
    fn enqueue(&self, command: &'static str, payload: impl Serialize) -> Result<()> {
        let payload = serde_json::to_value(payload).map_err(|e| Error::Channel(e.to_string()))?;
        self.queue.send((command, payload)).map_err(|_| Error::WorkerGone)
    }

    /// Receives what the system asks for. Replaces an earlier handler. The
    /// handler runs on a JVM/IPC thread: hand the action to the engine and
    /// return (the engine's controller calls are non-blocking).
    pub fn set_action_handler(&self, handler: impl Fn(MediaAction) + Send + Sync + 'static) -> Result<()> {
        let channel = Channel::<Value>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                match serde_json::from_str::<MediaAction>(&json) {
                    Ok(action) => handler(action),
                    Err(error) => log::debug!("media session: unreadable action: {error}"),
                }
            }
            Ok(())
        });
        self.enqueue("setEventChannel", json!({ "channel": channel }))?;
        *self.channel.lock().unwrap_or_else(|p| p.into_inner()) = Some(channel);
        Ok(())
    }

    /// Metadata and/or playback state. Starts the foreground service on the
    /// first `Playing`, and asks for the notification permission once.
    pub fn update(&self, update: &Update) -> Result<()> {
        if update.is_empty() {
            return Ok(());
        }
        self.enqueue("update", update)
    }

    /// Overrides the artwork URL (https only) without touching the metadata.
    pub fn set_artwork_url(&self, url: Option<&str>) -> Result<()> {
        self.enqueue("setArtworkUrl", json!({ "url": url }))
    }

    /// Takes audio focus. Called implicitly when playback starts; calling it
    /// again is harmless.
    pub fn request_audio_focus(&self) -> Result<()> {
        self.enqueue("requestAudioFocus", json!({}))
    }

    pub fn abandon_audio_focus(&self) -> Result<()> {
        self.enqueue("abandonAudioFocus", json!({}))
    }

    /// Ends the session: abandons focus, drops the notification and stops the
    /// service.
    pub fn stop(&self) -> Result<()> {
        self.enqueue("stop", json!({}))
    }

    pub fn configure_playback(&self, resume_bluetooth: bool, pause_at_zero: bool) -> Result<()> {
        self.enqueue("configurePlayback", json!({"resumeBluetooth":resume_bluetooth,"pauseAtZero":pause_at_zero}))
    }

    pub async fn status(&self) -> Result<Status> {
        let mut status: Status = self.handle.run_mobile_plugin_async("status", json!({})).await?;
        status.available = true;
        Ok(status)
    }

    pub async fn open_battery_settings(&self) -> Result<()> {
        self.handle.run_mobile_plugin_async::<Value>("openBatterySettings", json!({})).await?;
        Ok(())
    }
}

impl<R: Runtime> MediaSink for MediaSession<R> {
    fn update(&self, update: &Update) -> Result<()> {
        MediaSession::update(self, update)
    }
}
