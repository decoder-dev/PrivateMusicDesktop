//! The tray (menu bar on macOS) icon: transport controls, and the way back to
//! a window that was closed while music plays.

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
}

fn labels() -> Labels {
    let russian = sys_locale::get_locale().is_some_and(|l| l.to_ascii_lowercase().starts_with("ru"));
    if russian {
        Labels { toggle: "Играть / пауза", next: "Следующий", previous: "Предыдущий", show: "Открыть Private Music", quit: "Выйти" }
    } else {
        Labels { toggle: "Play / pause", next: "Next", previous: "Previous", show: "Open Private Music", quit: "Quit" }
    }
}

/// Brings the main window back (from the tray, a second launch, the Dock).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// `false` when the desktop has no tray (some Linux sessions): then closing
/// the window must quit, or the app would keep running with no way back.
pub fn create(app: &AppHandle) -> bool {
    // The Linux tray library is loaded at run time and panics when missing.
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
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &next, &previous, &separator, &show, &quit])?;

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

/// "Title — Artist" on hover, or the app name when nothing plays.
pub fn set_tooltip(app: &AppHandle, text: Option<&str>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text.unwrap_or("Private Music")));
    }
}
