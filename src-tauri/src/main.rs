// Релизные сборки для Windows не должны открывать окно консоли.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod login;
mod media;
mod tray;
mod updater;
mod window_smoke;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{CpalSink, Sink};
use pm_engine::store::{KeyringStore, SecretStore};
use pm_engine::vk::VkClient;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const KEYRING_SERVICE: &str = "com.dec.privatemusic2.desktop";

pub struct AppState {
    pub app: Arc<App>,
    pub login_running: Arc<AtomicBool>,
    pub login_check: Arc<AtomicBool>,
    /// Закрытие окна скрывает его (музыка продолжает играть), а не закрывается. Соблюдается только при
    /// наличии значка на панели задач, позволяющего вернуть окно.
    pub close_to_tray: Arc<AtomicBool>,
    pub has_tray: Arc<AtomicBool>,
    /// Звуковой вывод, выбранный пользователем; `None` соответствует системному значению по умолчанию.
    pub output_device: Arc<std::sync::Mutex<Option<String>>>,
}

/// В главном окне хостов разрешено показывать: только UI в комплекте. Ссылка или скрипт ни в коем
/// случае не должны переносить окно приложения на другой сайт.
fn is_app_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "tauri" | "asset")
        || matches!(url.host_str(), Some("tauri.localhost" | "localhost" | "127.0.0.1")) && cfg!(debug_assertions)
        || url.host_str() == Some("tauri.localhost")
}

/// Логи идут в `<app log dir>/private-music.log` (перезаписываются при каждом запуске), так что о
/// проблеме на чьей-то машине можно прочитать потом. `RUST_LOG` переопределяет уровень.
fn init_logging(handle: &tauri::AppHandle) {
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,private_music_desktop=info,pm_engine=info"),
    );
    if let Ok(dir) = handle.path().app_log_dir() {
        if std::fs::create_dir_all(&dir).is_ok() {
            if let Ok(file) = std::fs::File::create(dir.join("private-music.log")) {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
            }
        }
    }
    let _ = builder.try_init();
}

/// CI: откройте окно входа так же, как это делает кнопка (из асинхронной среды выполнения, а не из
/// основного потока) и сообщите, загружается ли страница VK.
fn start_login_smoke_test(handle: &tauri::AppHandle, report: std::path::PathBuf) {
    let _ = login::SMOKE_REPORT.set(report.clone());
    let opener = handle.clone();
    tauri::async_runtime::spawn(async move {
        let state = opener.state::<AppState>();
        if let Err(error) = login::start(&opener, &state) {
            let _ = std::fs::write(&report, format!("error {error}\n"));
            opener.exit(1);
        }
    });
    let timer = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(90));
        if let Some(report) = login::SMOKE_REPORT.get() {
            if !report.exists() {
                let _ = std::fs::write(report, "timeout\n");
            }
        }
        timer.exit(2);
    });
}

