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
`major*10000 + minor*100 + patch` (0.6.0 is 600, 0.6.1 is 601, 0.6.2 is 602, 0.6.3 is 603, 0.6.4 is 604, 0.6.5 is 605).

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
  Home has a station card; Library uses a two-column shortcut dashboard and segmented filters.
  The existing accent and theme preferences are preserved.
* **Collection parity with iPhone**: Library's Playlists and Albums sections have a List/Grid switch
  and visible pin buttons. Up to 24 collections can be pinned to the All section, with snapshots
  retained for navigation when a collection is absent from a fresh shelf response. Pins and layout
  are device-local and account-scoped (`js/mobile/collections.js`, via `personal.js`); they are not
  synced to VK or the iPhone. Profile uses the account photo when supplied, with a fallback avatar.
  Reference: `PrivateMusic2` commit `ecf7adb` (`LibraryView.swift`, `ProfileView.swift`).
  Preview screenshots in [design/android](design/android/) use the UI-test mock backend and generated
  placeholder covers: light at 360×640, dark at 412×915. These are browser previews, not device captures.
* **Hidden on Android** (`[data-desktop-only]`, `.desktop-only`, or not built): updates, window size
  and the mini window, close to tray, keyboard shortcuts, output device, the visualizer, export of a
  track to a file; the engine commands behind them are not called. Downloads and offline playlist sync,
  the equaliser, lyrics, friends, history, statistics, mixes and radio, smart playlists, Selena's tastes,
  the VPN card and bulk actions work as on the desktop.
* **Listen later**: the current track's menu can save up to 200 queue entries, shuffle/repeat and
  the playback position. Library shows the saved session above its shortcuts, with resume/remove
  actions. Streams are stripped before saving; snapshots are local and isolated by account. Resuming
  restores a paused queue before starting playback. Radio bookmarks also keep the personal/catalog
  mix options and page cursor, or the similar-track seed; their queues continue filling on resume.
  Old snapshots without a valid radio source still restore as regular queues. Radio seeds omit stream
  addresses; fresh tracks require an internet connection. Similar-radio recommendations rebuild
  their seed rotation from the saved source and queue. Queue history preserves the same radio
  context and restores it on pause.
* **Playlist import**: Library's actions menu opens a TXT/CSV importer (1 MB / 500 unique entries).
  It recognises numbered `Artist — Title` lists and CSV headers in English/Russian, searches VK
  sequentially, and lets the user review recording variants before creating a playlist. Stop keeps
  the confirmed partial playlist and prevents subsequent additions; leaving during matching cancels
  the search, and account changes clear the form and invalidate pending replies. No third-party
  streaming-service login is needed. File picking still needs verification on an Android device.
* **Upload MP3**: Library's actions menu opens a native file picker, followed by artist/title and
  an explicit Upload action. Only a bounded private copy (up to 200 MB) is staged; JavaScript gets
  an opaque account-scoped handle. Both Kotlin and Rust validate the MP3 header/size. Uploads use
  VK upload hosts over HTTPS, never follow redirects, and `audio.save` is not replayed on transport
  errors. Cancel/account changes stop further work; temporary copies are removed after success,
  cancellation or disposal. A save already sent to VK may complete despite cancellation. Device
  picking and real VK upload still need a device/account smoke test; no real media is sent by tests.
* **Share and Copy link**: track menus and playlist/album headers expose canonical VK links.
  Android uses the system share chooser or clipboard; desktop Copy uses the WebView clipboard
  with a fallback for older WebViews. Only collection access keys are retained where needed;
  stream URLs, account tokens and cookies are never used as the share payload. Native Rust/Kotlin
  restrict these actions to VK resource locators. Opening the share chooser does not mean a message
  was delivered. The playlist-link opener understands shared `access_key` query parameters.
* **Collection search**: Playlists and Albums have local filters; album searches include artists.
* **Tests**: `ui-tests/phone.test.js` (touch emulation of a Pixel 7, a 360 px and a 430 px phone, and
  landscape) includes the audits: every pressable at least 44 px, no hover-only control, no horizontal
  overflow on any screen, safe-area variables, reduced motion.

## Logs

    adb logcat -s RustStdoutStderr PrivateMusic

