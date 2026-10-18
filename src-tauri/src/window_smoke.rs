//! Встроенная проверка оконного режима, используемая сборкой установщика Windows.
use tauri::Manager;
pub fn start(handle: &tauri::AppHandle, report: std::path::PathBuf) {
    let handle = handle.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let window = handle.get_webview_window("main").ok_or("no main window")?;
            for mode in ["medium", "maximized", "fullscreen", "medium"] {
                crate::commands::set_window_mode(window.clone(), mode.into())
                    .map_err(|_| format!("cannot set {mode}"))?;
                let mut arrived = false;
                for _ in 0..40 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
                    let maximized = window.is_maximized().map_err(|e| e.to_string())?;
                    arrived = match mode {
                        "fullscreen" => fullscreen,
                        "maximized" => maximized && !fullscreen,
                        _ => !maximized && !fullscreen,
                    };
                    if arrived {
                        break;
                    }
                }
                if !arrived {
                    return Err(format!("{mode} did not arrive"));
                }
                if mode == "medium" {
                    let size = window.inner_size().map_err(|e| e.to_string())?;
                    let scale = window.scale_factor().map_err(|e| e.to_string())?;
                    let width = f64::from(size.width) / scale;
                    let height = f64::from(size.height) / scale;
                    if !(779.0..=1122.0).contains(&width) || !(539.0..=742.0).contains(&height) {
                        return Err(format!("medium size was {width}x{height}"));
                    }
                }
                if window.is_resizable().map_err(|e| e.to_string())? {
                    return Err(format!("{mode} allowed edge resizing"));
                }
            }
            Ok(())
        })();
        let (message, code) = match result {
            Ok(()) => ("passed medium maximized fullscreen fixed-edges\n".into(), 0),
            Err(error) => (format!("failed {error}\n"), 1),
        };
        let _ = std::fs::write(report, message);
        handle.exit(code);
    });
}
