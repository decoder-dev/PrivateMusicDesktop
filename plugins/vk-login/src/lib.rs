//! Android VK sign-in for Private Music, as a Tauri 2 mobile plugin.
//!
//! Tauri mobile cannot open a second webview window, so the sign-in runs in a
//! native Android `Activity` (`LoginActivity`) hosting an isolated `WebView`
//! that may only reach VK hosts over https. This crate is the Rust half: it
//! launches the Activity and turns the events the Kotlin side reports into
//! [`LoginEvent`]s for the host app. Cookies and tokens travel from Kotlin to
//! Rust over a native channel and never reach any web view's JavaScript.
//!
//! The parsing and the event vocabulary here are plain Rust, so they are
//! unit-tested on the desktop; the Android-only code is in `mobile.rs`.

use std::fmt;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

#[cfg(target_os = "android")]
mod mobile;
#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
pub use mobile::VkLogin;
#[cfg(not(target_os = "android"))]
pub use desktop::VkLogin;

/// Name of the Kotlin package that holds `LoginPlugin`.
#[cfg(target_os = "android")]
const ANDROID_PACKAGE: &str = "com.dec.privatemusic.vklogin";

/// Largest result accepted from the native side (a web_token answer is ~1 KiB).
const MAX_FIELD: usize = 64 * 1024;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// Not running on Android: there is no native sign-in screen.
    Unsupported,
    /// The native plugin could not be reached or refused the call.
    Native(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => f.write_str("the VK sign-in screen exists on Android only"),
            Error::Native(message) => write!(f, "VK sign-in plugin: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// What the signed-in web session looks like, as read inside the isolated
/// WebView. Every field is a secret or derived from one: `Debug` prints none.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSuccess {
    /// `name=value; name=value` for vk.ru and login.vk.ru (nothing else).
    pub cookie_header: String,
    /// The JSON body VK answered to `login.vk.ru/?act=web_token` with.
    pub web_token_json: String,
    /// `Set-Cookie` headers of that answer; the engine overlays them.
    #[serde(default)]
    pub set_cookies: Vec<String>,
    /// The browser identity the WebView and the exchange both used.
    pub user_agent: String,
}

impl fmt::Debug for LoginSuccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoginSuccess(<redacted>)")
    }
}

/// What the sign-in screen reports. The first six mirror the desktop app's
/// `pm-login` event payloads one to one (see [`LoginEvent::ui_message`]).
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LoginEvent {
    /// A session cookie exists but VK does not call it signed in yet (2FA,
    /// captcha), or the user pressed "Done" too early.
    Pending { reason: String },
    /// Signed in: the session is ready for the engine.
    Success(LoginSuccess),
    /// The sign-in failed for a reason that is not the user's: `kind` is an
    /// engine error kind (`offline`, `timeout`, `network`, `format`, ...).
    Error { kind: String },
    /// The screen was closed (Close, Back, `cancel()`, process death).
    Cancelled,
    /// VK's page did not load within 25 s, or the browser's error page showed.
    LoadFailed,
    /// VK's page loaded after a `LoadFailed`.
    LoadOk,
    /// CI smoke mode only (debug builds): the first VK page finished loading.
    SmokeLoaded { host: String },
}

impl LoginEvent {
    /// Parses one JSON message from the Kotlin side. Anything malformed or
    /// oversized becomes `Error { kind: "format" }`: a sign-in screen that
    /// speaks nonsense must end the attempt, not hang it.
    pub fn from_json(json: &str) -> LoginEvent {
        if json.len() > 4 * MAX_FIELD {
            return LoginEvent::format_error();
        }
        match serde_json::from_str::<LoginEvent>(json) {
            Ok(event) => event.validated(),
            Err(_) => LoginEvent::format_error(),
        }
    }

    fn format_error() -> LoginEvent {
        LoginEvent::Error { kind: "format".into() }
    }

    /// Strips what the UI must not be handed: odd tokens in `kind`/`reason`
    /// (they are pasted into `error:<kind>`), empty or huge secrets.
    fn validated(self) -> LoginEvent {
        let token = |s: &str| {
            !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        };
        match self {
            LoginEvent::Error { kind } if !token(&kind) => LoginEvent::Error { kind: "unknown".into() },
            LoginEvent::Pending { reason } if !token(&reason) => {
                LoginEvent::Pending { reason: "unauthorized".into() }
            }
            LoginEvent::SmokeLoaded { host }
                if host.is_empty()
                    || host.len() > 253
                    || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') =>
            {
                LoginEvent::format_error()
            }
            LoginEvent::Success(s) => {
                let fields = [&s.cookie_header, &s.web_token_json, &s.user_agent];
                let sane = fields.iter().all(|f| !f.trim().is_empty() && f.len() <= MAX_FIELD)
                    && s.set_cookies.len() <= 64
                    && s.set_cookies.iter().all(|c| c.len() <= 8 * 1024);
                if sane {
                    LoginEvent::Success(s)
                } else {
                    LoginEvent::format_error()
                }
            }
            other => other,
        }
    }

