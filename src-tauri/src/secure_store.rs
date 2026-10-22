//! The signed-in session on Android: kept by the `secure-store` Kotlin plugin
//! (`SecureStorePlugin`), which encrypts it with an AES-256-GCM key that lives
//! in the Android Keystore and keeps only the ciphertext in the app's private
//! files directory. This module is the Rust end: a [`BlobBackend`] that calls
//! the plugin, so the engine's [`BackendStore`] can use it as a `SecretStore`.
//!
//! The plugin has no JavaScript-facing permissions: only Rust can reach it, so
//! the web UI can never read the session.

use pm_engine::store::{BackendStore, BlobBackend};
use pm_engine::{Error, Result};
use serde::Deserialize;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime, Wry};

const PLUGIN_NAME: &str = "secure-store";
/// Package and class of the Kotlin plugin (src-tauri/gen/android/app/src/main/java/...).
const PLUGIN_PACKAGE: &str = "com.dec.privatemusic2.android";
const PLUGIN_CLASS: &str = "SecureStorePlugin";

struct Plugin<R: Runtime>(PluginHandle<R>);

#[derive(Deserialize)]
struct Value {
    value: Option<String>,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .setup(|app, api| {
            let handle = api.register_android_plugin(PLUGIN_PACKAGE, PLUGIN_CLASS)?;
            app.manage(Plugin(handle));
            Ok(())
        })
        .build()
}

/// The `SecretStore` for the running app.
pub fn store(app: &AppHandle<Wry>) -> BackendStore<KeystoreBackend> {
    BackendStore::new(KeystoreBackend { app: app.clone() })
}

pub struct KeystoreBackend {
    app: AppHandle<Wry>,
}

impl KeystoreBackend {
    fn call<T: serde::de::DeserializeOwned>(&self, command: &str, payload: serde_json::Value) -> Result<T> {
        let plugin = self
            .app
            .try_state::<Plugin<Wry>>()
            .ok_or_else(|| Error::Transport("secret store: plugin not loaded".into()))?;
        // The message comes from the plugin and never contains the session.
        plugin.0.run_mobile_plugin(command, payload).map_err(|e| Error::Transport(format!("secret store: {e}")))
    }
}

impl BlobBackend for KeystoreBackend {
    fn read(&self) -> Result<Option<String>> {
        Ok(self.call::<Value>("get", serde_json::json!({}))?.value)
    }

    fn write(&self, text: &str) -> Result<()> {
        self.call::<serde_json::Value>("set", serde_json::json!({ "value": text })).map(|_| ())
    }

    fn remove(&self) -> Result<()> {
        self.call::<serde_json::Value>("delete", serde_json::json!({})).map(|_| ())
    }
}
