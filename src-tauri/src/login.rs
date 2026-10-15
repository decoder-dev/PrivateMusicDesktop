//! Signing in to VK.
//!
//! A separate window shows vk.ru's own login page (so VK's password, 2FA and
//! captcha screens work as they do in a browser). The app never sees the
//! password: it watches that window's cookie jar, and once a session cookie
//! appears it asks VK for an API token with it, exactly like the iOS app.
//!
//! The window is isolated: private storage, no app capabilities, and it may
//! only navigate within vk.ru / vk.com over https.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pm_engine::vk::exchange_web_session;
use pm_engine::Error;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use crate::AppState;

const LOGIN_URL: &str = "https://vk.ru/login";
const COOKIE_URL: &str = "https://vk.ru/";
const POLL: Duration = Duration::from_millis(1500);
const SESSION_COOKIES: [&str; 3] = ["remixsid", "remixsid6", "remixnsid"];

/// The browser identity shown to VK, both by the sign-in window and by the
/// token exchange (VK ties the token to it).
#[cfg(target_os = "windows")]
pub const WEB_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
#[cfg(target_os = "macos")]
pub const WEB_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Safari/605.1.15";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const WEB_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn is_vk_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "vk.ru" || host.ends_with(".vk.ru") || host == "vk.com" || host.ends_with(".vk.com")
}

fn is_allowed_navigation(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str().map(is_vk_host).unwrap_or(false)
}

/// `name=value; …` for the cookies, or `None` while no session cookie exists.
fn session_header(cookies: &[(String, String)]) -> Option<String> {
    let signed_in = cookies
        .iter()
        .any(|(name, value)| SESSION_COOKIES.contains(&name.to_ascii_lowercase().as_str()) && !value.is_empty());
    signed_in.then(|| cookies.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; "))
}

pub fn start(handle: &AppHandle, state: &AppState) -> Result<(), String> {
    if let Some(existing) = handle.get_webview_window("login") {
        let _ = existing.set_focus();
        return Ok(());
    }
    if state.login_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let running = state.login_running.clone();
    let url: Url = LOGIN_URL.parse().map_err(|_| "bad login address".to_string())?;
    let window = WebviewWindowBuilder::new(handle, "login", WebviewUrl::External(url))
        .title("VK")
        .inner_size(520.0, 760.0)
        .user_agent(WEB_USER_AGENT)
        .incognito(true)
        .on_navigation(is_allowed_navigation)
        .build()
        .map_err(|e| {
            running.store(false, Ordering::SeqCst);
            e.to_string()
        })?;

    let handle = handle.clone();
    let app = state.app.clone();
    std::thread::spawn(move || {
        let outcome = poll(&handle, &window, &running);
        running.store(false, Ordering::SeqCst);
        match outcome {
            Some(Ok(session)) => {
                app.sign_in(session);
                let _ = handle.emit("pm-login", "success");
                let _ = window.close();
            }
            Some(Err(error)) => {
                let _ = handle.emit("pm-login", format!("error:{}", error.kind()));
                let _ = window.close();
            }
            None => {
                let _ = handle.emit("pm-login", "cancelled");
            }
        }
    });
    Ok(())
}

/// Waits for the user to finish signing in. `None` when the window was closed.
fn poll(
    handle: &AppHandle,
    window: &tauri::WebviewWindow,
    running: &Arc<AtomicBool>,
) -> Option<Result<pm_engine::vk::Session, Error>> {
    let cookie_url: Url = COOKIE_URL.parse().ok()?;
    let mut failures = 0;
    loop {
        std::thread::sleep(POLL);
        if handle.get_webview_window("login").is_none() || !running.load(Ordering::SeqCst) {
            return None;
        }
        let Ok(cookies) = window.cookies_for_url(cookie_url.clone()) else { continue };
        let pairs: Vec<(String, String)> = cookies.iter().map(|c| (c.name().to_string(), c.value().to_string())).collect();
        let Some(header) = session_header(&pairs) else { continue };
        match exchange_web_session(&header, WEB_USER_AGENT) {
            Ok(session) => return Some(Ok(session)),
            // A cookie exists but VK does not call it signed in yet (a 2FA or
            // captcha step): keep waiting.
            Err(Error::Unauthorized) => continue,
            Err(error) => {
                failures += 1;
                if failures >= 3 {
                    return Some(Err(error));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_vk_over_https_may_be_navigated_to() {
        for ok in ["https://vk.ru/login", "https://id.vk.com/auth", "https://login.vk.ru/?act=x", "https://VK.com/"] {
            assert!(is_allowed_navigation(&ok.parse().unwrap()), "{ok}");
        }
        for bad in [
            "http://vk.ru/login",
            "https://evilvk.ru/",
            "https://vk.ru.evil.example/",
            "https://example.com/?vk.ru",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://accounts.google.com/",
        ] {
            assert!(!is_allowed_navigation(&bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn a_session_cookie_is_required() {
        let anonymous = vec![("remixlang".to_string(), "0".to_string()), ("remixsid".to_string(), String::new())];
        assert_eq!(session_header(&anonymous), None);
        let signed_in = vec![("remixlang".to_string(), "0".to_string()), ("remixsid".to_string(), "abc".to_string())];
        assert_eq!(session_header(&signed_in).as_deref(), Some("remixlang=0; remixsid=abc"));
    }
}