    /// The string the desktop app emits as its `pm-login` event, which the
    /// bundled UI (`ui/js/views/login.js`) already understands. `Success`
    /// maps to `"success"`: call it only once the session was adopted.
    pub fn ui_message(&self) -> String {
        match self {
            LoginEvent::Pending { reason } => format!("pending:{reason}"),
            LoginEvent::Success(_) => "success".into(),
            LoginEvent::Error { kind } => format!("error:{kind}"),
            LoginEvent::Cancelled => "cancelled".into(),
            LoginEvent::LoadFailed => "loadfailed".into(),
            LoginEvent::LoadOk => "loadok".into(),
            // Not part of the desktop vocabulary: the UI treats it as no-op.
            LoginEvent::SmokeLoaded { .. } => "loadok".into(),
        }
    }
}

#[cfg(feature = "engine")]
impl LoginSuccess {
    /// Runs the engine's own decoding of the `web_token` answer.
    pub fn into_session(self) -> std::result::Result<pm_engine::vk::Session, pm_engine::Error> {
        pm_engine::vk::session_from_web_token(
            &self.web_token_json,
            &self.cookie_header,
            &self.set_cookies,
            &self.user_agent,
        )
    }
}

/// The whole host-side step for one event, ready to feed `pm-login`:
/// on `Success` it decodes the session and calls `sign_in` with it, and answers
/// `"success"` or `"error:<kind>"`; every other event is just translated.
#[cfg(feature = "engine")]
pub fn handle_event(event: LoginEvent, sign_in: impl FnOnce(pm_engine::vk::Session)) -> String {
    match event {
        LoginEvent::Success(success) => match success.into_session() {
            Ok(session) => {
                sign_in(session);
                "success".into()
            }
            Err(error) => format!("error:{}", error.kind()),
        },
        other => other.ui_message(),
    }
}

type Handler = Arc<dyn Fn(LoginEvent) + Send + Sync + 'static>;

/// Where events go. Shared by the plugin handle and the native channel.
#[derive(Clone, Default)]
pub(crate) struct Events {
    handler: Arc<Mutex<Option<Handler>>>,
}

// Only the Android half dispatches; the desktop stub and the tests use it too.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
impl Events {
    pub(crate) fn set(&self, handler: Handler) {
        *self.handler.lock().unwrap_or_else(|p| p.into_inner()) = Some(handler);
    }

    pub(crate) fn dispatch(&self, event: LoginEvent) {
        let handler = self.handler.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match handler {
            Some(handler) => handler(event),
            None => log::warn!("vk-login: no handler set, dropped a {:?} event", event_name(&event)),
        }
    }

    pub(crate) fn dispatch_json(&self, json: &str) {
        self.dispatch(LoginEvent::from_json(json));
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn event_name(event: &LoginEvent) -> &'static str {
    match event {
        LoginEvent::Pending { .. } => "pending",
        LoginEvent::Success(_) => "success",
        LoginEvent::Error { .. } => "error",
        LoginEvent::Cancelled => "cancelled",
        LoginEvent::LoadFailed => "loadFailed",
        LoginEvent::LoadOk => "loadOk",
        LoginEvent::SmokeLoaded { .. } => "smokeLoaded",
    }
}

/// `app.vk_login()` on any `Manager`.
pub trait VkLoginExt<R: Runtime> {
    fn vk_login(&self) -> &VkLogin<R>;
}

impl<R: Runtime, T: Manager<R>> VkLoginExt<R> for T {
    fn vk_login(&self) -> &VkLogin<R> {
        self.state::<VkLogin<R>>().inner()
    }
}

/// Registers the plugin. Call `.plugin(tauri_plugin_vk_login::init())` on the
/// app builder, then `app.vk_login().set_handler(..)` before the first `start`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("vk-login")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let login = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let login = desktop::init(app, api)?;
            app.manage(login);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success_json() -> String {
        r#"{"event":"success","cookieHeader":"remixsid=abc; remixlang=0","webTokenJson":"{\"type\":\"okay\"}","setCookies":["a=b; Path=/"],"userAgent":"UA"}"#.into()
    }

