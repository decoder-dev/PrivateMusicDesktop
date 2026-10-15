//! The brain of the app: queue + player + VK, behind a small thread-safe API.
//!
//! The UI layer only calls methods here and listens to [`UiEvent`]s. Policy
//! lives here, where it can be tested without a window:
//!
//! * a track that ends starts the next one (repeat/shuffle aware);
//! * a stream that fails is retried once with a fresh URL from
//!   `audio.getById` (VK stream URLs expire), then skipped, at most three
//!   times in a row so a dead network does not burn through the queue;
//! * network and API calls never run on the audio thread or under the state
//!   lock.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Serialize;

use crate::audio::{PlayerEvent, PlayerHandle, Remote, Sink};
use crate::queue::{Queue, Repeat};
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    State(State),
    Queue { tracks: Vec<Track>, current: Option<usize> },
    Error { kind: String, message: String },
    /// The VK token no longer works: show the sign-in screen.
    SessionExpired,
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
}

struct Shared {
    inner: Mutex<Inner>,
    client: VkClient,
    remote: OnceLock<Remote>,
    emit: Box<dyn Fn(UiEvent) + Send + Sync>,
    fetch_template: FetchConfig,
}

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
        config
    }

    /// Starts the queue's current track, or stops when there is none.
    fn start_current(self: &Arc<Self>, inner: &mut Inner, events: &mut Vec<UiEvent>) {
        let Some(track) = inner.queue.current().cloned() else {
            inner.status = Status::Idle;
            self.remote().stop();
            events.push(UiEvent::State(inner.state()));
            return;
        };
        inner.status = Status::Loading;
        inner.position = 0.0;
        inner.duration = f64::from(track.duration);
        events.push(UiEvent::State(inner.state()));
        match track.stream_url.clone() {
            Some(url) => {
                let config = self.fetch_config(inner);
                self.remote().play(&track.id(), &url, f64::from(track.duration), 0.0, config);
            }
            None => self.refresh_then_play(inner, track),
        }
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
            let result = shared.client.refreshed_track(&session, &track);
            match result {
                Ok(fresh) => shared.with(|inner, events| {
                    // The user may have moved on while VK was answering.
                    if inner.queue.current().map(Track::id) != Some(track.id()) {
                        return;
                    }
                    inner.queue.replace_track(&fresh);
                    let config = shared.fetch_config(inner);
                    if let Some(url) = &fresh.stream_url {
                        shared.remote().play(&fresh.id(), url, f64::from(fresh.duration), 0.0, config);
                    }
                    events.push(UiEvent::State(inner.state()));
                }),
                Err(error) => shared.fail_track(&track, error),
            }
        });
    }

    /// A track cannot be played: report it and move on, within limits.
    fn fail_track(self: &Arc<Self>, track: &Track, error: Error) {
        self.with(|inner, events| {
            if inner.queue.current().map(Track::id) != Some(track.id()) {
                return;
            }
            if error == Error::Unauthorized {
                events.push(UiEvent::SessionExpired);
            }
            events.push(UiEvent::Error { kind: error.kind().to_string(), message: error.to_string() });
            let skippable = matches!(error, Error::Stream(_) | Error::Http(_) | Error::Format(_) | Error::Vk { .. });
            inner.failures += 1;
            if skippable && inner.failures < MAX_CONSECUTIVE_FAILURES && inner.queue.advance(false).is_some() {
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.start_current(inner, events);
            } else {
                inner.status = Status::Idle;
                self.remote().stop();
                events.push(UiEvent::State(inner.state()));
            }
        });
    }

    fn on_player_event(self: &Arc<Self>, event: PlayerEvent) {
        match event {
            PlayerEvent::Position { track_id, seconds, duration } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.position = seconds;
                    if duration > 0.0 {
                        inner.duration = duration;
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Loading { .. } => {}
            PlayerEvent::Playing { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.status = Status::Playing;
                    inner.failures = 0;
                    inner.refreshed.remove(&track_id);
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
                } else {
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Failed { track_id, error } => {
                let current = {
                    let inner = self.lock();
                    inner.queue.current().cloned().filter(|t| t.id() == track_id)
                };
                let Some(track) = current else { return };
                // First failure of this track: VK URLs expire, try a fresh one.
                let retry = {
                    let mut inner = self.lock();
                    let can_refresh = inner.session.is_some()
                        && !inner.refreshed.contains(&track_id)
                        && matches!(error, Error::Stream(_) | Error::Http(_));
                    if can_refresh {
                        self.refresh_then_play(&mut inner, track.clone());
                    }
                    can_refresh
                };
                if !retry {
                    self.fail_track(&track, error);
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
            }),
            client,
            remote: OnceLock::new(),
            emit: Box::new(emit),
            fetch_template,
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
                inner.queue.clear();
                inner.status = Status::Idle;
                self.shared.remote().stop();
                events.push(inner.queue_event());
                events.push(UiEvent::State(inner.state()));
            }
        });
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
            inner.failures = 0;
            inner.refreshed.clear();
            events.push(inner.queue_event());
            self.shared.start_current(inner, events);
        });
    }

    pub fn pause(&self) {
        self.shared.remote().pause();
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
            events.push(UiEvent::State(inner.state()));
        });
        self.shared.remote().seek(seconds);
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
            let was_current = inner.queue.remove(index);
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
            }
        });
    }

    pub fn stop(&self) {
        self.shared.with(|inner, events| {
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
