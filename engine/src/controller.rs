//! The brain of the app: queue + player + VK, behind a small thread-safe API.
//!
//! The UI layer only calls methods here and listens to [`UiEvent`]s. Policy
//! lives here, where it can be tested without a window:
//!
//! * a track that ends starts the next one (repeat/shuffle aware);
//! * a stream that fails is recovered (see [`RecoveryPolicy`]): VK stream URLs
//!   expire and are bound to the network that asked for them, so the track is
//!   resumed where it broke from a fresh URL, with bounded back-off. A
//!   connection problem never skips tracks: after the bound playback pauses
//!   with an error and a retry; a dead track is skipped, at most three times
//!   in a row;
//! * network and API calls never run on the audio thread or under the state
//!   lock.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Serialize;

use crate::history::History;
use crate::net::{self, NetState};
use crate::audio::{PlayerEvent, PlayerHandle, Remote, Sink};
use crate::offline::OfflineStore;
use crate::queue::{Queue, Repeat};
use crate::radio::{self, Radio, RadioFetch, RadioKind};
use crate::stream::FetchConfig;
use crate::vk::{Session, Track, VkClient};
use crate::Error;

const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Pressing Previous after this many seconds restarts the track instead.
const RESTART_THRESHOLD: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
}

#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub status: Status,
    pub track: Option<Track>,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub queue_index: Option<usize>,
    pub queue_len: usize,
    /// Set while the queue refills itself from a mix or a seeded radio.
    pub radio: Option<RadioKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // events are short-lived and moved once
pub enum UiEvent {
    State(State),
    Queue { tracks: Vec<Track>, current: Option<usize> },
    Error { kind: String, message: String },
    /// The VK token no longer works: show the sign-in screen.
    SessionExpired,
    /// Visualizer levels (0…1), about 30 per second while it is on.
    Spectrum { bands: Vec<f32> },
    /// The route to the internet changed, vanished or settled (see [`crate::net`]).
    Network { state: NetState },
    /// Playback is healing itself after a failed stream.
    Recovery { phase: RecoveryPhase, attempt: u32, max: u32, kind: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryPhase {
    /// A new attempt is scheduled or running.
    Retrying,
    /// The bound was reached: playback is paused and waits for a retry.
    Failed,
    /// The track has been playing again for a few seconds.
    Recovered,
}

/// How stubbornly a broken stream is recovered. The numbers follow the iOS
/// app's `StreamFailureRetryPolicy` (3 same-track attempts for dead URLs, a
/// wider budget for connection failures, never skipping on those) with
/// exponential back-off and jitter on top.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryPolicy {
    /// Attempts after a connection failure (reset, timeout, DNS, TLS, 5xx).
    pub connectivity_attempts: u32,
    /// Attempts after the address stopped working (401/403/404/410, bad data).
    pub content_attempts: u32,
    /// Audio that stays dry this long counts as a failed stream.
    pub stall_after: Duration,
    /// Seconds of audio after a recovery before it counts as recovered (a
    /// stream that dies again sooner continues the same attempt count).
    pub probation_secs: f64,
    /// Multiplies every wait; tests shrink it.
    pub time_scale: f64,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            connectivity_attempts: 6,
            content_attempts: 3,
            stall_after: Duration::from_secs(20),
            probation_secs: 3.0,
            time_scale: 1.0,
        }
    }
}

impl RecoveryPolicy {
    pub fn attempts(&self, connectivity: bool) -> u32 {
        if connectivity { self.connectivity_attempts } else { self.content_attempts }
    }

    /// Wait before attempt number `attempt` (1-based). A dead address is
    /// re-resolved at once the first time, as always; a bad connection gets a
    /// moment first. Later attempts back off: 1.2, 2.4, 4.8 … seconds, up to
    /// 20 (6 for dead addresses), ±15 %.
    pub fn delay(&self, connectivity: bool, attempt: u32, unit: f64) -> Duration {
        let unit = if unit.is_finite() { unit.clamp(0.0, 1.0) } else { 0.5 };
        let seconds = match (connectivity, attempt) {
            (false, 0 | 1) => 0.0,
            (true, 0 | 1) => 0.4 * (0.85 + 0.3 * unit),
            (_, n) => net::backoff(n - 2, 1.2, if connectivity { 20.0 } else { 6.0 }, 0.15, unit),
        };
        self.scaled(seconds)
    }

    fn scaled(&self, seconds: f64) -> Duration {
        let scale = if self.time_scale.is_finite() { self.time_scale.max(0.0) } else { 1.0 };
        Duration::from_secs_f64((seconds * scale).clamp(0.0, 3600.0))
    }