(`RustStdoutStderr` carries the Rust log; the app's `private-music.log` file
is also written to the app's log directory.)

## Verified only on a device

(The `Android` workflow, see the CI section below, now runs the app on an emulator; what it covers is
listed there. Until its first green run, treat the list below as still unverified.)

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

## Выпуск Android-версии

The release workflow (`release.yml`, job `Android`) builds the **unsigned** arm64 release APK and attaches it
to the GitHub release as `PrivateMusic_<version>_android-arm64-unsigned.apk` (it is also listed in
`SHA256SUMS.txt`). The project owner signs it **outside CI** with a private key that never enters the
repository, and re-uploads the signed file as `PrivateMusic_<version>_android-arm64.apk`.

Steps (build-tools 35 or newer; the APK is already zipaligned, so sign it as is):

    gh release download v<version> --pattern '*android-arm64-unsigned.apk'
    $ANDROID_HOME/build-tools/35.0.0/apksigner sign --ks <release.jks> --ks-key-alias <alias> \
        --out PrivateMusic_<version>_android-arm64.apk PrivateMusic_<version>_android-arm64-unsigned.apk
    $ANDROID_HOME/build-tools/35.0.0/apksigner verify --verbose --print-certs PrivateMusic_<version>_android-arm64.apk
    gh release upload v<version> PrivateMusic_<version>_android-arm64.apk --clobber

Check that the certificate fingerprint printed by `verify` is the one of your release key. Then delete the
unsigned asset from the release (`gh release delete-asset v<version> PrivateMusic_<version>_android-arm64-unsigned.apk`)
and refresh the checksums (`SHA256SUMS.txt` lists the unsigned file): download the assets, run `sha256sum`
over them and `gh release upload v<version> SHA256SUMS.txt --clobber`.

Installing:

* with a cable: `adb install PrivateMusic_<version>_android-arm64.apk` (`-r` to update);
* or copy the APK to the phone, open it from a file manager or the browser's downloads and allow
  "Install unknown apps" for that app (Settings, Apps, Special access). Android 8+ asks per app.

Updates install over the previous version **only if it was signed with the same key** (and the version code
is higher: it grows with the version, `0.6.1` = 601). A different key means uninstalling first, which also
removes the signed-in session. Keep the keystore and its passwords backed up: without the key there are no
updates for the installed copies.

Optional signing in CI: if **all four** repository secrets exist, the `Android` job signs the APK itself
(Gradle reads them; the key is a temporary file removed at the end) and the asset is named
`PrivateMusic_<version>_android-arm64.apk` without "unsigned":

    ANDROID_KEYSTORE_BASE64    the .jks file, base64 (`base64 -w0 release.jks`)
    ANDROID_KEYSTORE_PASSWORD  ANDROID_KEY_ALIAS  ANDROID_KEY_PASSWORD

With any of them missing the job stays unsigned and prints a notice. If the Android job fails, the desktop
installers are still published (with a warning) and the APK can be added later with `gh release upload`.

## CI

Workflow `.github/workflows/android.yml` ("Android"). It runs on pushes to `android` and `android-**`, on pull
requests that touch the Android-relevant paths (`src-tauri`, `plugins`, `engine`, `native`, `ui`, Cargo files,
the Android scripts) and by hand (`workflow_dispatch`). Only the two already pinned actions (`checkout`,
`upload-artifact`) are used; everything else is plain `run:` steps, helpers are in `scripts/android-*.sh`.

**Job `build`** (ubuntu-latest): Rust stable with `aarch64-linux-android` and `x86_64-linux-android`; the
preinstalled Android SDK at `$ANDROID_HOME`, plus NDK `27.2.12479018`, platforms 35 and 36 and build-tools 35.0.0 and
36.0.0 when missing (`scripts/android-ci-sdk.sh`, licences accepted automatically); JDK 17 (21 as a fallback) from
the runner. `scripts/android-ci-env.sh` drops the runner's own NDK variables and sources `scripts/android-env.sh`,
so the exact NDK is used. Then:

1. release arm64 APK, unsigned (`scripts/android-build.sh release aarch64`);
2. `scripts/android-verify-apk.sh`: `libprivate_music_desktop_lib.so` for the ABI, `classes.dex`, package id,
   the permissions (INTERNET, ACCESS_NETWORK_STATE, FOREGROUND_SERVICE, FOREGROUND_SERVICE_MEDIA_PLAYBACK,
   POST_NOTIFICATIONS, WAKE_LOCK), `PlaybackService` as a `mediaPlayback` foreground service, `LoginActivity`,
   debuggable only in the debug build. It reports every failed check, then prints the manifest tree;
3. SHA-256 of the release APK;
4. debug x86_64 APK for the emulator (+ the same checks), and a second debug build with versionCode + 1
   (`ANDROID_VERSION_CODE_OVERRIDE`, read by `app/build.gradle.kts`) for the update test;
5. upload the artifacts; the emulator job can start from here;
6. `cargo clippy --target aarch64-linux-android -p private-music-desktop -p tauri-plugin-vk-login -p
   tauri-plugin-media-session --locked -- -D warnings`. The three crates share dependencies, so one command is
   cheaper than three; if it becomes too slow, keep only `private-music-desktop`;
7. the Kotlin unit tests of both plugins (`testDebugUnitTest`, Gradle 8.14.3 from the media-session wrapper).

**Job `emulator`** (needs `build`, 45 min limit, runs even when clippy or the Kotlin tests failed): downloads the
debug APKs with `gh run download`, enables KVM, installs the emulator and the image
`system-images;android-34;google_apis;x86_64`, creates a Pixel 6 AVD with 2 GB RAM and boots it headless
(`scripts/android-emulator-tests/boot-emulator.sh`, one retry; animations off). Then:

* `scripts/android-emulator-tests/run.mjs` drives the real app (no mock backend, signed out) through the WebView
  DevTools socket (`webview_devtools_remote_<pid>` forwarded with `adb forward`, Playwright `connectOverCDP`;
  Playwright needs no browser download for this). Steps: (1) starts, alive after 20 s, no crash in logcat;
  (2) the page loads from the app origin, `data-platform="android"` and `data-layout="phone"` (only a warning while
  the phone layout is not on the branch); (3) the sign-in screen with an enabled primary button;
  (4) `app_info` (platform `android`, version = `tauri.conf.json`); (5) `session_state` is signed out and
  `persistent`, plus the debug-only `debug_secure_selftest` command (write, read, delete through the Keystore plugin);
  (6a) the smoke hook (`--ez pm_smoke_login true`) loads a VK page: `files/pm_smoke_login.txt` says `loaded <host>`;
  (6b) the real button opens `LoginActivity`, the `pm-login` event `loadok` arrives, a page-initiated jump to a non-VK
  site is checked (warning only if it was not blocked), Back returns to the sign-in screen; (7) Back on the main screen
  moves the app to the background, relaunch works; (8) rotation keeps the same page; (9) revoking and granting
  POST_NOTIFICATIONS and the media plugin's `status` command; (10) no crash in the whole log and no uncaught page errors;
* `scripts/android-upgrade-check.sh`: installs the first APK, writes a file, a `localStorage` value and a Keystore
  probe (`files/secure/session.bin`), installs the second APK (higher versionCode) over it, then checks that
  everything survived (file, localStorage, same `session.bin` hash, the probe still decrypts) and that the app starts.
  It proves the install-over path and Keystore survival with a debug key; it cannot prove a migration from an
  older release (none exists yet) or a signature change.

**Artifacts** (Actions run page, "Artifacts"):

| Name | Content |
| --- | --- |
| `android-release-unsigned` | `PrivateMusic_<v>_android-arm64-unsigned.apk` and its `.sha256` |
| `android-debug-x86_64` | the debug APK and the same with versionCode + 1 |
| `android-kotlin-test-reports` | Gradle reports of the plugin unit tests |
| `android-emulator-logs` | everything below |

**Reading the emulator logs** (`android-emulator-logs`): `summary.md` / `results.json` (also in the job summary) tell
which step failed and why; `fail-NN.png` is the screen at the failure, `03-sign-in.png`, `08-landscape.png`,
`06-vk-login-page.png` are normal screenshots (the sign-in Activity is `FLAG_SECURE`, so only the DevTools
screenshot shows it); `logcat-full.txt` is the full `adb logcat -d` (search for `FATAL EXCEPTION`, `RustStdoutStderr`
for the Rust log, `PrivateMusic`, `PmSmokeLogin`); `emulator-boot-N.log` is the emulator's own output (KVM
problems show up there); `devtools-pages-*.json` lists the WebView pages (host only), `dumpsys-*.txt`,
`window-focus.txt`, `tombstones/` (native crashes, when readable) and `logcat-upgrade.txt` come from the end of the run.

