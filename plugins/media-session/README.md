# tauri-plugin-media-session

Android background playback for Private Music: a **foreground service**, a
**MediaSession** (notification, lock screen, Bluetooth, headset buttons) and
**audio focus**, for an app whose sound comes from the Rust engine
(decode, then cpal/oboe), not from Android's `MediaPlayer`/ExoPlayer.

Without this, Android throttles or kills the process shortly after the screen
turns off, and the system has no idea that anything is playing. On every other
platform the plugin compiles to a no-op, so the host needs no `cfg` of its own.

```
 engine UiEvent::State ──► bridge::Bridge ──► MediaSession::update ─┐  (Rust → Kotlin)
                           (MediaSync dedup)                         ▼
                                              MediaSessionPlugin → MediaHub → PlaybackService
                                                                       │  MediaSessionCompat, notification,
                                                                       │  AudioFocusController, wake/Wi-Fi locks
 engine Controller  ◄── bridge::dispatch ◄── action handler ◄─────────┘  (Kotlin → Rust, a Tauri Channel)
```

## Layout

| Path | What |
| --- | --- |
| `src/lib.rs` | Public types (`Update`, `MediaAction`, ...), `init()`, `MediaSessionExt` |
| `src/bridge.rs` | Engine glue: `Bridge`, `dispatch`, `Ducker` (pure Rust, unit-tested on the host) |
| `src/mobile.rs` / `src/desktop.rs` | Android implementation / no-op everywhere else |
| `android/` | Kotlin library module (`com.dec.privatemusic.mediasession`) |
| `permissions/` | `default.toml` (+ generated command permissions) |
| `scripts/android-test.sh` | Kotlin unit tests without an app or emulator |

## Public Rust API

```rust
// Registered with:  .plugin(tauri_plugin_media_session::init())
pub fn init<R: Runtime>() -> TauriPlugin<R>;

pub trait MediaSessionExt<R: Runtime> { fn media_session(&self) -> &MediaSession<R>; } // app.media_session()

impl<R: Runtime> MediaSession<R> {
    pub fn set_action_handler(&self, handler: impl Fn(MediaAction) + Send + Sync + 'static) -> Result<()>;
    pub fn update(&self, update: &Update) -> Result<()>;          // non-blocking (queued, in order)
    pub fn set_artwork_url(&self, url: Option<&str>) -> Result<()>;
    pub fn request_audio_focus(&self) -> Result<()>;              // also done implicitly when playback starts
    pub fn abandon_audio_focus(&self) -> Result<()>;
    pub fn stop(&self) -> Result<()>;                             // drop notification, focus, service
    pub async fn status(&self) -> Result<Status>;                 // notifications enabled? battery exemption?
    pub async fn open_battery_settings(&self) -> Result<()>;
}

pub struct TrackMetadata { title, artist, album: Option<String>, artwork_url: Option<String>, duration_secs: f64 }
pub enum   PlaybackKind  { Playing, Paused, Buffering, Stopped }
pub struct PlaybackInfo  { state: PlaybackKind, position_secs: f64, duration_secs: f64, rate: f64 }
pub struct Update        { metadata: Option<TrackMetadata>, clear_metadata: bool, playback: Option<PlaybackInfo> }
pub enum   MediaAction   { Play, Pause, Toggle, Next, Previous, Stop, SeekTo(f64), SeekBy(f64), Duck(bool) }
pub struct Status        { notifications_enabled, ignoring_battery_optimizations, available }
pub trait  MediaSink     { fn update(&self, update: &Update) -> Result<()>; }
pub struct AppSink<R>(pub AppHandle<R>);                          // MediaSink through the app handle

// bridge.rs
pub struct Bridge<S: MediaSink>;  // new(sink), on_event(&UiEvent, now_secs), on_state(&State, now_secs)
pub fn dispatch(controller: &Controller, ducker: &Ducker, action: MediaAction);
pub fn to_engine_action(action: MediaAction) -> Option<pm_engine::media_sync::MediaAction>; // None for Duck
pub struct Ducker;                // new(gain) / default() = 0.25; set(&impl Volume, on: bool)
pub trait Volume { fn volume(&self) -> f32; fn set_volume(&self, f32); } // implemented for Controller
```

