//! Авторизуемся в VK.
//!
//! В отдельном окне отображается собственная страница входа vk.ru (поэтому пароль VK, экраны 2FA и
//! капчи работают так же, как и в браузере). Приложение никогда не видит пароль: оно просматривает
//! jar-файл cookie этого окна, и как только появляется файл cookie сеанса, оно запрашивает у VK токен
//! API, точно так же, как приложение iOS.
//!
//! Окно изолировано: личное хранилище, нет возможностей приложения, и оно может перемещаться только
//! внутри vk.ru / vk.com через https.

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

/// Установлено CI: проверьте загрузку страницы и доступ к личным файлам cookie, затем выйдите.
pub static SMOKE_REPORT: OnceLock<PathBuf> = OnceLock::new();
static SMOKE_PAGE_LOADED: AtomicBool = AtomicBool::new(false);

// Пришла ли собственная страница VK? Заблокированный или недоступный VK (VPN, который включен или
// должен быть выключен) оставляет окно пустым или на странице ошибок браузера, и веб-представление не
// сообщает о собственных сбоях.
static PAGE_LOADED: AtomicBool = AtomicBool::new(false);
static PAGE_ERROR: AtomicBool = AtomicBool::new(false);
/// Сообщалось, что окно не загружается; следующее нажатие "sign in" перезагружает его.
static LOAD_FAILED: AtomicBool = AtomicBool::new(false);
/// Сообщает наблюдателю, что страницу снова запросили: его часы перезапускаются.
static LOAD_RESTARTED: AtomicBool = AtomicBool::new(false);

/// Сколько времени может пройти страница входа VK, прежде чем окно будет названо зависшим.
const LOAD_TIMEOUT: Duration = Duration::from_secs(25);
/// Пустая страница может быть первой страницей веб-представления: только страница, отличная от VK,
/// которая по-прежнему остается тем, что мы видим после того, как это долгое время принимается за
/// страницу с ошибкой.
const ERROR_PAGE_GRACE: Duration = Duration::from_secs(4);

const LOGIN_URL: &str = "https://vk.ru/login";
const POLL: Duration = Duration::from_millis(1500);
const SESSION_COOKIES: [&str; 3] = ["remixsid", "remixsid6", "remixnsid"];

/// Идентификатор браузера, отображаемый VK как в окне входа, так и при обмене токенами (VK привязывает
/// к нему токен).
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
        // Пустые рамки, которые страница VK создает сама для себя, не несут никакого контента.
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        // Объект URLs, созданный собственной страницей VK: blob:https://id.vk.ru/<uuid>.
        "blob" => Url::parse(url.path()).is_ok_and(|inner| is_allowed_navigation(&inner) && inner.scheme() == "https"),
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LoadVerdict {
    Waiting,
    Failed,
}

/// Страница входа считается не загружающейся, если страница VK не завершилась после [`LOAD_TIMEOUT`]
/// или завершилась страница, отличная от страницы VK (собственная страница ошибки браузера, на которую
/// это окно все равно отказывается перейти).
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

/// Автоматические проверки ждут известного файла cookie сеанса; Заполнение вручную позволяет VK
/// проверять любой непустой набор файлов cookie, как это делает кнопка завершения клиента iOS.
fn session_header(cookies: &[(String, String)], manual: bool) -> Option<String> {
    let signed_in = cookies
        .iter()
        .any(|(name, value)| SESSION_COOKIES.contains(&name.to_ascii_lowercase().as_str()) && !value.is_empty());
    (signed_in || (manual && !cookies.is_empty()))
        .then(|| cookies.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; "))
}

/// Прочитайте весь частный магазин файлов cookie: поиск по vk.ru/ удаляет файлы cookie только для
/// хоста, установленные login.vk.ru, где запрашивается web_token. Никогда не пересылайте на него файлы
/// cookie с VK, ID, vk.com или несвязанных сайтов.
fn token_cookies(cookies: &[tauri::webview::Cookie<'_>]) -> Vec<(String, String)> {
    let mut cookies: Vec<_> = cookies.iter().filter(|cookie| {
        let domain = cookie.domain().unwrap_or("").trim_start_matches('.');
        (domain.eq_ignore_ascii_case("vk.ru") || domain.eq_ignore_ascii_case("login.vk.ru"))
            && cookie.path().unwrap_or("/") == "/"
    }).collect();
    // Как и в браузере, при конфликте имен отдавайте предпочтение значению, специфичному для хоста.
    cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.domain().unwrap_or("").len()));
    let mut seen = std::collections::HashSet::new();
    cookies.into_iter().filter(|cookie| seen.insert(cookie.name().to_string()))
        .map(|cookie| (cookie.name().to_string(), cookie.value().to_string())).collect()
}

