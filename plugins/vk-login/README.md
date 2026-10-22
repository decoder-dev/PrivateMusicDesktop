# tauri-plugin-vk-login

The Android VK sign-in for Private Music, as a self-contained Tauri 2 mobile
plugin. The desktop app signs in through a second, incognito webview window
restricted to VK; Tauri mobile cannot open a second webview, so on Android the
same flow runs in a native `LoginActivity` hosting an isolated `WebView`.

```
plugins/vk-login/
  Cargo.toml  build.rs  permissions/        Rust crate (+ generated permission files)
  src/lib.rs        LoginEvent, parsing, ui_message(), VkLoginExt, init()   (unit-tested on desktop)
  src/mobile.rs     Android: register_android_plugin, start(), cancel()
  src/desktop.rs    every other target: a stub that returns Error::Unsupported
  android/          Kotlin library module (LoginPlugin, LoginActivity, pure logic + JUnit tests)
```

## How it works

1. `app.vk_login().start()` makes the Kotlin `LoginPlugin` open `LoginActivity`
   (`startActivityForResult`), passing a native `Channel` for events.
2. The Activity wipes cookies, web storage and cache, then loads
   `https://vk.ru/login` in a WebView whose navigation is restricted to VK.
3. Every 1.5 s it reads the cookie jar for `https://login.vk.ru/` (vk.ru and
   login.vk.ru cookies, nothing else). Once a session cookie
   (`remixsid`, `remixsid6`, `remixnsid`) exists, it performs the same
   `login.vk.ru/?act=web_token` request as `engine/src/vk/auth.rs`, with the
   same headers and the same browser identity. If VK answers `"type":"okay"`
   it emits `Success`. If VK does not call the session signed in (2FA, captcha) it
   keeps waiting (and emits `Pending` after 3 tries or when the user taps *Done*);
   three other failures end with `Error`.
4. `Success { cookie_header, web_token_json, set_cookies, user_agent }` goes
   from Kotlin to Rust over the native channel. The host decodes it with the
   engine and calls `App::sign_in`. The browser is wiped again and the Activity closes.

Cookies and tokens exist in: the WebView's cookie jar, the Kotlin `LoginEvent.Success`
object, the channel message, and the Rust `LoginSuccess`. They are never in
an Intent, a log line, the Activity result, or any page JavaScript (the WebView has
no `addJavascriptInterface`; nothing is read through `evaluateJavascript`). `Debug`
for both `LoginSuccess` (Rust) and `Success` (Kotlin) prints `<redacted>`.

## Public Rust API

```rust
pub fn init<R: Runtime>() -> TauriPlugin<R>;           // name "vk-login"
pub trait VkLoginExt<R: Runtime> { fn vk_login(&self) -> &VkLogin<R>; }  // on AppHandle, App, Window...

impl<R: Runtime> VkLogin<R> {
    pub fn set_handler(&self, f: impl Fn(LoginEvent) + Send + Sync + 'static);
    pub fn start(&self)  -> Result<()>;   // opens (or re-fronts) the screen; outcome arrives as events
    pub fn cancel(&self) -> Result<()>;   // closes it; the handler gets Cancelled
}

pub enum LoginEvent {
    Pending { reason: String },     // "pending:unauthorized"
    Success(LoginSuccess),          // "success" once the session was adopted
    Error { kind: String },         // "error:<kind>": offline | timeout | network | format | renderer | launch ...
    Cancelled, LoadFailed, LoadOk,  // "cancelled" | "loadfailed" | "loadok"
    SmokeLoaded { host: String },   // CI smoke mode only
}
impl LoginEvent {
    pub fn from_json(json: &str) -> LoginEvent;   // never fails: nonsense becomes Error{kind:"format"}
    pub fn ui_message(&self) -> String;           // the desktop `pm-login` payload strings
}
pub struct LoginSuccess { pub cookie_header: String, pub web_token_json: String,
                          pub set_cookies: Vec<String>, pub user_agent: String }

// feature "engine":
impl LoginSuccess { pub fn into_session(self) -> Result<pm_engine::vk::Session, pm_engine::Error>; }
pub fn handle_event(event: LoginEvent, sign_in: impl FnOnce(Session)) -> String;  // -> pm-login string
```

Off Android, `VkLogin::start/cancel` return `Err(Error::Unsupported)` and the crate
compiles and tests normally.