Wire format of `MediaAction` (Kotlin to Rust, one JSON object per event on a Tauri
`Channel`): `{"action":"play"}`, `{"action":"seek_to","value":42.5}` (seconds),
`{"action":"seek_by","value":-10}`, `{"action":"duck","value":true}`.

Semantics worth knowing:

* `update` carries **state changes only**. `Bridge` reuses `MediaSync`, so steady
  playback sends nothing: the position is sent on state, seek and track changes and
  Android's `PlaybackState` (position + speed) lets the system scrubber advance on its
  own. Loading and buffering are reported as `Buffering` (spinner, speed 0), which
  `MediaSync` itself folds into "playing".
* `metadata: None, clear_metadata: false` means "unchanged"; `clear_metadata: true`
  means nothing is loaded.
* Artwork: the plugin downloads `artwork_url` itself, on a background thread, **https
  only** (also on redirects), at most 2 MiB and 6000 px on the source side, scaled to
  512 px, no private/loopback hosts, one bitmap cached, URLs never logged.
  `set_artwork_url` overrides the URL of the metadata.
* `MediaAction::Duck(true)` is sent when another app asks us to be quieter (a
  navigation prompt); `Duck(false)` when it is over. The audio never passes through the
  JVM, so **only the host can lower it**: `bridge::dispatch` does it with `Ducker`
  (volume x 0.25, restored afterwards unless the user moved the slider meanwhile). A
  host that prefers its own gain applies it on `Duck` itself. Handle it idempotently.
  Android 8+ may also duck by itself for apps that do not pause when ducked; both are
  harmless together.

## Integration (what the integrator does)

1. **Dependency**, `src-tauri/Cargo.toml` (unconditional is fine, it is a no-op off Android):

   ```toml
   tauri-plugin-media-session = { path = "../plugins/media-session" }
   ```

2. **Register the plugin**, in the builder in `src-tauri/src/lib.rs`:

   ```rust
   .plugin(tauri_plugin_media_session::init())
   ```

3. **Capability**, add `"media-session:default"` to the mobile capability file
   (`src-tauri/capabilities/*.json`). It only grants the two webview commands
   (`status`, `open_battery_settings`); the playback commands are Rust-only.

4. **Android build**: nothing to copy. `tauri android init/build` picks the Kotlin module
   up from the crate (`build.rs` declares `android_path("android")`) and Gradle's manifest
   merger adds the service and the permissions of
   `android/src/main/AndroidManifest.xml` (`FOREGROUND_SERVICE`,
   `FOREGROUND_SERVICE_MEDIA_PLAYBACK`, `POST_NOTIFICATIONS`, `WAKE_LOCK`, `INTERNET`;
   service `foregroundServiceType="mediaPlayback"`). Do not strip them with
   `tools:node="remove"`. To use the app's own status-bar icon, add a drawable named
   `ic_stat_music` to the app resources (it overrides the placeholder). `targetSdk` 35 is fine.

5. **Engine to plugin**: feed every `UiEvent` through a `Bridge` from the closure that
   already forwards events to the webview (the emitter must not block; `update` only
   enqueues):

   ```rust
   use std::{sync::Mutex, time::Instant};
   use tauri_plugin_media_session::{bridge::Bridge, AppSink};

   let started = Instant::now();
   let bridge = Mutex::new(Bridge::new(AppSink(app_handle.clone())));
   let emit = move |event: UiEvent| {
       // ... existing forwarding to the UI ...
       if let Err(error) = bridge.lock().unwrap_or_else(|p| p.into_inner())
           .on_event(&event, started.elapsed().as_secs_f64())
       {
           log::debug!("media session: {error}");   // never log the update itself
       }
   };
   ```

6. **Plugin to engine**: once the `Controller` exists (same place `media.rs` attaches the
   desktop controls):

   ```rust
   use tauri_plugin_media_session::{bridge, MediaSessionExt};

   let ducker = bridge::Ducker::default();
   let target = app_handle.clone();
   app_handle.media_session().set_action_handler(move |action| {
       if let Some(state) = target.try_state::<AppState>() {
           bridge::dispatch(state.app.controller(), &ducker, action);
       }
   })?;
   ```

   `dispatch` goes through `pm_engine::media_sync::apply`, exactly like the desktop
   media keys. Call `set_action_handler` before the first `update`.

