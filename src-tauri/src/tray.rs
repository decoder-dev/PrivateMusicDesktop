//! Значок на панели задач (строка меню на macOS): элементы управления транспортировкой и возврат к
//! окну, которое было закрыто во время воспроизведения музыки.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::AppState;

pub const TRAY_ID: &str = "main";

struct Labels {
    toggle: &'static str,
    next: &'static str,
    previous: &'static str,
    show: &'static str,
    quit: &'static str,
    like: &'static str,
    unlike: &'static str,
}

/// The "Add to My music" item; its text and state follow the current track (`set_liked`).
struct TrayLike(MenuItem<tauri::Wry>);

fn labels() -> Labels {
    let russian = sys_locale::get_locale().is_some_and(|l| l.to_ascii_lowercase().starts_with("ru"));
    if russian {
        Labels { toggle: "Играть / пауза", next: "Следующий", previous: "Предыдущий", show: "Открыть Private Music", quit: "Выйти", like: "Добавить в Мою музыку", unlike: "Убрать из Моей музыки" }
    } else {
        Labels { toggle: "Play / pause", next: "Next", previous: "Previous", show: "Open Private Music", quit: "Quit", like: "Add to My music", unlike: "Remove from My music" }
    }
}

/// Возвращает главное окно обратно (из трея, второй запуск, Док).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// `false`, когда на рабочем столе нет панели задач (некоторые сеансы Linux): при закрытии окна
/// необходимо завершить работу, иначе приложение продолжит работать без возможности возврата.
pub fn create(app: &AppHandle) -> bool {
    // Библиотека лотков Linux загружается во время выполнения и вызывает панику при ее отсутствии.
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(app)));
    match built {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            log::warn!("no tray icon: {error}");
            false
        }
        Err(_) => {
            log::warn!("no tray icon: the system tray library is not available");
            false
        }
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let text = labels();
    let toggle = MenuItem::with_id(app, "toggle", text.toggle, true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", text.next, true, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", text.previous, true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", text.show, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", text.quit, true, None::<&str>)?;
    let like = MenuItem::with_id(app, "like", text.like, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &next, &previous, &separator, &like, &separator2, &show, &quit])?;
    app.manage(TrayLike(like));

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Private Music")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let controller = |f: fn(&pm_engine::controller::Controller)| {
                if let Some(state) = app.try_state::<AppState>() {
                    f(state.app.controller());
                }
            };
            match event.id.as_ref() {
                "toggle" => controller(|c| c.toggle()),
                "next" => controller(|c| c.next()),
                "previous" => controller(|c| c.previous()),
                // The library belongs to the UI: it toggles the current track and answers with set_liked.
                "like" => {
                    let _ = tauri::Emitter::emit(app, "media-like", ());
                }
                "show" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// "Title — Artist" при наведении или название приложения, когда ничего не воспроизводится.
pub fn set_tooltip(app: &AppHandle, text: Option<&str>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text.unwrap_or("Private Music")));
    }
}

/// Like state of the current track: `None` disables the item (nothing playing, a local file).
pub fn set_liked(app: &AppHandle, liked: Option<bool>) {
    if let Some(item) = app.try_state::<TrayLike>() {
        let text = labels();
        let _ = item.0.set_text(if liked == Some(true) { text.unlike } else { text.like });
        let _ = item.0.set_enabled(liked.is_some());
    }
}