The only engine change is `pm_engine::vk::session_from_web_token(body_json,
cookie_header, set_cookies, user_agent)`: it runs the existing private
`decode` and `merge_cookies` on an answer that was fetched elsewhere.

## Integration (what the Android app must do)

1. **Cargo** (`src-tauri/Cargo.toml`, or the Android app's crate):
   ```toml
   tauri-plugin-vk-login = { path = "../plugins/vk-login", features = ["engine"] }
   ```
2. **Register and wire** (in `run()`, next to the other plugins; on desktop the plugin is a harmless stub):
   ```rust
   use tauri_plugin_vk_login::{handle_event, VkLoginExt};
   builder.plugin(tauri_plugin_vk_login::init())
   // in .setup(|app| { ...
   let handle = app.handle().clone();
   let core = state.app.clone();            // Arc<pm_engine::App>
   app.vk_login().set_handler(move |event| {
       let message = handle_event(event, |session| core.sign_in(session));
       let _ = handle.emit("pm-login", message);       // the same event the desktop emits
   });
   ```
   and make the `login_start` command call `app.vk_login().start()` on Android
   (`crate::login::start` stays the desktop implementation behind `#[cfg(desktop)]`).
   `ui/js/views/login.js` needs no change: it already understands every string.
3. **Android manifest**: nothing. The library manifest declares `LoginActivity`
   (not exported, `singleTop`, `configChanges` for rotation/theme/keyboard) and
   the `INTERNET` permission, and Gradle manifest merging adds them to the app.
4. **Capabilities / permissions**: add nothing. `permissions/default.toml` is
   deliberately empty and the commands are called from Rust, so page JavaScript can never
   start the sign-in. Do not add `vk-login:allow-start` / `allow-cancel` to a capability.
5. **Gradle**: `tauri android init` / `tauri android build` pick the plugin up from the
   Rust dependency (`build.rs` records `android/` and copies Tauri's Android API to
   `android/.tauri/tauri-api`). Nothing to add by hand. `minSdk` 26 (the app's).
6. **ProGuard**: `android/consumer-rules.pro` keeps the plugin classes.

## Security decisions (match `src-tauri/src/login.rs`)

| Desktop | Android |
| --- | --- |
| isolated window, private storage | cookies, HTTP cache, history and web storage removed **before and after** every sign-in (`BrowsingData`); web storage is deleted per origin and skips the app's own `*.localhost` origins, because Android shares one profile with the host's main WebView and `WebStorage.deleteAllData()` would erase the UI's `localStorage` settings. Cookies are wiped globally (the UI has none) |
| navigation: https + vk.ru/vk.com (+subdomains), `about:blank`/`srcdoc`, `blob:https://<vk>` | the same rules in `VkUrlPolicy`, in `shouldOverrideUrlLoading`; also for top-level requests in `shouldInterceptRequest` (redirect hops). Stricter: no user-info (`https://x@vk.ru`), port must be 443, no non-ASCII/control/backslash characters. `intent:`, `market:`, custom schemes, `javascript:`, `data:`, `file:` are all refused |
| (not blocked) subresources | subresources may come from any **https** host (VK's CDNs), like the desktop, but `http`/`ws` requests get a 403 and `mixedContentMode` is `NEVER_ALLOW` |
| new windows denied | `onCreateWindow` returns false, `javaScriptCanOpenWindowsAutomatically=false`, downloads refused, file chooser cancelled, permission/geolocation prompts denied, console output discarded |
| logs: host only | `describeForLog` prints scheme and host only, never path, query, fragment or user-info |
| TLS | `onReceivedSslError` always cancels; client-cert and HTTP-auth challenges are cancelled |
| file access | `allowFileAccess=false`, `allowContentAccess=false`, file-URL access flags off, Safe Browsing on, password/form saving off |
| third-party cookies | off (`ALLOW_THIRD_PARTY_COOKIES = false` in `LoginActivity`) |
| screenshots | `FLAG_SECURE`: no screenshots or recents thumbnail of the login form |
| 25 s timer / error-page check | same `LoadWatch` constants (25 s, 4 s grace); `onReceivedError`/`onReceivedHttpError` on the main frame also count as an error page; `loadfailed`/`loadok` events as on desktop. The screen shows a "VK did not load" notice with *Retry*; closing it then reports `loadfailed` (not `cancelled`) so the app keeps its VPN hint |
| cookies for the exchange: vk.ru + login.vk.ru, path `/` | `CookieManager.getCookie("https://login.vk.ru/")` returns exactly that set; if a name occurs twice the first (most specific) value wins. The desktop sorts by domain length; Android's API cannot say which domain a value came from |
| browser identity | a fixed Chrome-on-Android user agent, used both by the WebView and the token request |

Back button: WebView history back, then `Cancelled` (`OnBackInvokedCallback` on Android 13+,
`onBackPressed` before). The top bar has *Close* (X) and *Done* (re-check the session now,
like the desktop's "return to sign-in" button).

### Process death

`startActivityForResult` goes through Tauri's `PluginManager` (an `ActivityResultLauncher`
of the host `MainActivity`). The events travel through an in-process bridge
(`LoginBridge`), not through the Activity result, so session cookies never enter an Intent.
If Android kills the app's process while the screen is up, the call and the channel die with it.
When the system later re-creates `LoginActivity` it has a saved-state bundle but `LoginBridge.live` is
false (static, so reset), which tells it nobody is listening: it finishes at once. The
user returns to the app's sign-in screen and presses *Sign in* again. If the Activity is destroyed
but the process lives (low memory), it comes back with the bridge intact: it keeps the browser
cookies and reloads the login page, and it keeps reporting events. Rotation and theme changes do
not re-create it (`configChanges`).

## CI smoke mode (debug builds only)

Start the host app with an extra and the screen opens by itself, waits for the first VK https
page to finish loading, proves the private cookie store works (sets and reads a probe cookie),
and closes, leaving the result in `files/pm_smoke_login.txt` and in `logcat` (tag `PmSmokeLogin`):

```sh
adb shell am start -n com.dec.privatemusic2.android/.MainActivity --ez pm_smoke_login true
adb shell run-as com.dec.privatemusic2.android cat files/pm_smoke_login.txt   # "loaded vk.ru" (first line)
```

Failure lines start with `error ` (no VK page within 60 s, or the cookie probe failed). Release
builds ignore the extra (checked against `FLAG_DEBUGGABLE`). The host handler also receives
`SmokeLoaded { host }`.

## Tests

```sh
cargo test -p tauri-plugin-vk-login                      # Rust, desktop
cargo test -p tauri-plugin-vk-login --features engine    # + the engine hand-off
cargo check -p tauri-plugin-vk-login --target aarch64-linux-android   # Android Rust half; also creates android/.tauri

cd plugins/vk-login/android
export ANDROID_HOME=$HOME/android-sdk
gradle testDebugUnitTest     # JUnit, no emulator or device
```

The Gradle build is standalone only for tests: `android/settings.gradle.kts` includes
`:tauri-android` from `android/.tauri/tauri-api` (run the `cargo check --target` line once so
build.rs creates it; it needs Android platform 36 for Tauri's own module and platform 35 for ours).
Inside an app, Tauri's generated project includes the module instead. If Maven Central rate-limits you, a
Gradle init script that points `https://repo.maven.apache.org/maven2` at a mirror is enough; nothing in the
repository depends on it.

## Not verifiable without a device

* The real sign-in on vk.ru: VK ID's current redirects, whether any step needs third-party
  cookies or a non-VK iframe, and that the Android user agent gets the same flow as the desktop one.
* `Channel` delivery from Kotlin to Rust on a running app, and the `startActivityForResult` round trip.
* Rendering: dark/light bars, insets with edge-to-edge, the keyboard, rotation.
* The smoke mode on an emulator.

## Native audio picker

`VkLogin::pick_audio()` calls the Android system document picker through `LoginPlugin`, independently
of the sign-in WebView. `AudioFilePolicy` checks the header and bounds the copied stream to 200 MB,
including providers which omit the size. It returns a private cache path to Rust only. The shell stages
an account-scoped opaque handle for its own `choose_audio`/`upload_audio`/`discard_audio` commands;
no capability grants a WebView direct access to the native picker. The picker never reads or returns
login cookies/tokens, and requires no broad storage permission. Kotlin policy tests run with the
existing standalone Gradle test task. Process death can leave a temporary copy; stale picker copies
are removed the next time the picker is used.
