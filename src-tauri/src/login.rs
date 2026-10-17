//! Signing in to VK.
//!
//! A separate window shows vk.ru's own login page (so VK's password, 2FA and
//! captcha screens work as they do in a browser). The app never sees the
//! password: it watches that window's cookie jar, and once a session cookie
//! appears it asks VK for an API token with it, exactly like the iOS app.
//!
//! The window is isolated: private storage, no app capabilities, and it may
//! only navigate within vk.ru / vk.com over https.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use pm_engine::vk::exchange_web_session;
use pm_engine::Error;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use crate::AppState;

/// Set by CI: verify both page loading and private cookie access, then quit.
pub static SMOKE_REPORT: OnceLock<PathBuf> = OnceLock::new();
static SMOKE_PAGE_LOADED: AtomicBool = AtomicBool::new(false);

// Did VK's own page arrive? A blocked or unreachable VK (a VPN that is on, or
// should be off) leaves the window blank or on the browser's error page, and
// the web view reports no failure of its own.
static PAGE_LOADED: AtomicBool = AtomicBool::new(false);
static PAGE_ERROR: AtomicBool = AtomicBool::new(false);
/// The window was reported as not loading; the next "sign in" press reloads it.
static LOAD_FAILED: AtomicBool = AtomicBool::new(false);
/// Tells the watcher that the page was asked for again: its clock restarts.
static LOAD_RESTARTED: AtomicBool = AtomicBool::new(false);

/// How long VK's login page may take before the window is called stuck.
const LOAD_TIMEOUT: Duration = Duration::from_secs(25);
/// A blank page can be the web view's own first one: only a non-VK page that
/// is still what we see after this long is taken for an error page.
const ERROR_PAGE_GRACE: Duration = Duration::from_secs(4);

