//! No-op implementation for desktop and iOS: the OS media controls there are
//! handled elsewhere (souvlaki on desktop). Everything succeeds and does nothing,
//! so host code needs no `cfg` of its own.

use std::marker::PhantomData;

use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{MediaAction, MediaSink, Result, Status, Update};

pub struct MediaSession<R: Runtime>(PhantomData<fn() -> R>);

pub fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> std::result::Result<MediaSession<R>, Box<dyn std::error::Error>> {
    Ok(MediaSession(PhantomData))
}

impl<R: Runtime> MediaSession<R> {
    pub fn set_action_handler(&self, _handler: impl Fn(MediaAction) + Send + Sync + 'static) -> Result<()> {
        Ok(())
    }
    pub fn update(&self, _update: &Update) -> Result<()> {
        Ok(())
    }
    pub fn set_artwork_url(&self, _url: Option<&str>) -> Result<()> {
        Ok(())
    }
    pub fn request_audio_focus(&self) -> Result<()> {
        Ok(())
    }
    pub fn abandon_audio_focus(&self) -> Result<()> {
        Ok(())
    }
    pub fn stop(&self) -> Result<()> {
        Ok(())
    }
    pub fn configure_playback(&self, _resume_bluetooth: bool, _pause_at_zero: bool) -> Result<()> { Ok(()) }
    pub async fn status(&self) -> Result<Status> {
        Ok(Status::default())
    }
    pub async fn open_battery_settings(&self) -> Result<()> {
        Ok(())
    }
}

impl<R: Runtime> MediaSink for MediaSession<R> {
    fn update(&self, update: &Update) -> Result<()> {
        MediaSession::update(self, update)
    }
}
