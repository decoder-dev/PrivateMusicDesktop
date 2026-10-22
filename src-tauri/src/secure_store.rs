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

/// Debug builds only (CI): checks of the Keystore plugin. It never touches a real stored session: when one
/// exists the write modes are skipped and say so.
///
/// * `""` / `"roundtrip"`: write, read back, delete.
/// * `"keep"`: write a probe and leave it (the update test installs a new APK over this one).
/// * `"verify"`: the probe left by `"keep"` must still decrypt (the Keystore key survived).
/// * `"clean"`: delete the probe.
///
/// The probe is not a session: the engine reads it at start, fails to parse it and counts it as signed out
/// without removing it.
#[cfg(debug_assertions)]
pub fn selftest(app: &AppHandle<Wry>, mode: &str) -> Result<&'static str> {
    const PROBE: &str = "pm-selftest-\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442}";
    let backend = KeystoreBackend { app: app.clone() };
    let fail = |text: &str| Err(Error::Transport(format!("secure store selftest: {text}")));
    match mode {
        "verify" => match backend.read()? {
            Some(value) if value == PROBE => Ok("ok"),
            Some(_) => fail("the stored value is not the probe"),
            None => fail("the probe is gone or no longer decrypts"),
        },
        "clean" => backend.remove().map(|()| "ok"),
        "" | "roundtrip" | "keep" => {
            if backend.read()?.is_some() {
                return Ok("skipped: a session is stored");
            }
            backend.write(PROBE)?;
            let back = backend.read();
            if mode == "keep" {
                return if back?.as_deref() == Some(PROBE) { Ok("ok") } else { fail("read a different value back") };
            }
            // Always clean up, even if the read failed.
            let removed = backend.remove();
            if back?.as_deref() != Some(PROBE) {
                return fail("read a different value back");
            }
            removed?;
            if backend.read()?.is_some() {
                return fail("the value survived the delete");
            }
            Ok("ok")
        }
        _ => fail("unknown mode"),
    }
}