    /// After the network came back: a short jittered start, not the back-off
    /// that belonged to the dead path.
    pub fn network_return_delay(&self, unit: f64) -> Duration {
        let unit = if unit.is_finite() { unit.clamp(0.0, 1.0) } else { 0.5 };
        self.scaled(0.1 + 0.4 * unit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// The connection or route is bad: wait, drop connections, try again.
    Connectivity,
    /// The address is dead or the data is bad: ask VK for a new one.
    Content,
    /// Nothing recovery can fix (access denied, sign-in needed, …).
    Fatal,
}

fn classify(error: &Error) -> Class {
    match error {
        Error::Timeout | Error::Offline | Error::Transport(_) => Class::Connectivity,
        Error::Http(408 | 429 | 500..=599) => Class::Connectivity,
        // 6/9/10: too many requests, flood control, VK internal error.
        Error::Vk { code: 6 | 9 | 10, .. } => Class::Connectivity,
        Error::Http(_) | Error::Stream(_) => Class::Content,
        _ => Class::Fatal,
    }
}

/// A log-safe name for a failure (never a URL).
fn what(error: &Error) -> String {
    match error {
        Error::Http(status) => status.to_string(),
        Error::Stream(_) => "stream error".into(),
        Error::Timeout => "timeout".into(),
        Error::Offline => "connect failed".into(),
        Error::Transport(_) => "connection lost".into(),
        other => other.kind().into(),
    }
}

struct Inner {
    queue: Queue,
    session: Option<Session>,
    status: Status,
    position: f64,
    duration: f64,
    volume: f32,
    rng: StdRng,
    failures: u32,
    /// Tracks already retried with a fresh URL during this attempt.
    refreshed: HashSet<String>,
    /// Where the next start of the current track begins (a restored session).
    pending_start: f64,
    /// The endless source the queue refills from, if it came from one.
    radio: Option<Radio>,
    radio_generation: u64,
    /// The failure being healed, if any.
    recovery: Option<Recovery>,
    /// Bumped to cancel an attempt that is waiting out its back-off.
    recovery_token: u64,
    /// Playback gave up on a connection problem and resumes by itself when
    /// the network settles.
    gave_up: bool,
    /// Bumped to cancel the stall watchdog of an earlier buffering.
    stall_token: u64,
    /// Bumped by every network change; URLs resolved under an older value are
    /// not trusted (VK ties them to the network that asked).
    network_epoch: u64,
    /// Track id → network epoch its stream URL was resolved under.
    fresh: HashMap<String, u64>,
    /// The current track plays from a downloaded file: nothing here applies.
    source_offline: bool,
    loading_since: Instant,
}

struct Recovery {
    track_id: String,
    /// Failures so far, which is also the number of the attempt they asked for.
    attempts: u32,
    /// The bound for the kind of failure seen last, and its name for the window.
    max: u32,
    kind: &'static str,
    started: Instant,
    /// Set when audio flows again: the position it restarted from.
    probation_from: Option<f64>,
}

struct Shared {
    inner: Mutex<Inner>,
    client: VkClient,
    remote: OnceLock<Remote>,
    emit: Box<dyn Fn(UiEvent) + Send + Sync>,
    fetch_template: FetchConfig,
    refresher: OnceLock<Refresher>,
    offline: OnceLock<Arc<OfflineStore>>,
    history: OnceLock<Arc<History>>,
    policy: Mutex<RecoveryPolicy>,
}

/// Renews an expired session (see [`Controller::set_refresher`]).
pub type Refresher = Arc<dyn Fn(&Session) -> crate::Result<Session> + Send + Sync>;

pub struct Controller {
    shared: Arc<Shared>,
    _player: PlayerHandle,
}

impl Inner {
    fn state(&self) -> State {
        State {
            status: self.status,
            track: self.queue.current().cloned(),
            position: self.position,
            duration: self.duration,
            volume: self.volume,
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            queue_index: self.queue.current_index(),
            queue_len: self.queue.len(),
            radio: self.radio.as_ref().map(|r| r.kind),
        }
    }

    fn queue_event(&self) -> UiEvent {
        UiEvent::Queue { tracks: self.queue.tracks().to_vec(), current: self.queue.current_index() }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn remote(&self) -> &Remote {
        self.remote.get().expect("controller is fully built")
    }

    /// Runs `f` under the lock, then emits what it returned after unlocking.
    fn with<R>(&self, f: impl FnOnce(&mut Inner, &mut Vec<UiEvent>) -> R) -> R {
        let mut events = Vec::new();
        let result = {
            let mut inner = self.lock();
            f(&mut inner, &mut events)
        };
        for event in events {
            (self.emit)(event);
        }
        result
    }

    fn fetch_config(&self, inner: &Inner) -> FetchConfig {
        let mut config = self.fetch_template.clone();
        if let Some(session) = &inner.session {
            config.user_agent = session.api_user_agent.clone();
        }
        config.offline_dir = self.offline.get().map(|o| o.dir().to_path_buf());
        config
    }

    /// Starts the queue's current track, or stops when there is none.
    fn start_current(self: &Arc<Self>, inner: &mut Inner, events: &mut Vec<UiEvent>) {
        // Whatever was being healed belongs to what was playing before.
        self.cancel_recovery(inner, events);
        if let Some(radio) = inner.radio.as_mut() {
            radio.waiting = false;
        }
        self.maybe_refill(inner);
        let Some(track) = inner.queue.current().cloned() else {
            inner.status = Status::Idle;
            self.remote().stop();
            events.push(UiEvent::State(inner.state()));
            return;
        };
        let start = std::mem::take(&mut inner.pending_start).clamp(0.0, f64::from(track.duration));
        // A fresh start: listening to it counts from zero again.
        if let Some(history) = self.history.get() {
            history.restart();
        }
        inner.status = Status::Loading;
        inner.loading_since = Instant::now();
        inner.position = start;
        inner.duration = f64::from(track.duration);
        events.push(UiEvent::State(inner.state()));
        // A downloaded copy plays from disk. If its file has gone, the player
        // fails and the usual retry fetches the track from VK instead.
        let downloaded = if inner.refreshed.contains(&track.id()) {
            None
        } else {
            self.offline.get().and_then(|o| o.address(&track.id()))
        };
        if let Some(address) = downloaded {
            // No credentials, no network: nothing about VPNs concerns it.
            inner.source_offline = true;
            let config = self.fetch_config(inner);
            self.remote().play(&track.id(), &address, f64::from(track.duration), start, config);
            return;
        }
        inner.source_offline = false;
        match track.stream_url.clone() {
            Some(url) if !self.is_stale(inner, &track) => {
                let config = self.fetch_config(inner);
                self.remote().play(&track.id(), &url, f64::from(track.duration), start, config);
            }
            // No address, or one resolved before the network changed.
            _ => {
                inner.pending_start = start;
                self.refresh_then_play(inner, track)
            }
        }
    }

    /// A URL resolved under an earlier network epoch is not trusted.
    fn is_stale(&self, inner: &Inner, track: &Track) -> bool {
        inner.session.is_some() && inner.network_epoch > 0 && inner.fresh.get(&track.id()) != Some(&inner.network_epoch)
    }

    fn policy(&self) -> RecoveryPolicy {
        *self.policy.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Asks VK for a fresh stream URL on a worker thread, then plays it.
    fn refresh_then_play(self: &Arc<Self>, inner: &mut Inner, track: Track) {
        let Some(session) = inner.session.clone() else {
            // Without a session there is nothing to ask; behave like a failure.
            let shared = self.clone();
            std::thread::spawn(move || shared.fail_track(&track, Error::Unauthorized));
            return;
        };
        inner.refreshed.insert(track.id());
        let shared = self.clone();
        std::thread::spawn(move || {
            let mut result = shared.client.refreshed_track_once(&session, &track);
            // An expired token is renewed from the stored cookie, not treated
            // as a sign-out, exactly like the app's own calls.
            if result == Err(Error::Unauthorized) {
                if let Some(refresh) = shared.refresher.get() {
                    result = refresh(&session).and_then(|fresh| shared.client.refreshed_track_once(&fresh, &track));
                }
            }
            match result {
                Ok(fresh) => shared.with(|inner, events| {
                    // The user may have moved on while VK was answering.
                    if inner.queue.current().map(Track::id) != Some(track.id()) {
                        return;
                    }
                    inner.queue.replace_track(&fresh);
                    inner.fresh.insert(fresh.id(), inner.network_epoch);
                    let config = shared.fetch_config(inner);
                    let start = std::mem::take(&mut inner.pending_start);
                    if let Some(url) = &fresh.stream_url {
                        shared.remote().play(&fresh.id(), url, f64::from(fresh.duration), start, config);
                    }
                    events.push(UiEvent::State(inner.state()));
                }),
                Err(error) => shared.stream_failed(track, error),
            }
        });
    }

    /// Starts a radio round on a worker thread when the queue is running low.
    fn maybe_refill(self: &Arc<Self>, inner: &mut Inner) {
        let upcoming = inner.queue.upcoming();
        let Some(radio) = inner.radio.as_mut() else { return };
        if !radio.wants_refill(upcoming) {
            return;
        }
        radio.in_flight = true;
        let generation = radio.generation;
        let shared = self.clone();
        std::thread::spawn(move || shared.refill(generation));
    }

    /// One radio round: up to a few requests until something new arrives.
    fn refill(self: &Arc<Self>, generation: u64) {
        let mut added_total = 0;
        let mut failure = None;
        for attempt in 0..radio::ATTEMPTS {
            let step = self.with(|inner, _| {
                inner.radio.as_ref().filter(|r| r.generation == generation).map(|r| (r.next_request(), r.fetch.clone()))
            });
            let Some((request, fetch)) = step else { return };
            let Some(request) = request else { break };
            let proposed = match fetch(&request) {
                Ok(tracks) => tracks,
                Err(error) => {
                    log::debug!("radio refill: {}", error.kind());
                    failure = Some(error);
                    break;
                }
            };
            let added = self.with(|inner, events| {
                let Inner { queue, radio, .. } = inner;
                let radio = radio.as_mut().filter(|r| r.generation == generation)?;
                radio.answered(&request);
                let fresh = radio.fresh(proposed);
                let room = radio::UPCOMING_LIMIT.saturating_sub(queue.upcoming());
                let appended = queue.append_unique(fresh, room);
                radio.record(&appended);
                if !appended.is_empty() {
                    events.push(inner.queue_event());
                    events.push(UiEvent::State(inner.state()));
                }
                Some(appended.len())
            });
            match added {
                None => return,
                Some(0) if attempt + 1 < radio::ATTEMPTS => std::thread::sleep(radio::ATTEMPT_PAUSE),
                Some(0) => {}
                Some(n) => {
                    added_total = n;
                    break;
                }
            }
        }
        self.with(|inner, events| {
            let Some(radio) = inner.radio.as_mut().filter(|r| r.generation == generation) else { return };
            radio.in_flight = false;
            radio.finish_round(added_total, failure.as_ref());
            if std::mem::take(&mut radio.waiting) {
                // The queue ran dry while this round was on its way.
                if added_total > 0 && inner.queue.advance(false).is_some() {
                    inner.failures = 0;
                    inner.refreshed.clear();
                    events.push(inner.queue_event());
                    self.start_current(inner, events);
                } else {
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    events.push(UiEvent::State(inner.state()));
                }
            } else if added_total > 0 {
                // Still short after this page: ask for the next one.
                self.maybe_refill(inner);
            }
        });
    }

    /// A track cannot be played: report it and move on, within limits.
    fn fail_track(self: &Arc<Self>, track: &Track, error: Error) {
        self.with(|inner, events| self.fail_track_in(inner, events, track, error));
    }

    fn fail_track_in(self: &Arc<Self>, inner: &mut Inner, events: &mut Vec<UiEvent>, track: &Track, error: Error) {
        if inner.queue.current().map(Track::id) != Some(track.id()) {
            return;
        }
        if error == Error::Unauthorized {
            events.push(UiEvent::SessionExpired);
        }
        events.push(UiEvent::Error { kind: error.kind().to_string(), message: error.to_string() });
        let skippable = matches!(error, Error::Stream(_) | Error::Http(_) | Error::Format(_) | Error::Vk { .. } | Error::Unavailable);
        inner.failures += 1;
        if skippable && inner.failures < MAX_CONSECUTIVE_FAILURES && inner.queue.advance(false).is_some() {
            inner.refreshed.clear();
            events.push(inner.queue_event());
            self.start_current(inner, events);
        } else {
            self.cancel_recovery(inner, events);
            inner.status = Status::Idle;
            self.remote().stop();
            events.push(UiEvent::State(inner.state()));
        }
    }

    /// Forgets the failure being healed (a new track started, the user
    /// stopped, …) and any attempt still waiting out its back-off.
    fn cancel_recovery(&self, inner: &mut Inner, events: &mut Vec<UiEvent>) {
        inner.recovery_token += 1;
        inner.stall_token += 1;
        inner.gave_up = false;
        if let Some(recovery) = inner.recovery.take() {
            if recovery.probation_from.is_some() {
                Self::announce_recovered(&recovery, events);
            }
        }
    }

    fn announce_recovered(recovery: &Recovery, events: &mut Vec<UiEvent>) {
        log::info!("recovered after {:.1} s ({} attempt{})", recovery.started.elapsed().as_secs_f64(), recovery.attempts, if recovery.attempts == 1 { "" } else { "s" });
        events.push(UiEvent::Recovery {
            phase: RecoveryPhase::Recovered,
            attempt: recovery.attempts,
            max: recovery.max,
            kind: recovery.kind.to_string(),
        });
    }

    /// A stream could not be fetched, decoded or resolved. Decides between
    /// healing it (fresh URL, fresh connections, back-off), skipping a dead
    /// track, and pausing with a retry once the bound is reached.
    fn stream_failed(self: &Arc<Self>, track: Track, error: Error) {
        let policy = self.policy();
        self.with(|inner, events| {
            if inner.queue.current().map(Track::id) != Some(track.id()) {
                return;
            }
            // A downloaded copy that cannot be read: fetch it from VK once.
            if std::mem::take(&mut inner.source_offline) {
                if inner.session.is_some() && !inner.refreshed.contains(&track.id()) && matches!(error, Error::Stream(_) | Error::Http(_)) {
                    inner.pending_start = inner.position;
                    self.refresh_then_play(inner, track);
                } else {
                    self.fail_track_in(inner, events, &track, error);
                }
                return;
            }
            let class = classify(&error);
            if inner.session.is_none() || class == Class::Fatal {
                self.fail_track_in(inner, events, &track, error);
                return;
            }
            let connectivity = class == Class::Connectivity;
            if connectivity {
                // Whatever sockets are pooled belong to a route that failed.
                net::drop_connections();
            }
            let max = policy.attempts(connectivity);
            let kind = if connectivity { "network" } else { "stream" };
            let id = track.id();
            let recovery = match inner.recovery.take() {
                Some(r) if r.track_id == id => r,
                _ => Recovery { track_id: id, attempts: 0, max, kind, started: Instant::now(), probation_from: None },
            };
            let recovery = inner.recovery.insert(Recovery { attempts: recovery.attempts + 1, max, kind, probation_from: None, ..recovery });
            let (attempt, started) = (recovery.attempts, recovery.started);
            if attempt > max {
                inner.recovery = None;
                inner.recovery_token += 1;
                if !connectivity {
                    // This address will not work however often it is asked
                    // for: a dead track, skipped within the usual limits.
                    log::warn!("stream {}: still failing after {max} fresh URLs, giving up on this track", what(&error));
                    self.fail_track_in(inner, events, &track, error);
                    return;
                }
                log::warn!("stream {}: still failing after {max} attempts ({:.1} s), pausing", what(&error), started.elapsed().as_secs_f64());
                inner.gave_up = true;
                inner.pending_start = inner.position;
                inner.status = Status::Idle;
                self.remote().stop();
                events.push(UiEvent::Error { kind: error.kind().to_string(), message: error.to_string() });
                // Only a real connection failure points at the network (and a
                // VPN); a service that keeps answering 5xx is another matter.
                let kind = if net::is_connectivity(&error) { "network" } else { "service" };
                events.push(UiEvent::Recovery { phase: RecoveryPhase::Failed, attempt: max, max, kind: kind.into() });
                events.push(UiEvent::State(inner.state()));
                return;
            }
            let delay = policy.delay(connectivity, attempt, net::jitter_unit());
            log::info!("stream {} → refreshing url (attempt {attempt}/{max}, in {:.1} s)", what(&error), delay.as_secs_f64());
            events.push(UiEvent::Recovery { phase: RecoveryPhase::Retrying, attempt, max, kind: kind.into() });
            inner.status = Status::Loading;
            inner.loading_since = Instant::now();
            events.push(UiEvent::State(inner.state()));
            self.schedule_attempt(inner, track.id(), delay);
        });
    }

    /// Runs the next recovery attempt for `track_id` after `delay`, unless it
    /// is cancelled (a new track, a stop, a network change that replaced it).
    fn schedule_attempt(self: &Arc<Self>, inner: &mut Inner, track_id: String, delay: Duration) {
        inner.recovery_token += 1;
        let token = inner.recovery_token;
        let shared = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            shared.with(|inner, events| {
                if inner.recovery_token != token || inner.session.is_none() {
                    return;
                }
                let Some(track) = inner.queue.current().cloned().filter(|t| t.id() == track_id) else { return };
                // Carry on where it broke, not from the beginning.
                inner.pending_start = inner.position;
                inner.status = Status::Loading;
                inner.loading_since = Instant::now();
                events.push(UiEvent::State(inner.state()));
                shared.refresh_then_play(inner, track);
            });
        });
    }

    /// Audio ran dry and stayed dry: treat it like a failed stream.
    fn watch_stall(self: &Arc<Self>, inner: &mut Inner, track_id: String) {
        inner.stall_token += 1;
        let token = inner.stall_token;
        let policy = self.policy();
        let after = policy.scaled(policy.stall_after.as_secs_f64());
        let shared = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let stalled = shared.with(|inner, _| {
                (inner.stall_token == token && inner.status == Status::Buffering && !inner.source_offline)
                    .then(|| inner.queue.current().cloned().filter(|t| t.id() == track_id))
                    .flatten()
            });
            if let Some(track) = stalled {
                log::warn!("playback stalled for {:.0} s, treating it as a failed stream", after.as_secs_f64());
                shared.stream_failed(track, Error::Timeout);
            }
        });
    }