pub fn start(handle: &AppHandle, state: &AppState) -> Result<(), String> {
    if let Some(existing) = handle.get_webview_window("login") {
        if LOAD_FAILED.load(Ordering::SeqCst) {
            // Страница так и не дошла: «Повторить» загружает ее снова, в том же окне и с теми же
            // ограничениями.
            log::info!("sign-in window: reloading the login page");
            reset_load_state();
            if let Ok(url) = LOGIN_URL.parse::<Url>() {
                let _ = existing.navigate(url);
            }
        }
        // Явное завершение также работает, если VK меняет имена файлов cookie сеанса. Только конечная
        // точка токена VK решает, аутентифицируются ли эти файлы cookie.
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
                // Только адреса входа хоста могут содержать одноразовые коды.
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
                    // В конце концов, это удалось (VPN тем временем был переключен).
                    let _ = webview.app_handle().emit("pm-login", "loadok");
                }
            } else if !PAGE_LOADED.load(Ordering::SeqCst) {
                // Не страница VK: страница ошибок движка или пустая.
                PAGE_ERROR.store(true, Ordering::SeqCst);
            }
            if SMOKE_REPORT.get().is_some() && is_vk_host(&host) {
                // Работник также должен доказать, что частные файлы cookie доступны для чтения.
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

/// Ожидает завершения входа пользователя в систему. `None`, когда окно было закрыто.
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
            // Сообщите окну: оно показывает подсказку VPN и кнопку «Повторить», и это окно остается
            // открытым для повторной попытки.
            log::warn!("sign-in window: the VK login page did not load");
            LOAD_FAILED.store(true, Ordering::SeqCst);
            let _ = handle.emit("pm-login", "loadfailed");
        }
        if SMOKE_REPORT.get().is_some() {
            // CI проверяет только браузер, а не конечную точку токена VK, в том числе во время загрузки
            // начальной страницы.
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
                // Закрытие окна во время запроса HTTP отменяет принятие.
                if handle.get_webview_window("login").is_none() || !running.load(Ordering::SeqCst) {
                    return None;
                }
                log::info!("sign-in window: token exchange succeeded");
                return Some(Ok(session));
            }
            // Файл cookie существует, но VK еще не вызывает его для входа в систему (этап 2FA или
            // проверки подлинности): продолжайте ждать.
            Err(Error::Unauthorized) => {
                failures += 1;
                if failures == 3 || manual {
                    log::warn!("sign-in window: VK has not confirmed the web session");
                    let _ = handle.emit("pm-login", "pending:unauthorized");
                }
                // Сохраняйте сессию браузера для 2FA, не забивая VK.
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

/// Запускает собственный менеджер файлов cookie в изолированном окне CI. Никаких учетных данных или
/// запросов токенов не требуется.
fn smoke_cookie_store(window: &tauri::WebviewWindow) -> bool {
    use tauri::webview::Cookie;
    let probes = [
        Cookie::build(("pm_probe_browser", "browser")).domain(".vk.ru").path("/").secure(true).build(),
        Cookie::build(("pm_probe_login", "login")).domain("login.vk.ru").path("/").secure(true).http_only(true).build(),
    ];
    // Один файл cookie поступает со страницы JavaScript, другой — от собственного менеджера. При
    // поездке туда и обратно только для местных жителей менеджер пропустит неправильный профиль.
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