const LOGIN_URL: &str = "https://vk.ru/login";
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
    match url.scheme() {
        "https" => url.host_str().map(is_vk_host).unwrap_or(false),
        // Blank frames VK's page creates for itself carry no content.
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        // Object URLs made by VK's own page: blob:https://id.vk.ru/<uuid>.
        "blob" => Url::parse(url.path()).is_ok_and(|inner| is_allowed_navigation(&inner) && inner.scheme() == "https"),
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LoadVerdict {
    Waiting,
    Failed,
}

/// The login page counts as not loading when VK's page has not finished after
/// [`LOAD_TIMEOUT`], or a page other than VK's finished (the browser's own
/// error page, which this window refuses to navigate to anyway).
fn load_verdict(elapsed: Duration, loaded: bool, error_page: bool) -> LoadVerdict {
    if loaded {
        LoadVerdict::Waiting
    } else if (error_page && elapsed >= ERROR_PAGE_GRACE) || elapsed >= LOAD_TIMEOUT {
        LoadVerdict::Failed
    } else {
        LoadVerdict::Waiting
    }
}

fn reset_load_state() {
    PAGE_LOADED.store(false, Ordering::SeqCst);
    PAGE_ERROR.store(false, Ordering::SeqCst);
    LOAD_FAILED.store(false, Ordering::SeqCst);
    LOAD_RESTARTED.store(true, Ordering::SeqCst);
}

/// Automatic checks wait for a known session cookie; manual completion lets
/// VK validate any nonempty cookie set, as the iOS client's finish button does.
fn session_header(cookies: &[(String, String)], manual: bool) -> Option<String> {
    let signed_in = cookies
        .iter()
        .any(|(name, value)| SESSION_COOKIES.contains(&name.to_ascii_lowercase().as_str()) && !value.is_empty());
    (signed_in || (manual && !cookies.is_empty()))
        .then(|| cookies.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; "))
}

/// Read the whole private cookie store: looking up just vk.ru/ drops
/// host-only cookies set by login.vk.ru, where web_token is requested.
/// Never forward cookies from VK ID, vk.com or an unrelated site to it.
fn token_cookies(cookies: &[tauri::webview::Cookie<'_>]) -> Vec<(String, String)> {
    let mut cookies: Vec<_> = cookies.iter().filter(|cookie| {
        let domain = cookie.domain().unwrap_or("").trim_start_matches('.');
        (domain.eq_ignore_ascii_case("vk.ru") || domain.eq_ignore_ascii_case("login.vk.ru"))
            && cookie.path().unwrap_or("/") == "/"
    }).collect();
    // As in a browser, prefer the host-specific value when names collide.
    cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.domain().unwrap_or("").len()));
    let mut seen = std::collections::HashSet::new();
    cookies.into_iter().filter(|cookie| seen.insert(cookie.name().to_string()))
        .map(|cookie| (cookie.name().to_string(), cookie.value().to_string())).collect()
}

pub fn start(handle: &AppHandle, state: &AppState) -> Result<(), String> {
    if let Some(existing) = handle.get_webview_window("login") {
        if LOAD_FAILED.load(Ordering::SeqCst) {
            // The page never arrived: "Retry" loads it again, in the same
            // window and with the same restrictions.
            log::info!("sign-in window: reloading the login page");
            reset_load_state();
            if let Ok(url) = LOGIN_URL.parse::<Url>() {
                let _ = existing.navigate(url);
            }
        }
        // Explicit completion also works if VK changes its session-cookie names.
        // Only VK's token endpoint decides whether these cookies authenticate.
        state.login_check.store(true, Ordering::SeqCst);
        let _ = existing.unminimize();
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    if state.login_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let running = state.login_running.clone();
    let check = state.login_check.clone();
    check.store(false, Ordering::SeqCst);
    reset_load_state();
    let url: Url = LOGIN_URL.parse().map_err(|_| "bad login address".to_string())?;
    let window = WebviewWindowBuilder::new(handle, "login", WebviewUrl::External(url))
        .title("VK")
        .inner_size(520.0, 760.0)
        .user_agent(WEB_USER_AGENT)
        .incognito(true)
        .devtools(false)
        .initialization_script("document.addEventListener('contextmenu', event => event.preventDefault(), true);")
        .on_navigation(|url| {
            let allowed = is_allowed_navigation(url);
            if !allowed {
                // Only the host: sign-in addresses can carry one-time codes.
                log::warn!("sign-in window: blocked a {} page on {:?}", url.scheme(), url.host_str());
            }
            allowed
        })
        .on_page_load(|webview, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let host = payload.url().host_str().unwrap_or("").to_string();
            log::info!("sign-in window: page loaded on {host}");
            if payload.url().scheme() == "https" && is_vk_host(&host) {
                PAGE_LOADED.store(true, Ordering::SeqCst);
                PAGE_ERROR.store(false, Ordering::SeqCst);
                if LOAD_FAILED.swap(false, Ordering::SeqCst) {
                    // It got through after all (the VPN was switched meanwhile).
                    let _ = webview.app_handle().emit("pm-login", "loadok");
                }
            } else if !PAGE_LOADED.load(Ordering::SeqCst) {
                // Not VK's page: the engine's error page, or a blank one.
                PAGE_ERROR.store(true, Ordering::SeqCst);
            }
            if SMOKE_REPORT.get().is_some() && is_vk_host(&host) {
                // The worker must also prove that private cookies are readable.
                SMOKE_PAGE_LOADED.store(true, Ordering::SeqCst);
            }
        })
        .build()
        .map_err(|e| {
            running.store(false, Ordering::SeqCst);
            e.to_string()
        })?;

    let handle = handle.clone();
    let app = state.app.clone();
    std::thread::spawn(move || {
        let outcome = poll(&handle, &window, &running, &check);
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
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Waits for the user to finish signing in. `None` when the window was closed.
fn poll(
    handle: &AppHandle,
    window: &tauri::WebviewWindow,
    running: &Arc<AtomicBool>,
    check: &Arc<AtomicBool>,
) -> Option<Result<pm_engine::vk::Session, Error>> {
    let mut failures = 0;
    let mut last_header = None;
    let mut load_started = std::time::Instant::now();
    loop {
        std::thread::sleep(POLL);
        if handle.get_webview_window("login").is_none() || !running.load(Ordering::SeqCst) {
            return None;
        }
        if LOAD_RESTARTED.swap(false, Ordering::SeqCst) {
            load_started = std::time::Instant::now();
        }
        if !LOAD_FAILED.load(Ordering::SeqCst)
            && SMOKE_REPORT.get().is_none()
            && load_verdict(
                load_started.elapsed(),
                PAGE_LOADED.load(Ordering::SeqCst),
                PAGE_ERROR.load(Ordering::SeqCst),
            ) == LoadVerdict::Failed
        {
            // Tell the window: it shows the VPN hint and a Retry button, and
            // this window stays open for the retry.
            log::warn!("sign-in window: the VK login page did not load");
            LOAD_FAILED.store(true, Ordering::SeqCst);
            let _ = handle.emit("pm-login", "loadfailed");
        }
        if SMOKE_REPORT.get().is_some() {
            // CI only probes the browser, never VK's token endpoint, including
            // while the initial page is still loading.
            if !SMOKE_PAGE_LOADED.load(Ordering::SeqCst) { continue; }
            let verified = smoke_cookie_store(window);
            if let Some(report) = SMOKE_REPORT.get() {
                let _ = std::fs::write(report, if verified {
                    "loaded VK; private cookie store verified\n"
                } else {
                    "error private cookie store probe failed\n"
                });
            }
            handle.exit(if verified { 0 } else { 1 });
            return None;
        }
        let cookies = match window.cookies() {
            Ok(cookies) => cookies,
            Err(_) => {
                log::warn!("sign-in window: could not read the private cookie store");
                return Some(Err(Error::Transport("sign-in cookie store unavailable".into())));
            }
        };
        let pairs = token_cookies(&cookies);
        let manual = check.swap(false, Ordering::SeqCst);
        let Some(header) = session_header(&pairs, manual) else {
            if manual { let _ = handle.emit("pm-login", "pending:unauthorized"); }
            continue;
        };
        if last_header.as_ref() != Some(&header) {
            failures = 0;
            last_header = Some(header.clone());
            log::info!("sign-in window: session detected, exchanging token");
        }
        match exchange_web_session(&header, WEB_USER_AGENT) {
            Ok(session) => {
                // Closing the window during the HTTP request cancels adoption.
                if handle.get_webview_window("login").is_none() || !running.load(Ordering::SeqCst) {
                    return None;
                }
                log::info!("sign-in window: token exchange succeeded");
                return Some(Ok(session));
            }
            // A cookie exists but VK does not call it signed in yet (a 2FA or
            // captcha step): keep waiting.
            Err(Error::Unauthorized) => {
                failures += 1;
                if failures == 3 || manual {
                    log::warn!("sign-in window: VK has not confirmed the web session");
                    let _ = handle.emit("pm-login", "pending:unauthorized");
                }
                // Keep the browser session for 2FA, without hammering VK.
                std::thread::sleep(Duration::from_secs(5));
            }
            Err(error) => {
                failures += 1;
                if failures >= 3 {
                    return Some(Err(error));
                }
            }
        }
    }
}

/// Exercises the actual native cookie manager in the isolated CI window.
/// No credentials or token requests are involved.
fn smoke_cookie_store(window: &tauri::WebviewWindow) -> bool {
    use tauri::webview::Cookie;
    let probes = [
        Cookie::build(("pm_probe_browser", "browser")).domain(".vk.ru").path("/").secure(true).build(),
        Cookie::build(("pm_probe_login", "login")).domain("login.vk.ru").path("/").secure(true).http_only(true).build(),
    ];
    // One cookie comes from page JavaScript, the other from the native manager.
    // A native-only round trip would miss a manager reading the wrong profile.
    for cookie in &probes[1..] {
        if window.set_cookie(cookie.clone()).is_err() { return false; }
    }
    let mut verified = false;
    for _ in 0..10 {
        let _ = window.eval("document.cookie = 'pm_probe_browser=browser; Domain=vk.ru; Path=/; Secure; SameSite=Lax'");
        if let Ok(cookies) = window.cookies() {
            let pairs = token_cookies(&cookies);
            verified = probes.iter().all(|probe| pairs.iter().any(|(name, value)| name == probe.name() && value == probe.value()));
            if verified { break; }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    for cookie in probes { let _ = window.delete_cookie(cookie); }
    verified
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_vk_over_https_may_be_navigated_to() {
        for ok in [
            "https://vk.ru/login",
            "https://id.vk.com/auth",
            "https://login.vk.ru/?act=x",
            "https://VK.com/",
            "about:blank",
            "blob:https://id.vk.ru/6b1f0f7e-0000-4000-8000-000000000000",
        ] {
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
            "about:config",
            "blob:https://evil.example/123",
            "blob:http://vk.ru/123",
            "data:text/html,<script>1</script>",
        ] {
            assert!(!is_allowed_navigation(&bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn a_login_page_that_does_not_arrive_is_reported() {
        let secs = Duration::from_secs;
        assert_eq!(load_verdict(secs(3), false, false), LoadVerdict::Waiting);
        assert_eq!(load_verdict(secs(24), false, false), LoadVerdict::Waiting);
        assert_eq!(load_verdict(secs(25), false, false), LoadVerdict::Failed);
        assert_eq!(load_verdict(secs(2), false, true), LoadVerdict::Waiting, "the web view's first blank page is not an error");
        assert_eq!(load_verdict(secs(5), false, true), LoadVerdict::Failed, "an error page is not waited out");
        assert_eq!(load_verdict(secs(600), true, false), LoadVerdict::Waiting, "a loaded page is never a failure");
        assert_eq!(load_verdict(secs(600), true, true), LoadVerdict::Waiting);
    }

    #[test]
    fn a_session_cookie_is_required() {
        let anonymous = vec![("remixlang".to_string(), "0".to_string()), ("remixsid".to_string(), String::new())];
        assert_eq!(session_header(&anonymous, false), None);
        assert!(session_header(&anonymous, true).is_some(), "manual completion lets VK validate an unrecognized session");
        assert_eq!(session_header(&[], true), None);
        let signed_in = vec![("remixlang".to_string(), "0".to_string()), ("remixsid".to_string(), "abc".to_string())];
        assert_eq!(session_header(&signed_in, false).as_deref(), Some("remixlang=0; remixsid=abc"));
    }

    #[test]
    fn token_exchange_keeps_login_host_cookies_and_excludes_other_hosts() {
        use tauri::webview::Cookie;
        let cookie = |name, value, domain| Cookie::build((name, value)).domain(domain).path("/").build();
        let cookies = vec![
            cookie("remixsid", "root", ".vk.ru"),
            cookie("web_token", "host-only", "login.vk.ru"),
            cookie("id_session", "private", "id.vk.ru"),
            cookie("other", "private", ".vk.com"),
            cookie("foreign", "private", "vk.ru.evil.example"),
        ];
        let header = session_header(&token_cookies(&cookies), false).unwrap();
        assert!(header.contains("remixsid=root"));
        assert!(header.contains("web_token=host-only"));
        assert!(!header.contains("private"));
        let scoped = vec![
            cookie("remixsid", "root", ".vk.ru"),
            cookie("remixsid", "login", "login.vk.ru"),
            Cookie::build(("wrong_path", "private")).domain("vk.ru").path("/login").build(),
        ];
        assert_eq!(session_header(&token_cookies(&scoped), false).as_deref(), Some("remixsid=login"));
    }
}