**Rerun**: "Re-run failed jobs" on the run page; or `workflow_dispatch` on the branch. Emulator boots flake on shared
runners: the boot is retried once inside the job, a second red boot is worth one rerun before digging.

**Locally** on a connected device or emulator (debug build, `adb` on PATH, Node 20+):

    scripts/android-emulator-tests/run-local.sh --apk path/to/app-universal-debug.apk --clear
    # --clear erases the app's data (the tests need the signed-out state); --upgrade also runs the update check
    # (it uninstalls and reinstalls the app); --offline skips the VK steps when the device has no internet

Without `--apk` it takes the newest debug APK under `src-tauri/gen/android/app/build/outputs/apk`. Results go to
`scripts/android-emulator-tests/out`. To call one command in the running app:
`node scripts/android-emulator-tests/invoke.mjs app_info`.

### Download network cost

Android reads the active network’s metered flag locally (`ACCESS_NETWORK_STATE`, a normal permission).
Manual downloads ask before using metered traffic or when cost cannot be determined. Consent lasts
for the current action; batch downloads recheck before each new file. Offline requests stop before
starting a file. Automatic playlist sync waits on metered/unknown networks without opening prompts;
paused rules are checked locally every 30 seconds while the app is visible, and again on focus/visibility
return. Once Android reports an unmetered connection, sync resumes without a background consent
dialog. While waiting, these probes do not contact VK. The pause reason appears in Downloads;
unknown network cost remains paused. Cancelling consent or changing accounts
prevents pending downloads from starting. Already downloaded files can still be removed or manually
protected from sync cleanup without a network request. An in-flight file may finish if the network
changes while that file is downloading.