7. **Settings screen (optional)**: `invoke("plugin:media-session|status")` returns
   `{ notificationsEnabled, ignoringBatteryOptimizations, available }`;
   `invoke("plugin:media-session|open_battery_settings")` opens the system list.

8. **Stop on logout / quit**: `app.media_session().stop()`.

No change to `engine/` is needed. (`Controller::set_volume` and `media_sync::apply`
are used as they are.)

## Behaviour

**Foreground service.** `PlaybackService` (`mediaPlayback`) is started on the first
`Playing`/`Buffering` update. Timeline after a pause: foreground for 30 s (a call or a
short interruption keeps the process at full priority so a focus gain can resume),
then the service lets go of foreground (`STOP_FOREGROUND_DETACH`; the notification stays,
is no longer ongoing and can be swiped), and stops after 10 min. Swiping the notification
while not playing stops it at once. `Stopped` leaves after 5 s (a track change that
passes through "stopped" does not kill it). These numbers are `ForegroundPolicy.Config`.

* `onStartCommand` always calls `startForeground` for a start from
  `startForegroundService` (a missing call crashes the app), returns `START_NOT_STICKY`
  (a restart without the app would have nothing to show).
* `onTimeout(startId, fgsType)` (API 35) and `onTimeout(startId)` (API 34) stop
  the sound (`Stop` action), leave foreground and `stopSelf()`. `mediaPlayback` has no
  timeout today; the handler is there so that this stays true if Android changes.
* `onTaskRemoved`: playing/buffering, the service keeps going (the engine lives in
  the same process and keeps playing, as music apps do); paused or stopped, it stops.
* Re-entering foreground from the background (focus gain, after the detach) can be
  refused on API 31+ (`ForegroundServiceStartNotAllowedException`); this is caught,
  logged and degrades to a plain notification. Sound is unaffected while the process lives.
* A `PARTIAL_WAKE_LOCK` (6 h safety timeout, renewed on every playback message) and a
  high-performance `WifiLock` are held **only** while playing/buffering.

**Notification.** Channel `playback` ("Playback"), `IMPORTANCE_LOW`, no sound, no badge,
public on the lock screen, `onlyAlertOnce`, ongoing only while playing, buttons
previous / play-pause / next, `MediaStyle` bound to the session token, tinted from the
artwork's average colour (Android 12+ draws media notifications itself and ignores the
tint). Tapping opens the app. On Android 13+ the system builds its media controls from
the `PlaybackState` actions, which are advertised too (play, pause, play/pause, next,
previous, stop, seek-to when the duration is known).