    fn on_network(self: &Arc<Self>, state: NetState) {
        match state {
            NetState::Changed => self.network_changed(),
            NetState::Ok => self.network_settled(),
            NetState::Unreachable => {}
        }
    }

    /// Every stream URL resolved so far belongs to the old network: each
    /// remote track is asked for a fresh one before it plays next. Whatever
    /// plays from a downloaded file is untouched.
    fn network_changed(&self) {
        self.with(|inner, _| {
            inner.network_epoch += 1;
            let queued = inner.queue.len();
            log::info!("network changed: stream URLs of {queued} queued track(s) are stale, fresh ones are asked for before they play");
        });
    }

    /// The network is stable again: a stream that was waiting out a back-off,
    /// a stalled one, and a playback that gave up all start over at once
    /// (after a short random pause, so nothing stampedes).
    fn network_settled(self: &Arc<Self>) {
        let shared = self;
        let policy = self.policy();
        self.with(|inner, events| {
            let Some(track) = inner.queue.current().cloned() else { return };
            if inner.source_offline || inner.session.is_none() {
                return;
            }
            let waiting = inner.recovery.is_some() || inner.gave_up;
            let stalled = inner.status == Status::Buffering
                || (inner.status == Status::Loading && inner.loading_since.elapsed() > policy.scaled(3.0));
            if !waiting && !stalled {
                return;
            }
            log::info!("network is back: restarting {}", if inner.gave_up { "the paused playback" } else { "the stream" });
            if inner.recovery.is_none() {
                inner.recovery = Some(Recovery {
                    track_id: track.id(),
                    attempts: 0,
                    max: policy.connectivity_attempts,
                    kind: "network",
                    started: Instant::now(),
                    probation_from: None,
                });
            }
            inner.gave_up = false;
            inner.pending_start = inner.position;
            inner.status = Status::Loading;
            inner.loading_since = Instant::now();
            events.push(UiEvent::State(inner.state()));
            let delay = policy.network_return_delay(net::jitter_unit());
            shared.schedule_attempt(inner, track.id(), delay);
        });
    }

