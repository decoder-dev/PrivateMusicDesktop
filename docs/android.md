# Private Music for Android

Status: **sign-in and background playback are wired** (version 0.6.0 line). The app
builds and packages; "Sign in" opens the VK screen of the `vk-login` plugin, and playback
runs under the foreground service of the `media-session` plugin (notification, lock
screen, headset and Bluetooth buttons, audio focus). **None of it has run on a device
yet** (see the last section).

It is Tauri 2 mobile: the same Rust engine (`engine/`, `native/`) and the same
web UI (`ui/`) in the Android WebView, with Kotlin plugins for what Tauri does
not provide on Android. The shell crate is a library
(`src-tauri/src/lib.rs`, `mobile_entry_point`); desktop-only code (tray, media
keys, single instance, window state, the sign-in window) is `cfg(desktop)`.

## Requirements

Android SDK (platforms 35 and 36, build-tools 35.0.0 and 36.0.0), NDK 27.2.12479018, JDK 17+,
Rust with the targets you build, Node (for `npx @tauri-apps/cli@2`):

    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android

`scripts/android-env.sh` exports the variables (override any of them before
sourcing):

    . scripts/android-env.sh
    cd src-tauri

## Build

    npx --yes @tauri-apps/cli@2 android build --debug --target aarch64 --apk
    npx --yes @tauri-apps/cli@2 android build --target aarch64 --apk     # release

Outputs are under `src-tauri/gen/android/app/build/outputs/apk/`:

* debug: `universal/debug/app-universal-debug.apk` (debug-signed, installable)
* release: `universal/release/app-universal-release-unsigned.apk` (see Signing)

ABIs: `--target aarch64` (arm64-v8a, every current phone), `armv7`
(armeabi-v7a), `x86_64` and `i686` (emulators). Without `--target` all four
are built (slow, large). Debug builds are big because they are unoptimised.

Minimum Android is 8.0 (API 26); target SDK is 35 (compile SDK 36, which the Tauri Android library requires). The version
name is the workspace version; the version code is
`major*10000 + minor*100 + patch` (0.6.0 is 600).

## Signing

Release builds are signed only when all four variables are set:

    ANDROID_KEYSTORE_PATH  ANDROID_KEYSTORE_PASSWORD  ANDROID_KEY_ALIAS  ANDROID_KEY_PASSWORD

Otherwise Gradle prints a warning and the release APK is **unsigned**
(zipaligned), with no fallback key. No keystore or password is ever stored in
the repository. Sign afterwards:

    $ANDROID_HOME/build-tools/35.0.0/apksigner sign --ks <jks> --ks-key-alias <alias> app-universal-release-unsigned.apk

Always use the **same key**: Android installs an update over the previous
version only when the signature matches, and only then do the app's data (the
signed-in session) survive the update. A different key means uninstall first
and signing in again.

## Where things are

* **Sign-in** (`plugins/vk-login`, glue in `src-tauri/src/mobile_login.rs`): `login_start` opens
  a native Activity with an isolated WebView limited to VK. The plugin does the `web_token`
  exchange itself, inside the browser session (so 2FA/captcha keep the page), and hands the
  answer to Rust, where `pm_engine::vk::session_from_web_token` builds the same `Session`
  as the desktop exchange (including the refresh cookie used to renew the token) and
  `App::sign_in` stores it. The plugin's events become the same `pm-login` strings the UI
  already knows. The plugin's commands are not granted to the page.
* **Background playback** (`plugins/media-session`, glue in `src-tauri/src/mobile_media.rs`):
  the engine's state goes through a `StatePublisher` worker and `Bridge` to the plugin;
  system buttons come back through `bridge::dispatch`. Capability: `capabilities/mobile.json`
  (`media-session:default`).
* Desktop-only commands have stubs on Android so the UI never meets "command not found":
  window modes and full screen (no-op), output devices (empty), built-in updates (answer
  "no installer for this system"), track export (error `unsupported`).
* The sign-in session is encrypted with an AES-256-GCM key in the Android
  Keystore (`SecureStorePlugin.kt`); only ciphertext is on disk
  (`files/secure/session.bin`). `allowBackup` is off, so it is never backed up.
* TLS roots are bundled (`rustls-tls-webpki-roots`) on Android, the OS store
  elsewhere.
* Cleartext traffic is denied (manifest and network security config).
* The page gets `--sat --sab --sal --sar` (system bar / cutout insets) and
  `--kb` (keyboard height) as CSS px variables on `<html>`; the hardware Back
  button dispatches a cancelable `pm-back` event on `window` (call
  `preventDefault()` to handle it; otherwise the app moves to the background).
