// Release builds on Windows should not open a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod login;
mod media;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Closing the window hides it (music keeps playing) instead of quitting.
    /// Only honoured when a tray icon exists to bring the window back.
    pub close_to_tray: Arc<AtomicBool>,
    pub has_tray: Arc<AtomicBool>,
    /// The sound output the user picked; `None` follows the system default.
    pub output_device: Arc<std::sync::Mutex<Option<String>>>,
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
        // First: a second launch hands over to the running app and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all() - tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .with_denylist(&["login"])
                .build(),
        )
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
            let tray_handle = handle.clone();
            let last_tooltip = std::sync::Mutex::new(None::<String>);
            let media: Arc<std::sync::OnceLock<Option<media::Media>>> = Arc::new(std::sync::OnceLock::new());
            let media_for_events = media.clone();
            let output_device: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
            let device_for_sink = output_device.clone();
            let app = App::new(
                VkClient::new().expect("the built-in API address is valid"),
                move || {
                    let name = device_for_sink.lock().unwrap_or_else(|p| p.into_inner()).clone();
                    CpalSink::open(name.as_deref()).map(|sink| Box::new(sink) as Box<dyn Sink>)
                },
                store,
                move |event| {
                    if let Some(Some(media)) = media_for_events.get() {
                        media.on_event(&event);
                    }
                    if let pm_engine::controller::UiEvent::State(state) = &event {
                        let text = state.track.as_ref().map(|t| format!("{} — {}", t.title, t.artist));
                        let mut last = last_tooltip.lock().unwrap_or_else(|p| p.into_inner());
                        if *last != text {
                            tray::set_tooltip(&tray_handle, text.as_deref());
                            *last = text;
                        }
                    }
                    let _ = emitter.emit("pm-event", &event);
                },
                AppOptions {
                    offline_dir: handle.path().app_data_dir().ok().map(|dir| dir.join("downloads")),
                    ..AppOptions::default()
                },
            );
            let has_tray = Arc::new(AtomicBool::new(false));
            let close_to_tray = Arc::new(AtomicBool::new(true));
            tauri_app.manage(AppState {
                app: Arc::new(app),
                login_running: Arc::new(AtomicBool::new(false)),
                close_to_tray: close_to_tray.clone(),
                has_tray: has_tray.clone(),
                output_device,
            });

            WebviewWindowBuilder::new(&handle, "main", WebviewUrl::App("index.html".into()))
                .title("Private Music")
                .inner_size(1120.0, 740.0)
                .min_inner_size(780.0, 540.0)
                .on_navigation(is_app_url)
                .build()?
                .on_window_event({
                    let window_handle = handle.clone();
                    let has_tray = has_tray.clone();
                    let close_to_tray = close_to_tray.clone();
                    move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            let hide = cfg!(target_os = "macos")
                                || (has_tray.load(Ordering::Relaxed) && close_to_tray.load(Ordering::Relaxed));
                            if hide {
                                api.prevent_close();
                                if let Some(window) = window_handle.get_webview_window("main") {
                                    let _ = window.hide();
                                }
                            } else {
                                window_handle.exit(0);
                            }
                        }
                    }
                });
            has_tray.store(tray::create(&handle), Ordering::Relaxed);
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
            commands::set_equalizer,
            commands::lyrics,
            commands::create_playlist,
            commands::edit_playlist,
            commands::delete_playlist,
            commands::add_to_playlist,
            commands::remove_from_playlist,
            commands::downloads,
            commands::download,
            commands::remove_download,
            commands::mix,
            commands::recommendations,
            commands::similar,
            commands::add_to_library,
            commands::remove_from_library,
            commands::artwork_palette,
            commands::set_fullscreen,
            commands::set_close_to_tray,
            commands::output_devices,
            commands::set_output_device,
            commands::app_info,
            commands::set_shuffle,
            commands::set_repeat,
            commands::jump,
            commands::play_next,
            commands::play_last,
            commands::remove_from_queue,
            commands::stop,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the application")
        .run(|app, event| {
            // macOS: clicking the Dock icon after the window was closed.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main(app);
            }
            let _ = (app, event);
        });
}
