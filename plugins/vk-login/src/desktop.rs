//! Stub for every target without the native Activity. It keeps the crate (and
//! a host app that depends on it) compiling on the desktop.

use tauri::plugin::PluginApi;
use tauri::{AppHandle, Runtime};

use crate::{Error, Events, LoginEvent, Result};

pub struct VkLogin<R: Runtime> {
    events: Events,
    _app: AppHandle<R>,
}

pub(crate) fn unsupported() -> Result<()> {
    Err(Error::Unsupported)
}

pub(crate) fn init<R: Runtime>(
    app: &AppHandle<R>,
    _api: PluginApi<R, ()>,
) -> std::result::Result<VkLogin<R>, Box<dyn std::error::Error>> {
    Ok(VkLogin { events: Events::default(), _app: app.clone() })
}

impl<R: Runtime> VkLogin<R> {
    /// Receives every event of every sign-in, on a plugin thread.
    pub fn set_handler(&self, handler: impl Fn(LoginEvent) + Send + Sync + 'static) {
        self.events.set(std::sync::Arc::new(handler));
    }

    /// Always `Err(Error::Unsupported)` off Android.
    pub fn start(&self) -> Result<()> {
        unsupported()
    }

    pub fn pick_local_audio(&self) -> Result<crate::PickedAudioBatch> { Err(Error::Unsupported) }

    pub fn pick_audio(&self) -> Result<Option<crate::PickedAudio>> { Err(Error::Unsupported) }

    /// Always `Err(Error::Unsupported)` off Android.
    pub fn cancel(&self) -> Result<()> {
        unsupported()
    }
}
