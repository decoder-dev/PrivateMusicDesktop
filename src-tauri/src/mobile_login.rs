//! Вход в VK на Android.
//!
//! На десктопе (`login.rs`) вход идёт во втором изолированном окне; у Tauri mobile одно WebView, поэтому
//! экран входа — отдельная Activity из плагина `tauri-plugin-vk-login` (приватный WebView, только
//! vk.ru / vk.com по https). Плагин сам обменивает cookie на `web_token` внутри своей браузерной сессии
//! (так 2FA и капча не теряют страницу) и присылает результат в Rust. Дальше тот же путь, что у десктопа:
//! `session_from_web_token` собирает `Session` (с `refresh_cookie` для обновления токена, как после
//! `exchange_web_session`), `App::sign_in` сохраняет её в Keystore и запускает движок.
//!
//! События плагина превращаются в те же строки события `pm-login`, что и на десктопе, поэтому UI
//! (`ui/js/views/login.js`) не меняется: `pending:…`, `success`, `error:<kind>`, `cancelled`,
//! `loadfailed`, `loadok`. Команды плагина вызываются только из Rust: веб-странице они недоступны
//! (в capabilities `vk-login` не выдаётся).

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_vk_login::{handle_event, LoginEvent, VkLoginExt};

use crate::AppState;

/// Событие, которое слушает UI.
pub const EVENT: &str = "pm-login";

/// Подключает обработчик событий плагина. Вызывается после `manage(AppState)`.
///
/// Плагин отдаёт события из Kotlin в потоке интерфейса Android (главном). Сохранение сессии идёт через плагин
/// Keystore, который отвечает тоже через главный поток: выполнять его прямо в обработчике значит ждать ответа,
/// который не может прийти, пока поток занят ожиданием (на телефоне это зависание «приложение не отвечает» сразу
/// после входа). Поэтому обработчик только кладёт событие в очередь, а работает отдельный поток, по порядку.
pub fn install(handle: &AppHandle) {
    let target = handle.clone();
    let (queue, events) = std::sync::mpsc::channel::<LoginEvent>();
    let worker = std::thread::Builder::new().name("vk-login-events".into()).spawn(move || {
        for event in events {
            process(&target, event);
        }
    });
    if let Err(error) = worker {
        log::error!("sign-in (android): could not start the event thread: {error}");
        return;
    }
    let queue = std::sync::Mutex::new(queue);
    handle.vk_login().set_handler(move |event| {
        if let Ok(queue) = queue.lock() {
            let _ = queue.send(event);
        }
    });
}

fn process(target: &AppHandle, event: LoginEvent) {
    // В журнал попадает только имя события: cookie и токены там не бывают.
    let name = match &event {
        LoginEvent::Pending { .. } => "pending",
        LoginEvent::Success(_) => "success",
        LoginEvent::Error { .. } => "error",
        LoginEvent::Cancelled => "cancelled",
        LoginEvent::LoadFailed => "loadfailed",
        LoginEvent::LoadOk | LoginEvent::SmokeLoaded { .. } => "loadok",
    };
    log::info!("sign-in (android): event={name}");
    let message = handle_event(event, |session| {
        if let Some(state) = target.try_state::<AppState>() {
            state.app.sign_in(session);
            log::info!("sign-in (android): session handed to the application");
        }
    });
    log::info!("sign-in (android): result={message}");
    let _ = target.emit(EVENT, message);
}

/// Открывает экран входа (или возвращает открытый и просит проверить сессию сейчас). Возвращается сразу;
/// результат приходит событием `pm-login`.
///
/// `reset` на Android не нужен: экран входа при каждом открытии стирает cookie, кэш и веб-хранилище
/// (`BrowsingData`), то есть всегда начинает с чистой браузерной сессии.
pub fn start(handle: &AppHandle, _state: &AppState, reset: bool) -> Result<(), String> {
    log::info!("sign-in requested (android): reset={reset}");
    handle.vk_login().start().map_err(|e| e.to_string())
}
