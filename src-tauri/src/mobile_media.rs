//! Фоновое воспроизведение на Android: состояние движка уходит в плагин `tauri-plugin-media-session`
//! (foreground-сервис, MediaSession, уведомление, аудиофокус), а кнопки системы — обратно в движок.

use std::sync::Mutex;
use std::time::Instant;

use pm_engine::media_sync::StatePublisher;
use tauri::{AppHandle, Manager};
use tauri_plugin_media_session::{bridge, bridge::Bridge, AppSink, MediaSessionExt};

use crate::AppState;

/// Рабочий поток, который передаёт состояния плееру системы. Как и на десктопе, UI-события не ждут его.
pub fn publisher(handle: &AppHandle) -> Option<StatePublisher> {
    let started = Instant::now();
    let bridge = Mutex::new(Bridge::new(AppSink(handle.clone())));
    StatePublisher::spawn(move |state| {
        let mut bridge = bridge.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(error) = bridge.on_state(&state, started.elapsed().as_secs_f64()) {
            log::debug!("media session: {error}");
        }
    })
    .map_err(|error| log::warn!("system media worker unavailable: {error}"))
    .ok()
}

/// Кнопки уведомления, гарнитуры и аудиофокус управляют движком так же, как медиаклавиши на десктопе.
/// Вызывается после `manage(AppState)` и до первого обновления.
pub fn attach_actions(handle: &AppHandle) {
    // Кнопки уведомления, гарнитуры и аудиофокус приходят из Kotlin в главном потоке Android. Операции
    // контроллера могут ждать блокировку движка (и сеть при переходе к следующему треку), а движок в это время
    // может ждать главный поток (обновление уведомления): выполнять их прямо здесь значит рисковать
    // «приложение не отвечает». Обработчик только ставит действие в очередь; выполняет отдельный поток по порядку.
    let (queue, actions) = std::sync::mpsc::channel();
    let target = handle.clone();
    let worker = std::thread::Builder::new().name("media-actions".into()).spawn(move || {
        let ducker = bridge::Ducker::default();
        for action in actions {
            if let Some(state) = target.try_state::<AppState>() {
                bridge::dispatch(state.app.controller(), &ducker, action);
            }
        }
    });
    if let Err(error) = worker {
        log::warn!("media session: action thread unavailable: {error}");
        return;
    }
    let queue = Mutex::new(queue);
    let result = handle.media_session().set_action_handler(move |action| {
        if let Ok(queue) = queue.lock() {
            let _ = queue.send(action);
        }
    });
    if let Err(error) = result {
        log::warn!("media session: actions unavailable: {error}");
    }
}

pub fn stop(handle: &AppHandle) {
    let _ = handle.media_session().stop();
}
