//! The Android half: launches `LoginPlugin` (Kotlin) and forwards what it
//! reports through a native `Channel`, never through the web view.

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

use crate::{Error, Events, LoginEvent, Result, ANDROID_PACKAGE};

pub struct VkLogin<R: Runtime> {
    plugin: PluginHandle<R>,
    events: Events,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartArgs {
    on_event: Channel<InvokeResponseBody>,
}

/// `started` is a fresh sign-in; `running` means one is already on screen and
/// was only brought back to the front (its events still go to the first channel).
#[derive(Deserialize)]
struct StartReply {
    #[allow(dead_code)]
    status: String,
}

pub(crate) fn init<R: Runtime>(
    _app: &AppHandle<R>,
    api: PluginApi<R, ()>,
) -> std::result::Result<VkLogin<R>, Box<dyn std::error::Error>> {
    let plugin = api.register_android_plugin(ANDROID_PACKAGE, "LoginPlugin")?;
    Ok(VkLogin { plugin, events: Events::default() })
}

impl<R: Runtime> VkLogin<R> {
    /// Receives every event of every sign-in, on a plugin thread.
    pub fn set_handler(&self, handler: impl Fn(LoginEvent) + Send + Sync + 'static) {
        self.events.set(std::sync::Arc::new(handler));
    }

    /// Opens the VK sign-in screen (or brings an open one back and asks it to
    /// check the session now, like the desktop "return to sign-in" button).
    /// Returns as soon as the launch is under way; the outcome is an event.
    pub fn start(&self) -> Result<()> {
        let events = self.events.clone();
        let channel = Channel::new(move |body| {
            match body {
                InvokeResponseBody::Json(json) => events.dispatch_json(&json),
                InvokeResponseBody::Raw(_) => log::warn!("vk-login: ignored a binary message"),
            }
            Ok(())
        });
        let plugin = self.plugin.clone();
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("vk-login-start".into())
            .spawn(move || {
                // Kotlin answers when the Activity is gone, so this blocks that long.
                if let Err(error) = plugin.run_mobile_plugin::<StartReply>("start", StartArgs { on_event: channel }) {
                    log::warn!("vk-login: start failed: {error}");
                    events.dispatch(LoginEvent::Error { kind: "launch".into() });
                }
            })
            .map(|_| ())
            .map_err(|e| Error::Native(e.to_string()))
    }

    /// Closes the screen if it is open (the Activity reports `Cancelled`).
    pub fn cancel(&self) -> Result<()> {
        self.plugin
            .run_mobile_plugin::<serde_json::Value>("cancel", ())
            .map(|_| ())
            .map_err(|e| Error::Native(e.to_string()))
    }
}