### Playlist recommendations

Creation and editing include an optional VK recommendations choice (`no_discover`), matching iOS.
The default sends no flag, preserving VK's current/default policy. Excluding a playlist from
recommendations does not make its link private. The choice travels in the same single-attempt
metadata write; changing accounts while the dialog is open invalidates submission. VK does not
consistently return this flag in playlist lists, so reopening the form offers “Keep current”.

### Share audio files

Android track menus offer **Share audio file** separately from sharing a VK link. The app uses an
native-verified offline file or downloads the track (with the metered-network check), then stages a copy
in its private `audio-share` cache. The system chooser receives an AAC/MP3 `content://` URI with a
read-only grant. It cannot access login state, other downloads or the MP3 picker directory. Internal
paths never go through the WebView. Account changes abort preparation before opening the chooser;
repeated taps on the same track share one preparation. Stale offline metadata cannot trigger an
unapproved download: the first native call probes with downloads disabled. Unshared copies are removed immediately;
shared copies remain available for a day and are cleaned during later preparations. Opening the
chooser does not mean a file was sent. AAC is remuxed to M4A without re-encoding; MP3 keeps its original audio frames. Prepared copies
include title, artist, album, available artwork and saved lyrics. Timed lyrics also produce an LRC
sidecar, plain lyrics a UTF-8 text file; both files use the same scoped read grant. Artwork is optional
and uses the bounded public-HTTPS image loader. Offline export continues without artwork if loading
fails. Verify recipient app compatibility on a real Android device; local tests cover staging, access
boundaries, native compilation, metadata readback and UI.

### Own playlist covers

Android owners can choose a JPEG/PNG/WebP image (up to 10 MB) through the system document picker.
The native decoder checks bounds before allocating pixels, samples to roughly 1024 px, applies
camera rotation/mirroring, crops the center square and recompresses JPEG without the source metadata.
The dialog previews that exact image and requires Save. No image path reaches the WebView; the small
preview and an account-scoped upload handle are temporary. Cancel/navigation/account changes discard
the handle and prevent late picker/upload results from changing the interface. Stop during upload
is best effort: a binding already accepted by VK can still finish. Ambiguous writes invalidate the
handle and ask the user to refresh before trying again.

The engine gets `photos.getAudioPlaylistCoverUploadServer`, posts field `photo` only to the accepted
VK HTTPS hosts without following redirects, and binds its opaque `photo`/`hash` via a single
`audio.setPlaylistCoverPhoto` call. Ownership is checked before upload and binding; cancellation is
checked again after ownership resolution. A confirmed save is kept distinct from a later list
refresh failure. VK may take time to return the new artwork URL. Preview images are not persisted
in pins or web storage. Phone owner actions wrap without overlapping neighboring hit targets.

