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

    pub fn network_usage(&self) -> Result<serde_json::Value> {
        self.plugin.run_mobile_plugin("networkUsage", ())
            .map_err(|error| Error::Native(error.to_string()))
    }

    /// Device, WebView and process-death report from the Android side (see `CrashReport.kt`).
    pub fn crash_report(&self) -> Result<String> {
        let reply: serde_json::Value = self.plugin.run_mobile_plugin("crashReport", ())
            .map_err(|error| Error::Native(error.to_string()))?;
        Ok(reply.get("text").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned())
    }

    pub fn share_audio(&self, path: &str) -> Result<()> {
        let _: serde_json::Value = self.plugin.run_mobile_plugin("shareAudio", serde_json::json!({"path":path}))
            .map_err(|error| Error::Native(error.to_string()))?;
        Ok(())
    }

    pub fn share_link(&self, url: &str, title: &str) -> Result<()> {
        self.plugin.run_mobile_plugin::<serde_json::Value>("shareLink", serde_json::json!({"url":url,"title":title}))
            .map(|_|()).map_err(|error|Error::Native(error.to_string()))
    }

    pub fn copy_link(&self, url: &str) -> Result<()> {
        self.plugin.run_mobile_plugin::<serde_json::Value>("copyLink", serde_json::json!({"url":url}))
            .map(|_|()).map_err(|error|Error::Native(error.to_string()))
    }

    pub fn cover_selftest(&self) -> Result<String> {
        let reply:serde_json::Value=self.plugin.run_mobile_plugin("coverSelfTest",())
            .map_err(|error|Error::Native(error.to_string()))?;
        reply.get("status").and_then(serde_json::Value::as_str).map(str::to_owned)
            .ok_or_else(||Error::Native("invalid cover self-test reply".into()))
    }

    pub fn pick_cover(&self) -> Result<Option<String>> {
        let reply: serde_json::Value = self.plugin.run_mobile_plugin("pickCover", ())
            .map_err(|error| Error::Native(error.to_string()))?;
        if reply.get("cancelled").and_then(serde_json::Value::as_bool)==Some(true) {return Ok(None);}
        reply.get("jpeg").and_then(serde_json::Value::as_str).filter(|value|value.len()<=3_000_000)
            .map(|value|Some(value.to_owned())).ok_or_else(||Error::Native("invalid image picker reply".into()))
    }

    pub fn pick_audio(&self) -> Result<Option<crate::PickedAudio>> {
        let reply: serde_json::Value = self.plugin.run_mobile_plugin("pickAudio", ())
            .map_err(|error| Error::Native(error.to_string()))?;
        if reply.get("cancelled").and_then(serde_json::Value::as_bool) == Some(true) { return Ok(None); }
        serde_json::from_value(reply).map(Some).map_err(|_| Error::Native("invalid audio picker reply".into()))
    }

    /// Closes the screen if it is open (the Activity reports `Cancelled`).
    pub fn cancel(&self) -> Result<()> {
        self.plugin
            .run_mobile_plugin::<serde_json::Value>("cancel", ())
            .map(|_| ())
            .map_err(|e| Error::Native(e.to_string()))
    }
}