    fn on_player_event(self: &Arc<Self>, event: PlayerEvent) {
        match event {
            PlayerEvent::Position { track_id, seconds, duration } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.position = seconds;
                    let probation = self.policy().probation_secs;
                    if inner.recovery.as_ref().and_then(|r| r.probation_from).is_some_and(|from| seconds >= from + probation) {
                        if let Some(recovery) = inner.recovery.take() {
                            Self::announce_recovered(&recovery, events);
                        }
                    }
                    if duration > 0.0 {
                        inner.duration = duration;
                    }
                    if let (Some(history), Some(track)) = (self.history.get(), inner.queue.current()) {
                        history.tick(track, seconds, inner.duration, inner.status == Status::Playing);
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Spectrum { bands } => {
                let bands = bands.iter().map(|b| (b * 1000.0).round() / 1000.0).collect();
                (self.emit)(UiEvent::Spectrum { bands });
            }
            PlayerEvent::Loading { .. } => {}
            PlayerEvent::Playing { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.status = Status::Playing;
                    inner.failures = 0;
                    inner.refreshed.remove(&track_id);
                    let position = inner.position;
                    if let Some(recovery) = inner.recovery.as_mut().filter(|r| r.track_id == track_id) {
                        // Audio flows again; it counts as recovered after a few seconds.
                        recovery.probation_from = Some(position);
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Paused { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.status = Status::Paused;
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Buffering { track_id, on } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str())
                    && matches!(inner.status, Status::Playing | Status::Buffering)
                {
                    inner.status = if on { Status::Buffering } else { Status::Playing };
                    if on && !inner.source_offline {
                        self.watch_stall(inner, track_id.clone());
                    } else {
                        inner.stall_token += 1;
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Stopped => {}
            PlayerEvent::Ended { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() != Some(track_id.as_str()) {
                    return;
                }
                if inner.queue.advance(true).is_some() {
                    events.push(inner.queue_event());
                    self.start_current(inner, events);
                } else if inner.radio.as_ref().is_some_and(|r| !r.done) {
                    // An endless queue is never over: wait for its next page.
                    if let Some(radio) = inner.radio.as_mut() {
                        radio.waiting = true;
                    }
                    inner.status = Status::Loading;
                    inner.position = 0.0;
                    events.push(UiEvent::State(inner.state()));
                    self.maybe_refill(inner);
                } else {
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    inner.pending_start = 0.0;
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Failed { track_id, error } => {
                let current = {
                    let inner = self.lock();
                    inner.queue.current().cloned().filter(|t| t.id() == track_id)
                };
                if let Some(track) = current {
                    self.stream_failed(track, error);
                }
            }
        }
    }
}

impl Controller {
    /// `make_sink` builds the output device on the audio thread;
    /// `emit` receives every [`UiEvent`] (it must not block).
    pub fn new<F>(
        client: VkClient,
        make_sink: F,
        emit: impl Fn(UiEvent) + Send + Sync + 'static,
        volume: f32,
    ) -> Self
    where
        F: FnMut() -> crate::Result<Box<dyn Sink>> + Send + 'static,
    {
        Self::with_fetch(client, make_sink, emit, volume, FetchConfig::new("PrivateMusicDesktop"))
    }

    /// Like [`Controller::new`] with an explicit stream-fetch configuration
    /// (tests allow a loopback server this way).
    pub fn with_fetch<F>(
        client: VkClient,
        make_sink: F,
        emit: impl Fn(UiEvent) + Send + Sync + 'static,
        volume: f32,
        fetch_template: FetchConfig,
    ) -> Self
    where
        F: FnMut() -> crate::Result<Box<dyn Sink>> + Send + 'static,
    {
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                queue: Queue::default(),
                session: None,
                status: Status::Idle,
                position: 0.0,
                duration: 0.0,
                volume: volume.clamp(0.0, 1.0),
                rng: StdRng::from_entropy(),
                failures: 0,
                refreshed: HashSet::new(),
                pending_start: 0.0,
                radio: None,
                radio_generation: 0,
                recovery: None,
                recovery_token: 0,
                gave_up: false,
                stall_token: 0,
                network_epoch: 0,
                fresh: HashMap::new(),
                source_offline: false,
                loading_since: Instant::now(),
            }),
            client,
            remote: OnceLock::new(),
            emit: Box::new(emit),
            fetch_template,
            refresher: OnceLock::new(),
            offline: OnceLock::new(),
            history: OnceLock::new(),
            policy: Mutex::new(RecoveryPolicy::default()),
        });
        let callback_shared = shared.clone();
        let player = PlayerHandle::spawn(make_sink, move |event| callback_shared.on_player_event(event));
        let _ = shared.remote.set(player.remote());
        player.set_volume(volume.clamp(0.0, 1.0));
        Self { shared, _player: player }
    }

    pub fn set_session(&self, session: Option<Session>) {
        self.shared.with(|inner, events| {
            let signed_out = session.is_none();
            inner.session = session;
            if signed_out {
                self.shared.cancel_recovery(inner, events);
                inner.radio = None;
                inner.queue.clear();
                inner.status = Status::Idle;
                self.shared.remote().stop();
                events.push(inner.queue_event());
                events.push(UiEvent::State(inner.state()));
            }
        });
    }

    /// How the controller renews an expired token when it asks VK for a
    /// fresh stream address. Set once by the app.
    pub fn set_refresher(&self, refresher: Refresher) {
        let _ = self.shared.refresher.set(refresher);
    }

    /// Changes how stubbornly broken streams are recovered (tests shorten the waits).
    pub fn set_recovery_policy(&self, policy: RecoveryPolicy) {
        *self.shared.policy.lock().unwrap_or_else(|p| p.into_inner()) = policy;
    }

    /// The route to the internet changed, settled or vanished (see [`net`]).
    pub fn on_network(&self, state: NetState) {
        self.shared.on_network(state);
    }

    /// The same as [`on_network`](Self::on_network) for a thread that outlives
    /// this borrow (the network monitor).
    pub fn network_listener(&self) -> impl Fn(NetState) + Send + Sync + 'static {
        let shared = self.shared.clone();
        move |state| shared.on_network(state)
    }

    /// The user's "try again" for a playback that failed: fresh connections,
    /// a fresh URL, the same position, a full set of attempts.
    pub fn retry(&self) {
        self.shared.with(|inner, events| {
            if inner.queue.current().is_none() || inner.source_offline {
                return;
            }
            let position = inner.position;
            net::drop_connections();
            inner.network_epoch += 1;
            inner.failures = 0;
            inner.refreshed.clear();
            self.shared.cancel_recovery(inner, events);
            inner.pending_start = position;
            self.shared.start_current(inner, events);
        });
    }

    /// Where downloaded tracks are played from. Set once by the app.
    pub fn set_offline(&self, store: Arc<OfflineStore>) {
        let _ = self.shared.offline.set(store);
    }

    /// Where plays are recorded for the history and statistics. Set once by the app.
    pub fn set_history(&self, history: Arc<History>) {
        let _ = self.shared.history.set(history);
    }

    /// The download settings playback uses (user agent, size cap).
    pub fn fetch_config(&self) -> FetchConfig {
        let inner = self.shared.lock();
        self.shared.fetch_config(&inner)
    }

    pub fn session(&self) -> Option<Session> {
        self.shared.lock().session.clone()
    }

    pub fn state(&self) -> State {
        self.shared.lock().state()
    }

    pub fn play_tracks(&self, tracks: Vec<Track>, index: usize) {
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.replace(tracks, index, rng);
            inner.radio = None;
            inner.failures = 0;
            inner.refreshed.clear();
            events.push(inner.queue_event());
            self.shared.start_current(inner, events);
        });
    }