    #[test]
    fn plain_events_parse() {
        assert_eq!(LoginEvent::from_json(r#"{"event":"cancelled"}"#), LoginEvent::Cancelled);
        assert_eq!(LoginEvent::from_json(r#"{"event":"loadFailed"}"#), LoginEvent::LoadFailed);
        assert_eq!(LoginEvent::from_json(r#"{"event":"loadOk"}"#), LoginEvent::LoadOk);
        assert_eq!(
            LoginEvent::from_json(r#"{"event":"pending","reason":"unauthorized"}"#),
            LoginEvent::Pending { reason: "unauthorized".into() }
        );
        assert_eq!(
            LoginEvent::from_json(r#"{"event":"error","kind":"timeout"}"#),
            LoginEvent::Error { kind: "timeout".into() }
        );
        assert_eq!(
            LoginEvent::from_json(r#"{"event":"smokeLoaded","host":"vk.ru"}"#),
            LoginEvent::SmokeLoaded { host: "vk.ru".into() }
        );
    }

    #[test]
    fn success_parses_and_is_never_printed() {
        let event = LoginEvent::from_json(&success_json());
        let LoginEvent::Success(ok) = &event else { panic!("{event:?}") };
        assert_eq!(ok.cookie_header, "remixsid=abc; remixlang=0");
        assert_eq!(ok.set_cookies, vec!["a=b; Path=/".to_string()]);
        assert_eq!(ok.user_agent, "UA");
        let printed = format!("{event:?} {ok:?}");
        assert!(!printed.contains("remixsid") && !printed.contains("okay"), "{printed}");
        // set_cookies is optional.
        let bare = r#"{"event":"success","cookieHeader":"a=b","webTokenJson":"{}","userAgent":"UA"}"#;
        assert!(matches!(LoginEvent::from_json(bare), LoginEvent::Success(s) if s.set_cookies.is_empty()));
    }

    #[test]
    fn nonsense_ends_the_attempt_with_a_format_error() {
        let format = LoginEvent::Error { kind: "format".into() };
        for bad in [
            "",
            "not json",
            r#"{"event":"explode"}"#,
            r#"{"event":"error"}"#,
            r#"{"event":"success","cookieHeader":"","webTokenJson":"{}","userAgent":"UA"}"#,
            r#"{"event":"success","cookieHeader":"a=b","webTokenJson":"  ","userAgent":"UA"}"#,
            r#"{"event":"success","cookieHeader":"a=b","webTokenJson":"{}","userAgent":""}"#,
            r#"{"event":"smokeLoaded","host":"vk.ru/../x"}"#,
            r#"{"event":"smokeLoaded","host":""}"#,
        ] {
            assert_eq!(LoginEvent::from_json(bad), format, "{bad}");
        }
        let huge = format!(
            r#"{{"event":"success","cookieHeader":"{}","webTokenJson":"{{}}","userAgent":"UA"}}"#,
            "a".repeat(MAX_FIELD + 1)
        );
        assert_eq!(LoginEvent::from_json(&huge), format);
    }

    #[test]
    fn error_kinds_cannot_smuggle_text_into_the_ui_message() {
        let weird = LoginEvent::from_json(r#"{"event":"error","kind":"x:y <b>"}"#);
        assert_eq!(weird.ui_message(), "error:unknown");
        let weird = LoginEvent::from_json(r#"{"event":"pending","reason":"UPPER"}"#);
        assert_eq!(weird.ui_message(), "pending:unauthorized");
    }

    #[test]
    fn ui_messages_match_the_desktop_vocabulary() {
        let msg = |json: &str| LoginEvent::from_json(json).ui_message();
        assert_eq!(msg(r#"{"event":"pending","reason":"unauthorized"}"#), "pending:unauthorized");
        assert_eq!(msg(&success_json()), "success");
        assert_eq!(msg(r#"{"event":"error","kind":"offline"}"#), "error:offline");
        assert_eq!(msg(r#"{"event":"cancelled"}"#), "cancelled");
        assert_eq!(msg(r#"{"event":"loadFailed"}"#), "loadfailed");
        assert_eq!(msg(r#"{"event":"loadOk"}"#), "loadok");
    }

    #[test]
    fn events_reach_the_handler_and_are_dropped_without_one() {
        let events = Events::default();
        events.dispatch(LoginEvent::Cancelled); // no handler: only a log line
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        events.set(Arc::new(move |e| sink.lock().unwrap().push(e)));
        events.dispatch_json(r#"{"event":"loadOk"}"#);
        events.dispatch_json("garbage");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![LoginEvent::LoadOk, LoginEvent::Error { kind: "format".into() }]
        );
    }

    #[test]
    fn the_stub_refuses_cleanly_off_android() {
        #[cfg(not(target_os = "android"))]
        assert!(matches!(desktop::unsupported(), Err(Error::Unsupported)));
    }

    #[cfg(feature = "engine")]
    #[test]
    fn a_success_becomes_an_engine_session() {
        let body = r#"{"type":"okay","data":{"access_token":"0123456789abcdef0123","user_id":9,"expires":3600}}"#;
        let json = serde_json::json!({
            "event": "success", "cookieHeader": "remixsid=a", "webTokenJson": body,
            "setCookies": ["remixsid=b; Path=/"], "userAgent": "UA"
        })
        .to_string();
        let mut adopted = None;
        let message = handle_event(LoginEvent::from_json(&json), |s| adopted = Some(s));
        assert_eq!(message, "success");
        let session = adopted.unwrap();
        assert_eq!(session.user_id, Some(9));
        assert_eq!(session.refresh_cookie, "remixsid=b");
        assert_eq!(session.web_user_agent, "UA");

        // VK says "not signed in": nothing is adopted and the UI hears why.
        let refused = serde_json::json!({
            "event": "success", "cookieHeader": "remixsid=a", "webTokenJson": r#"{"type":"error"}"#, "userAgent": "UA"
        })
        .to_string();
        let message = handle_event(LoginEvent::from_json(&refused), |_| panic!("must not sign in"));
        assert_eq!(message, "error:unauthorized");
        assert_eq!(handle_event(LoginEvent::LoadFailed, |_| panic!()), "loadfailed");
    }
}
