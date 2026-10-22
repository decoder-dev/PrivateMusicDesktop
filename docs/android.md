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
* the desktop-oriented parts of the UI (window size, updates, output device, export) are
  not adapted for a phone yet: they are harmless but pointless there.