    /// Plays a queue that keeps refilling from its source (see [`radio`]).
    /// `offset` is where a stream mix continues, `seeds` the tracks a seeded
    /// radio starts asking about; `tracks` count as already heard.
    pub fn play_radio(&self, tracks: Vec<Track>, index: usize, kind: RadioKind, fetch: RadioFetch, offset: u32, seeds: Vec<Track>) {
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.replace(tracks, index, rng);
            inner.radio_generation += 1;
            inner.radio = Some(Radio::new(kind, inner.radio_generation, fetch, offset, seeds, inner.queue.tracks()));
            inner.failures = 0;
            inner.refreshed.clear();
            events.push(inner.queue_event());
            self.shared.start_current(inner, events);
        });
    }

    /// Puts a queue in place without playing it (a restored session). The
    /// next play starts at `position` seconds.
    pub fn load_queue(&self, tracks: Vec<Track>, index: usize, position: f64) {
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.replace(tracks, index, rng);
            inner.radio = None;
            inner.status = Status::Idle;
            inner.pending_start = if position.is_finite() { position.max(0.0) } else { 0.0 };
            inner.position = inner.pending_start;
            inner.duration = inner.queue.current().map(|t| f64::from(t.duration)).unwrap_or(0.0);
            events.push(inner.queue_event());
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn pause(&self) {
        self.shared.with(|inner, events| {
            if inner.recovery.is_some() && inner.status == Status::Loading {
                // Pausing while the stream is being healed stops the healing;
                // Play starts again from here.
                let position = inner.position;
                self.shared.cancel_recovery(inner, events);
                inner.pending_start = position;
                inner.status = Status::Idle;
                self.shared.remote().stop();
                events.push(UiEvent::State(inner.state()));
            } else {
                self.shared.remote().pause();
            }
        });
    }

    pub fn resume(&self) {
        self.shared.remote().resume();
    }

    pub fn toggle(&self) {
        let status = self.shared.lock().status;
        match status {
            Status::Playing | Status::Buffering => self.pause(),
            Status::Paused => self.resume(),
            Status::Idle => {
                // Start whatever is queued, from the top of the track.
                self.shared.with(|inner, events| {
                    if inner.queue.current().is_some() {
                        self.shared.start_current(inner, events);
                    }
                });
            }
            Status::Loading => {}
        }
    }

    pub fn seek(&self, seconds: f64) {
        self.shared.with(|inner, events| {
            if inner.queue.current().is_none() || !seconds.is_finite() {
                return;
            }
            let target = seconds.max(0.0);
            inner.position = if inner.duration > 0.0 { target.min(inner.duration) } else { target };
            if inner.status == Status::Idle {
                // Nothing is playing: the next Play starts here.
                inner.pending_start = inner.position;
            }
            events.push(UiEvent::State(inner.state()));
        });
        self.shared.remote().seek(seconds);
    }

    /// Switches to another sound output (the shell's `make_sink` decides
    /// which): the device is reopened and playback carries on where it was.
    pub fn reopen_output(&self) {
        self.shared.with(|inner, events| {
            self.shared.remote().reopen_sink();
            let had_track = inner.queue.current().is_some();
            match inner.status {
                Status::Playing | Status::Buffering | Status::Loading if had_track => {
                    inner.pending_start = inner.position;
                    self.shared.start_current(inner, events);
                }
                _ => {
                    // Paused or idle: the next Play starts from here on the new output.
                    if had_track {
                        inner.pending_start = inner.position;
                    }
                    inner.status = Status::Idle;
                    events.push(UiEvent::State(inner.state()));
                }
            }
        });
    }

    /// Equaliser gains in dB (see `audio::eq::BANDS`); `None` turns it off.
    pub fn set_equalizer(&self, gains: Option<[f32; 10]>) {
        self.shared.remote().set_equalizer(gains);
    }

    /// Starts or stops the spectrum events. Off by default: analysing audio
    /// costs CPU that only the full-screen view needs.
    pub fn set_visualizer(&self, on: bool) {
        self.shared.remote().set_visualizer(on);
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        self.shared.with(|inner, events| {
            inner.volume = volume;
            events.push(UiEvent::State(inner.state()));
        });
        self.shared.remote().set_volume(volume);
    }

    pub fn next(&self) {
        self.shared.with(|inner, events| {
            if inner.queue.advance(false).is_some() {
                inner.failures = 0;
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn previous(&self) {
        let restart = {
            let inner = self.shared.lock();
            inner.queue.current().is_some() && inner.position > RESTART_THRESHOLD
        };
        if restart {
            self.seek(0.0);
            return;
        }
        self.shared.with(|inner, events| {
            if inner.queue.previous().is_some() {
                inner.failures = 0;
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.shared.start_current(inner, events);
            }
        });
    }

    /// Plays the track at `index` of the queue as listed.
    pub fn jump(&self, index: usize) {
        self.shared.with(|inner, events| {
            if inner.queue.jump(index).is_some() {
                inner.failures = 0;
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn set_shuffle(&self, on: bool) {
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.set_shuffle(on, rng);
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        self.shared.with(|inner, events| {
            inner.queue.set_repeat(repeat);
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn play_next(&self, track: Track) {
        self.shared.with(|inner, events| {
            let was_empty = inner.queue.is_empty();
            inner.queue.play_next(track);
            events.push(inner.queue_event());
            if was_empty {
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn play_last(&self, track: Track) {
        self.shared.with(|inner, events| {
            let was_empty = inner.queue.is_empty();
            inner.queue.play_last(track);
            events.push(inner.queue_event());
            if was_empty {
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn remove_from_queue(&self, index: usize) {
        self.shared.with(|inner, events| {
            let was_last = inner.queue.at_last();
            let was_current = inner.queue.remove(index);
            if inner.queue.is_empty() {
                inner.radio = None;
            }
            if was_current && was_last && !inner.queue.is_empty() {
                // Removing the last track of the queue while it plays: the
                // queue is over, unless it repeats. Never replay the previous one.
                if inner.queue.repeat() == Repeat::All {
                    inner.queue.rewind();
                } else {
                    events.push(inner.queue_event());
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    self.shared.remote().stop();
                    events.push(UiEvent::State(inner.state()));
                    return;
                }
            }
            events.push(inner.queue_event());
            if was_current {
                if inner.queue.is_empty() {
                    inner.status = Status::Idle;
                    self.shared.remote().stop();
                    events.push(UiEvent::State(inner.state()));
                } else {
                    self.shared.start_current(inner, events);
                }
            } else {
                events.push(UiEvent::State(inner.state()));
                self.shared.maybe_refill(inner);
            }
        });
    }

    pub fn stop(&self) {
        self.shared.with(|inner, events| {
            self.shared.cancel_recovery(inner, events);
            if let Some(radio) = inner.radio.as_mut() {
                radio.waiting = false;
            }
            inner.status = Status::Idle;
            inner.position = 0.0;
            self.shared.remote().stop();
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn client(&self) -> &VkClient {
        &self.shared.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_told_apart_by_what_helps() {
        for error in [Error::Timeout, Error::Offline, Error::Transport("x".into()), Error::Http(503), Error::Http(429), Error::Vk { code: 9, message: String::new() }] {
            assert_eq!(classify(&error), Class::Connectivity, "{error:?}");
        }
        for error in [Error::Http(403), Error::Http(404), Error::Http(410), Error::Http(401), Error::Stream("decode".into())] {
            assert_eq!(classify(&error), Class::Content, "{error:?}");
        }
        for error in [Error::Unauthorized, Error::Format("x".into()), Error::Vk { code: 15, message: String::new() }, Error::InvalidRequest, Error::Cancelled] {
            assert_eq!(classify(&error), Class::Fatal, "{error:?}");
        }
    }

    #[test]
    fn log_names_never_carry_the_error_text() {
        // `Transport` and `Stream` texts can hold addresses; the log gets a fixed name.
        assert_eq!(what(&Error::Transport("https://cdn/x?token=abc".into())), "connection lost");
        assert_eq!(what(&Error::Stream("https://cdn/x?token=abc".into())), "stream error");
        assert_eq!(what(&Error::Http(403)), "403");
    }

    #[test]
    fn the_schedule_is_bounded_jittered_and_backs_off() {
        let policy = RecoveryPolicy::default();
        let secs = |connectivity, attempt, unit| policy.delay(connectivity, attempt, unit).as_secs_f64();
        // A dead address is re-resolved at once the first time, then backs off.
        assert_eq!(secs(false, 1, 0.5), 0.0);
        assert!((secs(false, 2, 0.5) - 1.2).abs() < 1e-9);
        assert!((secs(false, 3, 0.5) - 2.4).abs() < 1e-9);
        // A bad connection gets a moment, then 1.2, 2.4, 4.8 … capped at 20 s.
        assert!((0.3..0.5).contains(&secs(true, 1, 0.5)));
        let ladder: Vec<f64> = (2..=6).map(|n| secs(true, n, 0.5)).collect();
        assert!(ladder.windows(2).all(|w| w[1] > w[0]), "{ladder:?}");
        assert!(ladder.iter().all(|d| *d <= 20.0));
        // Jitter stays within ±15 %, and nothing ever exceeds the caps.
        for attempt in 1..=40 {
            for unit in [0.0, 0.3, 1.0, f64::NAN] {
                assert!(secs(true, attempt, unit) <= 20.0 && secs(false, attempt, unit) <= 6.0);
            }
        }
        let (low, high) = (secs(true, 4, 0.0), secs(true, 4, 1.0));
        assert!(high / low < 1.4 && low < 4.8 && high > 4.8);
        // The total wait before giving up on a connection problem is well under a minute.
        let total: f64 = (1..=policy.connectivity_attempts).map(|n| secs(true, n, 1.0)).sum();
        assert!(total < 50.0, "{total}");
        // After the network returns: a short random pause.
        assert!(policy.network_return_delay(0.0).as_secs_f64() >= 0.1 && policy.network_return_delay(1.0).as_secs_f64() <= 0.5);
        // Tests shrink every wait.
        let fast = RecoveryPolicy { time_scale: 0.01, ..policy };
        assert!(fast.delay(true, 6, 1.0).as_secs_f64() < 0.25);
    }
}