* Remote debugging of the WebView (`chrome://inspect`) is on in debug builds only.

## The phone UI

The same `ui/` serves the wide (desktop) layout and a phone layout; `js/mobile/layout.js` sets
`data-platform` (`android` from the user agent or `app_info.platform`), `data-layout`
(`phone` on Android, or a narrow window with a touch pointer; `?layout=phone` /
`?platform=android` force it for trying it in a desktop browser), `data-orientation` and
`data-keyboard`. Everything phone-specific is scoped to `html[data-layout="phone"]`, so the
desktop UI and its tests are unchanged.

* **Frame** (`js/mobile/phone-shell.js`, `css/mobile.css`): the bottom dock (Home, Library,
  Profile and a round Search button), an iOS-style navigation bar with a large title that shrinks
  into it, one push stack per tab (tapping the active tab pops to its root, then scrolls to the top).
  Screens scroll as a whole; the virtual track list measures itself against the screen.
* **Screens**: Home (`js/mobile/home.js`: the stage, continue card, recommendations, mix and release
  shelves on the Studio mix cards, recently played), Library hub (`library-tab.js`: Downloads, Liked,
  Smart playlists, playlists, albums, and "My music" as the Tracks section), Profile (`profile.js`);
  everything else (Explore = "For you", Search, playlist, album, artist, friends, history, stats,
  settings, smart playlists) is the existing 0.6.0 view pushed onto the stack.
* **Player**: a mini player above the dock (tap or swipe up opens, swipe left/right is next/previous)
  and the full-screen player as a draggable sheet (`np-sheet.js`) with the cover, lyrics (with timing
  edit chips), queue and the track menu.
* **Gestures and sheets**: long press and swipe on rows (`row-gestures.js`), pull to refresh
  (`pull-refresh.js`), bottom action sheets instead of pop-up menus (`sheet.js`, used by `openMenu`),
  alert-style dialogs.
* **Back**: `js/mobile/back.js` listens to `pm-back` and closes layers top-down (dialogs and sheets,
  the player, an open mix tuning, the page stack); unhandled, the app goes to the background.
* **Safe areas**: `css/mobile.css` uses `--sat --sab --sal --sar` (set by `MainActivity`) with
  `env(safe-area-inset-*)` as the fallback, and `--kb` (keyboard) for sheets and the dock.
* **Studio look**: `css/mobile-studio.css` adapts the 0.6.0 tokens, Onest and radii to the phone.
* **Hidden on Android** (`[data-desktop-only]`, `.desktop-only`, or not built): updates, window size
  and the mini window, close to tray, keyboard shortcuts, output device, the visualizer, export of a
  track to a file; the engine commands behind them are not called. Downloads and offline playlist sync,
  the equaliser, lyrics, friends, history, statistics, mixes and radio, smart playlists, Selena's tastes,
  the VPN card and bulk actions work as on the desktop.
* **Tests**: `ui-tests/phone.test.js` (touch emulation of a Pixel 7, a 360 px and a 430 px phone, and
  landscape) includes the audits: every pressable at least 44 px, no hover-only control, no horizontal
  overflow on any screen, safe-area variables, reduced motion.

## Logs

    adb logcat -s RustStdoutStderr PrivateMusic

(`RustStdoutStderr` carries the Rust log; the app's `private-music.log` file
is also written to the app's log directory.)

## Verified only on a device

Nothing here has been run on a device or emulator (the build machine has no KVM). Built and
unit-tested without one: both APKs (release unsigned, debug), the manifest merge (service
`mediaPlayback`, `LoginActivity`, permissions), the plugins' Kotlin unit tests and the Rust
tests. To check on a phone:

* the app starts, the UI renders under the system bars with the right insets, Back button;
* the VK sign-in: that VK ID's redirects, 2FA and captcha work with the Android user agent,
  the `Channel` delivery from Kotlin to Rust, the exchange and the stored refresh cookie;
* the Keystore round trip (the session survives a restart and an update with the same key);
* audio output through cpal/oboe, then: play, lock the screen for 5 minutes, pull the
  headphones, take a call, swipe the app away while playing and while paused, deny the
  notification permission;
* OEM battery savers (they can kill a foreground service), and the CSP
  (`connect-src ipc: http://ipc.localhost`) with Tauri's IPC;
* the phone layout on a real WebView: insets from `MainActivity`, the `pm-back` handling, touch
  gestures and the on-screen keyboard (`--kb`) behave as in the Chromium emulation of the tests,
  but only a device proves it.
