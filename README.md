# Private Music for desktop

The desktop twin of the iOS app: an unofficial VK Music player for Windows,
Linux and macOS. Rust engine + [Tauri](https://tauri.app) shell + a small web UI
that looks like the iOS app and moves softly. It reuses the iOS app's `native/` crate (HLS parsing, AES-128, MPEG-TS
and CMAF demuxing, VK stream unmasking), so both clients play streams the same
way. `native/` is a copy of the one in decoder-dev/PrivateMusic2: keep them in sync.

## Features

- VK sign-in in an isolated window; the token lives in the OS keyring and is
  renewed from the session cookie when it expires.
  If the VK page is signed in but the player is still waiting, use **I'm signed
  in — check again** in the player to request completion without losing the web session.
- My music, playlists, search, a queue with shuffle and repeat; the queue and
  position survive a restart.
- Hearts: add any track to My music or take it out, from lists, the player bar
  or the full-screen view.
- Artist and album pages: click an artist in any row, the player bar or the
  full-screen view (or use *Go to artist* / *Go to album* in a track's menu) to
  see their popular tracks and albums, with more loaded as you scroll; an album page plays the release and
  saves it to your albums. Search has Artists and Albums tabs, and followed
  albums are listed under Playlists. Community albums whose access key VK
  withholds are resolved the way the iOS app does it (search for the key, and
  failing that, rebuild the album from public search results).
- For you: VK's personal mix, recommendations and catalogue mixes; "similar tracks" on any row
  plays VK's radio seeded with that track; the personal mix can be tuned by
  mood, activity or genre. A "New releases" shelf shows VK's fresh albums as
  cards that open the album page.
- Endless radio: a queue started from the personal mix, a catalogue mix or
  "similar tracks" keeps filling from the same source while it plays. When six
  tracks or fewer are left, the next page is fetched (the next stream window,
  or recommendations seeded with a track the radio queued), tracks already
  queued or played are skipped, and at most 80 are kept ahead of the current
  one. A source that fails or has nothing new is given up on after three
  attempts in a row, and the queue then simply ends. The Queue view says when it
  is a mix. Playing anything else turns the radio off.
- Search remembers recent queries, which can be reused, removed or cleared.
- Friends' music: the friends list with photos; open a friend to see their tracks and
  playlists (a playlist opens into its tracks), play from there, and use the hearts and
  the track menu as anywhere else. A friend who keeps their audio private gets a plain
  message instead of VK's error.
- History and statistics, kept on this computer only: a track lands in **History**
  (the last 250, newest first, filterable, removable one by one or all at once) once
  30 % of it has really been listened to, as in the iOS app (seeking and pausing do
  not count). **Statistics** shows time listening, plays and days with music, top
  artists and tracks, and when you listen (hour of the day, favourite weekday, busiest
  day) for a month, a year or all time, with a reset button. Nothing is sent to VK
  (the iOS app's optional "report playback to VK" setting is not ported).
- Downloads: any track can be saved to the app's folder and then plays with no
  internet (the file is the decrypted audio, not VK's encrypted stream).
- A ten-band equaliser with presets.
- Full-screen "now playing": blurred cover backdrop in the cover's colours, a
  live spectrum of what is actually playing (bars, mirror, wave, radial), and
  synced lyrics (LRCLIB, VK text, then Genius fallback) that follow the song; click a line to
  jump there, pause auto-scroll or return to the current line.
- OS integration: media keys and Now Playing (MPRIS / SMTC / MediaRemote), a
  tray icon with controls, keep-running-in-the-tray on close, one instance only,
  window size and position remembered.
- Settings: sound output device (switches without losing the place), equaliser,
  tray, full-screen defaults, theme, accent colour and keyboard shortcuts.

## Install

Installers are built by CI and attached to GitHub releases. The easiest way is
the install script for your system: it downloads the latest release, checks its
SHA-256, installs it and (on macOS) clears the quarantine flag, so the unsigned
build opens without warnings. Windows' SmartScreen does not stop a file that a
script downloaded either.

The repository is private, so the scripts need a token: set `GITHUB_TOKEN`, or
run `gh auth login` once and they pick it up.

| System | Command |
| --- | --- |
| Windows (PowerShell) | `.\install.ps1` (`-Silent` for no window, `-Uninstall` to remove) |
| macOS | `./install.sh` (`--uninstall` to remove) |
| Linux | `./install.sh` (`.deb` through apt on Debian/Ubuntu, otherwise an AppImage in `~/.local`) |

Installing by hand works too (download from the release page), but the builds
are not code-signed: on Windows choose *More info → Run anyway*, on macOS
right-click the app and choose *Open*.

Windows installs per user (no administrator rights), in Russian or English
depending on the system language, with a Start menu entry and an uninstaller.

**Updating** is the same as installing: run the new installer (or `install.ps1`)
over the old version. It replaces the program in place; the saved sign-in (it
lives in Windows Credential Manager, not in the install folder) and the app's
data stay, so you are still signed in. CI proves this on Windows for every build
and every release: it installs the last release, plants a sign-in and data
files, installs the new installer over it and fails if anything is gone
(`scripts/upgrade-check.ps1`).

### Making a release

1. Raise `version` in `Cargo.toml` and `src-tauri/tauri.conf.json` (they must match).
2. Push a tag (`git tag v0.2.0 && git push origin v0.2.0`), or run the *Release*
   workflow by hand on GitHub with the tag name: it creates the tag itself.
3. The workflow builds Windows (`-setup.exe`), macOS (universal `.dmg`) and Linux
   (`.deb`, `.AppImage`) installers and publishes them with `SHA256SUMS.txt` as
   the latest release, which is what the install scripts download.

Installer artwork is generated by `scripts/make-installer-art.py` from the app icon.

## Layout

| Path | What |
| --- | --- |
| `engine/` | Everything except pixels: VK client, stream fetch/decrypt/demux, decode (symphonia), resample (rubato), output (cpal), queue and playback policy (`controller`), sign-in/token refresh (`app`), secret store (OS keyring). No UI or Tauri dependency; fully testable headless. |
| `src-tauri/` | The shell: window, IPC commands, isolated VK sign-in window, media keys (`souvlaki`). |
| `ui/` | Plain ES modules, no framework, no build step. Looks like the iOS app; short, system-like motion that stops under reduced motion (see *UI design*). |
| `ui-tests/` | Playwright tests of the UI against a mock backend. |

## UI design

The look follows the iOS app: the classic palette, nine accent colours with a
readable label on each (black on the dark-theme accents, white on the light
ones), the same radii and elevations. The tokens are in `ui/css/tokens.css`
(each group names the iOS file it comes from).

- **Motion is soft and short**: fades on hover and press, a settle when a screen,
  menu or dialog appears, a slide when an artist or album page is pushed, the
  full-screen view rising like a sheet. Nothing moves on its own while idle; the
  live spectrum in the full-screen view is the only continuous animation.
  Everything stops under the system's *reduce motion* (read live, not once).
- **Track lists never animate**: they are virtualised and recreate rows while
  scrolling, so a row or anything in it never has a transition.
- **Blur is for chrome only**: the player bar, menus, dialogs and toasts are glass.
  It turns into an opaque surface under *reduce transparency* and *increased
  contrast*, with the *Simplified look* switch in Settings, when the window has
  been in the background for a few seconds, and where `backdrop-filter` is missing.
- **Heroes have atmosphere, not boxes**: artist, album and playlist headers
  stand on two soft washes taken from the cover's palette (the engine already
  extracts it), drawn as radial gradients, so there is no blur to repaint. If the
  palette cannot be read the header stays plain. The full-screen player keeps its
  single small blurred raster backdrop, and its cover steps back when paused.
- **Rhythm and type**: a 4/8 pt scale, tabular numerals for times and counts,
  `text-wrap: balance` on headings and `pretty` on notes, one weight per control
  (selection never changes a width: see the theme picker), and header controls that
  fold into tidy full-width rows in narrow windows instead of wrapping one by one.
- **Depth**: layered soft shadows with a hairline top highlight, concentric radii,
  accent-tinted selection, focus rings with a soft halo; glass only on chrome.
- **Entrances are one-shot and capped**: shelves and cards rise one after another
  (eight steps, about half a second in all), cards lift on hover (transform and
  shadow only), the play/pause and heart glyphs pop when they change, slider
  thumbs grow while pointed at, bars and share bars grow once in Statistics.
  Skeletons sweep once while loading. All of it is finite, removed by
  *reduce motion*, and the tests assert that an idle screen has no running animation.
- **States**: empty and failed panels show a small theme- and accent-aware
  illustration (inline SVG) near the top of the area, never a big blank panel; a
  shelf that fails is one line with a retry.
- **Components**: the theme picker previews the real UI (sidebar, rows, player bar)
  in each theme, with "system" split along the true diagonal by a clipped layer
  (anti-aliased), a constant label weight, a check badge, 44 px hit areas and
  radio-group keyboard behaviour (arrows, one tab stop). Tooltips (`ui/js/tooltip.js`)
  replace the native `title` bubble after a short rest of the mouse and show
  shortcut hints (`data-kbd`). Buttons have primary, secondary, tertiary and
  destructive forms, a small size and a loading state (`aria-busy`); scrollbars are
  thin and appear only while the pointer is over the scroller.
- **Fonts**: SF on macOS, Segoe UI (Variable) on Windows; Linux uses the bundled
  Inter (`ui/fonts`, SIL Open Font License, Latin and Cyrillic subsets).
- Nothing inline: styles come from classes (and `style.setProperty` from code),
  so the strict `style-src 'self'` policy holds.

## Run and test

Linux needs `libwebkit2gtk-4.1-dev libgtk-3-dev libasound2-dev libdbus-1-dev libssl-dev`; the
tests also need `ffmpeg` (they generate HLS fixtures).

```sh

cargo run -p private-music-desktop                    # debug app
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                                # uses a simulated sound card
cargo test --release --test cpu_cost -- --nocapture   # CPU / wake-up budget
(cd ui-tests && npm install && npx playwright install chromium && npm test)
PM_SCREENSHOTS=/tmp/shots npm test --prefix ui-tests   # also saves screenshots
cd src-tauri && cargo tauri build                     # installers
```

Optional tests: `live_vk` (real VK endpoints, needs network) and `cpal_smoke`
(real audio backend, e.g. ALSA null device) are `#[ignore]`d by default.

## VPN and network changes

VK ties API tokens, signed stream URLs and cookies to the network the client was on,
so switching a VPN on or off (or Wi-Fi to cable) while the app runs leaves stale
connections and stream addresses behind. The app heals itself, as far as it can:

- **Network watcher.** While audio plays or the window is focused (and never otherwise:
  the thread sleeps without wake-ups) the engine compares, every 8 s, the local addresses
  the OS would use to reach the internet (a UDP `connect`, no packet is sent; IPv4 to a
  generic and to a VK address, IPv6 by /64). A change drops every pooled connection,
  marks all cached stream URLs stale and tells the window (`network`: `changed`, then `ok`
  once it holds still for ~2.5 s, or `unreachable`).
- **Playback.** A stream that fails (403/404/410/401, connection reset, timeout, DNS, TLS,
  or audio that stays dry for 20 s) is resumed at the same position from a fresh
  `audio.getById` URL on new connections, with exponential back-off and jitter
  (connection problems: 6 attempts, 0.4 s then 1.2 s doubling to 20 s; dead addresses:
  3 attempts, the first immediate; iOS rules: one `getById` per attempt, no extra
  retries inside it). A connection problem never skips tracks: after the last attempt
  playback pauses with an error and **Try again**, and starts by itself when the network
  settles. A dead track is skipped as before (at most three in a row). Downloaded tracks
  are never touched. The queue and position are kept throughout.
- **API calls.** After a transport error the pooled connections are dropped. Reads are
  retried (3 tries, back-off with jitter); writes (likes, playlist changes, follow) are
  never retried. A session VK stops accepting (error 5, 28, or "validation required",
  17) is renewed once from the cookie and the call repeated once; no loops. Lyrics and
  cover requests retry once and otherwise fail silently.
- **Window.** A network failure (timeout, transport) on a screen, in playback or in the
  sign-in window shows "Не удаётся подключиться к VK" with the hint to try the VPN off (or
  on), **Try again** and "How to check your VPN" (Windows/macOS/Linux). It is a
  suggestion, never a claim that a VPN was detected, and it is not shown for sign-in,
  refused-request or track errors. Failed loads repeat by themselves when the network
  returns, and "Connection restored" is shown once. If VK's login page does not load
  within 25 s the sign-in window reports it and Try again reloads it (same restrictions:
  VK hosts over https only).
- **Logs** name each step without URLs or tokens: `stream 403 → refreshing url (attempt
  2/3)`, `network changed: dropping connections`, `recovered after 3.2 s`.

## Security model

- **Tokens** live only in the OS keyring (Secret Service / Credential Manager /
  Keychain). If it is unavailable the sign-in is kept in memory for the run and
  the UI says it will not persist; nothing is ever written to disk in clear.
- **Sign-in** happens on vk.ru in a separate, incognito webview whose
  navigation is restricted to VK hosts over https; only the `web_token` exchange
  result leaves it. The main window cannot be navigated away from the bundled UI.
- **CSP** is strict (`default-src 'self'`, no inline script, no `eval`);
  capabilities grant the main window only the app's own commands.
- **IPC** arguments are validated in the engine (numbers finite and in range,
  artist ids and album locators/keys plain tokens before they are sent to VK,
  stream URLs https-only and never loopback, private or link-local addresses (also across redirects); no redirects on VK API calls, where a
  token is attached).
- Logs never contain tokens or cookies.
- **Listening history** is one small JSON file per VK account in the app data folder
  (`history/listening-<VK user id>.json`): the last 250 tracks (title, artist, cover
  address and the ids VK uses for them, plus a track's access key where VK gave one,
  which is needed to play it again), and per-month counters. It never holds a token, a
  cookie or a stream address, it is bounded in size, it is never sent anywhere, and
  *Clear* (History) and *Reset statistics* remove their parts of it. A damaged or hand-edited file is
  set aside or repaired on load.

## Known limits

- CI checks page loading and private cookie access on Windows, macOS and Linux.
  Signing in to a real account and OS media controls still need manual verification.
- CMAF streams with encryption are untested against real VK data.
- `install.ps1` has not been run on a real Windows machine yet.