### Emulator host failures

CI starts the emulator in its own session with a supervisor. Each attempt writes the real child PID
and an exit record (`emulator-exit-N.txt`), including signal-derived status. The host watcher follows
that PID rather than matching its own command line. Rendering uses the current `software` backend;
`swiftshader_indirect` is deprecated by Android. This change does not establish the cause of an earlier
runner disappearance. Losing adb fails the active step and skips dependent checks. Failed log retrieval
cannot report a clean crash check. Guest artifact collection stops promptly when the device is gone;
host/live logs remain available. Run the supervisor regression checks without an SDK using
`node --test scripts/android-emulator-tests/supervisor.test.mjs`.

### Saved lyrics and offline search

Opening a nonempty lyric automatically keeps an account-local copy (including a chosen version or
personal LRC). Search → Lyrics finds all query words across artist, title and text, ignoring case and
accents; it makes no VK request. The excerpt opens a read-only reader, so reading another song never
seeks the currently playing track. Playback has its own button and resolves current library/queue
metadata first. The reader closes on navigation or account change.

The index keeps at most 150 tracks within one million UTF-16 code units; each lyric is limited to
150 KB UTF-8, with bounded timing lines. Track snapshots contain no playback URLs or VK access keys.
If browser storage fills up, older cached lyrics are evicted first; a failed write preserves the previous
cache. These are local cached copies, not a backup or a sync service. Older personal LRC files join the
index when opened. Reset removes the saved copy before fetching a fresh version. Offline availability
of lyrics does not imply that the audio file is downloaded.

### Downloaded audio file details

Downloaded tracks expose **Audio file details** in the track menu. The native store supplies AAC/MP3
format, recorded size, save date and current file availability. A missing, truncated, replaced directory
or symlink is not reported as playable. No cache filename/path crosses IPC. Reading the details never
starts playback or a network download. Play rechecks the file; Android sharing uses the cached-only
native call and disables itself if that copy disappears. Removal requires a separate confirmation inside
the same window, removes only the downloaded copy and refreshes Downloads. Account changes/navigation
close the window and invalidate delayed metadata responses. The availability check is a snapshot; a file
can still change after checking.

Downloaded file details also offer **Keep separately**, an account-local manual retention flag. This
requires an existing native-verified file and does not download another copy. The action is disabled while
playlist sync is running. Details distinguish a separately kept
copy from a regular local file. Sync cleanup rechecks changed manual retention between deletion IPC
calls, preserving a copy marked manually while an earlier deletion was in flight. Explicit removal
and Clear downloads still remove files at the user's request. The footer stays visible while long
file details scroll independently on a small screen.

Android CI caches Cargo registry data and build outputs using compiler, runner architecture, pinned NDK
and Cargo manifest/lockfile keys. Each source revision gets a new cache entry, with dependency-compatible
fallbacks for later revisions. Cargo still checks changed Rust/UI assets. SDK installations, account
credentials and signed APKs are outside these paths. The first run has no cache; savings can only be
measured after a later cache hit.

The 37.2.12 runner also reproduced a host emulator SIGSEGV (exit 139) during sign-in; the app
startup, Keystore and generated cover decoder checks passed before the host disappeared. Software
rendering alone did not resolve it. CI now disables guest Vulkan following Android's Chromium
compatibility guidance, and retains privileged kernel tail/limited emulator Crashpad dumps for a
further diagnosis. This is a test-environment workaround, not evidence of a fixed app bug.

### Copying lyrics

Hold a timed lyric line for 550 ms or use its context-menu gesture to copy that line as plain text.
The release is consumed so it does not also seek. Moving, changing tracks, hiding the panel or changing
accounts cancels an unfinished hold. Regular player-state updates leave the hold intact. A separately
labelled copy button copies the currently displayed version; the saved-lyrics reader has its own Copy
lyrics action and a fixed footer. Plain text can also be selected. Android uses a dedicated clipboard
command bounded to 150 KB UTF-8; the stricter VK-link clipboard command remains separate. The emulator
run exercises the native write with generated text and rejects an empty copy, without reading the
user's clipboard.


### Phone feature parity (iPhone reference ecf7adb)

The scope is the phone application. Widgets, Wear OS, WatchKit and Apple-only system surfaces are
outside this change. Existing library/search/catalog, queue/radio, recommendations, playlists/import,
manual downloads/sync, statistics/history, sleep timer, lyrics/search/editing and sharing remain in place.
The Android player keeps its album-first layout and low-cost static background.

