// Release builds on Windows should not open a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod login;
mod media;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{CpalSink, Sink};
use pm_engine::store::{KeyringStore, MemoryStore, SecretStore};
use pm_engine::vk::VkClient;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const KEYRING_SERVICE: &str = "com.dec.privatemusic2.desktop";

pub struct AppState {
    pub app: Arc<App>,
    pub login_running: Arc<AtomicBool>,
}

/// Hosts the main window is allowed to show: the bundled UI only. A link or
/// a script must never be able to take the app window to another site.
fn is_app_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "tauri" | "asset")
        || matches!(url.host_str(), Some("tauri.localhost" | "localhost" | "127.0.0.1")) && cfg!(debug_assertions)
        || url.host_str() == Some("tauri.localhost")
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    tauri::Builder::default()
        .setup(|tauri_app| {
            let handle = tauri_app.handle().clone();

            let store: Box<dyn SecretStore> = Box::new(KeyringStore::new(KEYRING_SERVICE));
            // If the OS secret store cannot be used at all, keep the sign-in
            // in memory for this run instead of writing credentials to disk.
            let store: Box<dyn SecretStore> = match store.load() {
                Ok(_) => store,
                Err(error) => {
                    log::warn!("secret store unavailable, sign-in will not persist: {error}");
                    Box::new(MemoryStore::default())
                }
            };

            let emitter = handle.clone();
            let media: Arc<std::sync::OnceLock<Option<media::Media>>> = Arc::new(std::sync::OnceLock::new());
            let media_for_events = media.clone();
            let app = App::new(
                VkClient::new().expect("the built-in API address is valid"),
                || CpalSink::open_default().map(|sink| Box::new(sink) as Box<dyn Sink>),
                store,
                move |event| {
                    if let Some(Some(media)) = media_for_events.get() {
                        media.on_event(&event);
                    }
                    let _ = emitter.emit("pm-event", &event);
                },
                AppOptions::default(),
            );
            tauri_app.manage(AppState { app: Arc::new(app), login_running: Arc::new(AtomicBool::new(false)) });

            WebviewWindowBuilder::new(&handle, "main", WebviewUrl::App("index.html".into()))
                .title("Private Music")
                .inner_size(1120.0, 740.0)
                .min_inner_size(780.0, 540.0)
                .on_navigation(is_app_url)
                .build()?;
            // After the window exists: Windows needs its handle.
            let _ = media.set(media::Media::new(&handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::session_state,
            commands::profile,
            commands::login_start,
            commands::logout,
            commands::library,
            commands::playlists,
            commands::playlist_tracks,
            commands::search,
            commands::player_state,
            commands::play_tracks,
            commands::load_queue,
            commands::toggle,
            commands::pause,
            commands::resume,
            commands::next,
            commands::previous,
            commands::seek,
            commands::set_volume,
            commands::set_visualizer,
            commands::artwork_palette,
            commands::set_fullscreen,
            commands::set_shuffle,
            commands::set_repeat,
            commands::jump,
            commands::play_next,
            commands::play_last,
            commands::remove_from_queue,
            commands::stop,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the application");
}