fn main() {
    tauri::Builder::default()
        // Первый: второй запуск передает работающему приложению и завершает работу.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // Восстановите размещение и максимизацию рабочего стола, сохранив фиксированные
                // настройки и исходный кадр.
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::MAXIMIZED)
                .with_denylist(&["login"])
                .build(),
        )
        .setup(|tauri_app| {
            let handle = tauri_app.handle().clone();
            init_logging(&handle);
            tauri_app.manage(updater::UpdateState(Arc::new(pm_engine::update::Updater::new(
                handle.path().app_cache_dir()?.join("updates"),
                &handle.package_info().version.to_string(),
            ))));
            log::info!("Private Music {} starting", handle.package_info().version);

            let store: Box<dyn SecretStore> = Box::new(KeyringStore::new(KEYRING_SERVICE)
                .with_logout_marker(handle.path().app_data_dir()?.join("session.signed-out")));

            let emitter = handle.clone();
            let tray_handle = handle.clone();
            let mut last_tooltip = None::<String>;
            let media: Arc<std::sync::OnceLock<Option<media::Media>>> = Arc::new(std::sync::OnceLock::new());
            let media_for_events = media.clone();
            let system_updates = pm_engine::media_sync::StatePublisher::spawn(move |state| {
                if let Some(Some(media)) = media_for_events.get() {
                    media.on_event(&pm_engine::controller::UiEvent::State(state.clone()));
                }
                let text = state.track.as_ref().map(|t| format!("{} — {}", t.title, t.artist));
                if last_tooltip != text {
                    tray::set_tooltip(&tray_handle, text.as_deref());
                    last_tooltip = text;
                }
            })
            .map_err(|error| log::warn!("system media worker unavailable: {error}"))
            .ok();
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
                    if let pm_engine::controller::UiEvent::State(state) = &event {
                        if let Some(updates) = &system_updates {
                            updates.publish(state.clone());
                        }
                    }
                    let _ = emitter.emit("pm-event", &event);
                },
                AppOptions {
                    offline_dir: handle.path().app_data_dir().ok().map(|dir| dir.join("downloads")),
                    history_dir: handle.path().app_data_dir().ok().map(|dir| dir.join("history")),
                    ..AppOptions::default()
                },
            );
            let has_tray = Arc::new(AtomicBool::new(false));
            let close_to_tray = Arc::new(AtomicBool::new(true));
            tauri_app.manage(AppState {
                app: Arc::new(app),
                login_running: Arc::new(AtomicBool::new(false)),
                login_check: Arc::new(AtomicBool::new(false)),
                close_to_tray: close_to_tray.clone(),
                has_tray: has_tray.clone(),
                output_device,
            });

            WebviewWindowBuilder::new(&handle, "main", WebviewUrl::App("index.html".into()))
                .devtools(false)
                .initialization_script("document.addEventListener('contextmenu', e => e.preventDefault(), true);")
                .title("Private Music")
                .inner_size(1120.0, 740.0)
                .min_inner_size(780.0, 540.0)
                .on_navigation(is_app_url)
                .resizable(false)
                .maximizable(true)
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
            // После того, как окно существует: Windows нужен его дескриптор.
            let _ = media.set(media::Media::new(&handle));
            if let Some(report) = std::env::var_os("PM_SMOKE_WINDOW") {
                window_smoke::start(&handle, report.into());
            }
            if let Some(report) = std::env::var_os("PM_SMOKE_LOGIN") {
                start_login_smoke_test(&handle, report.into());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updater::update_state,
            updater::update_check,
            updater::update_download,
            updater::update_cancel,
            updater::update_access,
            updater::update_open,
            commands::set_window_mode,
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
            commands::mix_settings,
            commands::catalog_mixes,
            commands::stream_mix,
            commands::new_releases,
            commands::play_radio,
            commands::recommendations,
            commands::similar,
            commands::add_to_library,
            commands::remove_from_library,
            commands::artwork_palette,
            commands::set_fullscreen,
            commands::set_close_to_tray,
            commands::network_focus,
            commands::network_check,
            commands::retry_playback,
            commands::output_devices,
            commands::set_output_device,
            commands::app_info,
            commands::set_shuffle,
            commands::set_repeat,
            commands::jump,
            commands::play_next,
            commands::play_last,
            commands::remove_from_queue,
            commands::clear_upcoming,
            commands::move_upcoming,
            commands::set_sleep_timer,
            commands::sleep_timer,
            commands::export_track,
            commands::stop,
            commands::search_artists,
            commands::find_artist,
            commands::artist_tracks,
            commands::artist_albums,
            commands::search_albums,
            commands::followed_albums,
            commands::album_tracks,
            commands::playlist_by_id,
            commands::follow_album,
            commands::friends,
            commands::friend_tracks,
            commands::friend_playlists,
            commands::history,
            commands::history_remove,
            commands::history_clear,
            commands::listening_stats,
            commands::reset_listening_stats,
            commands::set_utc_offset,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the application")
        .run(|app, event| {
            // macOS: щелчок по значку в доке после закрытия окна.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main(app);
            }
            if let tauri::RunEvent::Exit = event {
                // Все, что было воспроизведено с момента последнего сохранения, сохраняется на диске.
                if let Some(state) = app.try_state::<AppState>() {
                    state.app.retry_session_storage();
                    state.app.flush_history();
                }
            }
            let _ = (app, event);
        });
}