Phone settings now include all 23 iPhone equalizer presets (plus the historical Android Night preset
and Custom), preamp −12…+6 dB, dynamic-range compression, stereo widening/intensity, normalization,
prefetch/gapless and crossfade 0.5…10 seconds. Preamp and stereo transitions are smoothed. Compression
and stereo use the same native DSP as iPhone; the default bypass is bit-exact. These are stereo effects,
not head tracking. HQ is on by default; disabling it selects the best audio-only HLS variant up to
256 kbps, or the lowest available if all exceed it. Progressive files retain source quality. Changing
quality applies to the next source and invalidates neighbor preparation.

Bluetooth resume and pause at zero music-stream volume are implemented by the process-owned Android
media module. Registering a route callback does not resume an already connected device. Only a newly
connected Bluetooth output can resume a loaded, paused track, outside communication mode. Zero-volume
pause does not automatically resume when volume rises. Unplug/noisy and audio-focus policies still
apply. The unavailable-track advance switch preserves the existing bounded recovery attempts;
disabling it keeps the failed track selected. Connectivity failures never skip the queue.

Appearance adds four text sizes, the Home player-card switch and haptic feedback control. Font sizes
and line heights scale together; virtual row heights and list arithmetic remain stable. Settings
switches use the whole labelled row as a touch target, and horizontal sliders have a 44 px target.
The Android manifest includes the normal vibration permission.

Automatic downloads are account-local and disabled by default. Likes use unmetered connections and
recheck cost between tracks. Automatic cache copies a completed playback buffer on a worker: it never
starts a second audio fetch. An incomplete buffer stays pending; leaving that track cancels its pending
cache request. The 1/2/4/8/16/32 GB limit prevents new automatic cache writes beyond the available
budget. Cleanup evicts least-recently-played cache files owned by this account, protecting the current
track, manual files, liked downloads and pinned playlists across accounts. Unowned/corrupt retention
metadata grants no deletion rights. Protected/manual files may exceed the limit. Signing out disposes
schedulers and invalidates work; a delayed initial profile still starts the account's saved policies.

VK status broadcast and listening reporting are separate, explicit account-local opt-ins, both off by
default. Playback-start/stop events remain in memory only, use the same event vocabulary as iPhone and
stop after opt-out. Integration errors stop that integration until a preference change or Retry; they
never interrupt music. Account IDs are checked again at the native API boundary. Uncertain writes are
not retried automatically. The Settings section exposes an error and Retry action.

Audio export prepares tagged MP3/M4A on a worker and shares saved text/LRC when available. Metadata
readback with FFprobe and decoded-audio MD5 comparison verify that adding metadata preserves audio.
Android MediaExtractor/MediaMuxer compilation and container/tag tests pass locally; actual device
remuxing, Bluetooth routing, system-volume observation and receiving-app compatibility still need
physical-device validation. Local host checks do not establish a frame-rate or battery measurement on
the user's phone.

Validation added in `ui-tests/ios-phone-parity.test.js`, `ui-tests/audio-export-lyrics.test.js`, native
DSP/HLS tests, offline-buffer caching/recovery/VK integration tests and Android export-tag tests. The
full interface run exposed two issues (new touch targets and an outdated sharing argument assertion);
both were corrected and their targeted runs passed. Small/large/landscape layouts, enlarged text,
permission opt-out, delayed profiles, unmetered resume, cross-account file protection and scroll/player
performance have dedicated checks. Run the UI suite with `npm test` inside `ui-tests`, the engine with
`cargo test -p pm_engine`, and the standalone media plugin with its `scripts/android-test.sh`.

Phone screen audit covers 28 main routes and 10 additional sign-in, loading, empty, error and dialog
states. Browser screenshots were reviewed at 360×640 dark, 412×915 light and 320×568 with 120% text;
regression checks also walk all main screens at 915×412 with 120% text. Toolbars and catalog headings
wrap, search tabs retain readable labels, Settings stays inside the viewport and navigation titles
leave room for actions. Equalizer frequency navigation exposes all ten bands with 44px buttons.
Alert actions retain readable accent text without the general primary-button gradient. Tests live in
`ui-tests/screen-audit.test.js`; browser emulation does not replace physical Android device validation.