**Notification permission** (`POST_NOTIFICATIONS`, API 33+): asked once, the first time
playback starts while the Activity is in the foreground. Denial only hides the
notification: Android 13+ still runs the foreground service and the sound continues
(it then shows only in the system's Task Manager). It is not asked a second time;
`status()` tells a settings screen when to send the user to system settings.

**Audio focus** (`USAGE_MEDIA`, `CONTENT_TYPE_MUSIC`, `setWillPauseWhenDucked(false)`,
delayed focus accepted), as a pure state machine (`FocusStateMachine`):

| Event | Reaction |
| --- | --- |
| playback starts | request focus; denied: `Pause`; delayed: `Pause`, resume on gain |
| `LOSS` (another player took over) | `Pause`, release focus, never resumes by itself |
| `LOSS_TRANSIENT` (call, alarm) | `Pause`, remember "paused by focus" |
| `LOSS_TRANSIENT_CAN_DUCK` | `Duck(true)` while playing |
| `GAIN` | `Duck(false)`; `Play` only if paused by focus |
| `ACTION_AUDIO_BECOMING_NOISY` | `Pause`, never resumes by itself |
| user pauses / presses play meanwhile | the automatic resume is cancelled |
| idle (detach) / stopped | focus abandoned (kept only while waiting for a transient gain) |

**Media buttons.** Headset single/double/triple click (play-pause / next / previous) is
handled by `MediaSessionCompat`; `KEYCODE_MEDIA_PLAY/PAUSE/PLAY_PAUSE/NEXT/PREVIOUS/STOP/
FAST_FORWARD/REWIND` are mapped by us regardless of which actions the state advertises.
While the service is running the active session receives the buttons even with the
screen off or the app in the background. Once the service has stopped (paused for more
than 10 min) a button press cannot restart a dead engine; there is deliberately no
`MediaButtonReceiver`.

## Why `MediaSessionCompat` and not Media3

Media3's `MediaSession` wants a `Player` that owns playback; here the engine does, so a
`SimpleBasePlayer` would have to mirror every state and answer commands it cannot
execute, and `MediaSessionService` starts and stops the foreground state by itself
(from the mirrored player state), which fights the policy above (grace after a pause,
swipe to stop, `onTimeout`, `onTaskRemoved`) and cannot be tested without a device. It
also pulls ExoPlayer-sized dependencies (guava, media3-common) into a Tauri app for a
no-op player. `MediaSessionCompat` (`androidx.media:media`) is the same framework
`MediaSession` underneath, supported on API 26 to 35, shown by the system notification
and lock screen, advertises exactly the actions we set, and leaves the foreground
service, the notification and the timers in plain code we control and unit test.
Migrating later is local: only `PlaybackService` and `NotificationFactory` touch it.

## Battery and "keeps playing with the screen off"

* The foreground service of type `mediaPlayback` is what keeps playback alive; the
  notification is its visible part (see the permission note above).
* Stock Android (Doze, App Standby) does not stop a foreground media service. Many OEM
  battery savers (Xiaomi/MIUI/HyperOS, Huawei, Oppo/Realme, Vivo, Samsung "sleeping
  apps", OnePlus) kill apps in the background anyway, including foreground services.
  Nothing in the app can fix that; the user has to allow background activity for the
  app ("no restrictions", "autostart", "lock in recents", depending on the maker; see
  dontkillmyapp.com). Offer `open_battery_settings` (the system's *ignore battery
  optimisations* list) from a settings screen with that explanation.
* The plugin **never requests the exemption itself** and does not declare
  `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` (Play policy). `open_battery_settings` only opens
  the list, with a fallback to the main settings.
* Streaming needs the network while the screen is off: the Wi-Fi lock covers Wi-Fi power
  saving; mobile data saver and per-app data restrictions are the user's.
* If the process is killed anyway, nothing restarts it (`START_NOT_STICKY`) and the
  notification disappears with it.

## Tests

```sh
cargo test -p tauri-plugin-media-session          # Rust: wire types + bridge with a fake sink
plugins/media-session/scripts/android-test.sh     # Kotlin unit tests (JUnit, no emulator)
```

`android-test.sh` runs `gradlew testDebugUnitTest` in `android/`, a standalone build of
the library (`settings.gradle.kts`, AGP 8.9.3, Kotlin 2.1.21, compileSdk 35). It needs
`ANDROID_HOME` with platform 35 (and 36 for Tauri's own `tauri-android` module, which
Gradle downloads if the licences are accepted), JDK 17+, and finds Tauri's Android
library in the cargo registry (`-PtauriAndroidDir` / `TAURI_ANDROID_DIR`). Use
`GRADLE=gradle` to run an installed Gradle instead of downloading the wrapper.
Add `compileDebugKotlin`/`assembleDebug` to only build.

The Kotlin tests cover the pure logic: focus state machine, foreground policy, artwork
URL/size rules, state and action mapping, notification layout, wire parsing, permission
policy. The Android glue (`PlaybackService`, `AudioFocusController`, `NotificationFactory`,
`MediaHub`, the plugin) compiles but is **not** tested without a device.

## Not verifiable without a device

Real behaviour of: notification and lock-screen rendering on Android 12 to 15, Bluetooth
AVRCP metadata, headset click counting, audio focus races with calls and other players,
foreground-start refusals from the background, OEM task killers, the permission dialog,
and whether Android's automatic ducking applies to oboe/AAudio streams (the `Duck` event
makes the host independent of that). Smoke-test on a phone: play, lock the screen, wait
5 minutes; pull the headphones; take a call; swipe the app from recents while playing and
while paused; deny the notification permission and play again.
