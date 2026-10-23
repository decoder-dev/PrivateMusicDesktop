//! Glue between the engine and the plugin, with no Android in it (so it is
//! tested on the desktop host).
//!
//! * [`Bridge`] turns the engine's [`State`]s into [`Update`]s, reusing
//!   [`MediaSync`] exactly like `src-tauri/src/media.rs` does: a new title when
//!   the track changes, a playback message only when the state changes or the
//!   position jumps. The system extrapolates between messages.
//! * [`dispatch`] applies a [`MediaAction`] from the system to the engine,
//!   through `pm_engine::media_sync::apply`, plus volume ducking.

use std::sync::Mutex;

use pm_engine::controller::{Controller, State, Status, UiEvent};
use pm_engine::media_sync::{self, MediaSync, Metadata, Playback};

use crate::{MediaAction, MediaSink, PlaybackInfo, PlaybackKind, Result, TrackMetadata, Update};

/// Turns engine states into plugin updates.
pub struct Bridge<S: MediaSink> {
    sink: S,
    sync: MediaSync,
    buffering: bool,
}

impl<S: MediaSink> Bridge<S> {
    pub fn new(sink: S) -> Self {
        Self { sink, sync: MediaSync::default(), buffering: false }
    }

    /// Feed every [`UiEvent`]; only `State` matters. `now` is any monotonic
    /// clock in seconds. Sink errors are returned but leave the bridge
    /// consistent: the host should log them and carry on.
    pub fn on_event(&mut self, event: &UiEvent, now: f64) -> Result<()> {
        match event {
            UiEvent::State(state) => self.on_state(state, now),
            _ => Ok(()),
        }
    }

    pub fn on_state(&mut self, state: &State, now: f64) -> Result<()> {
        let update = self.build(state, now);
        if update.is_empty() {
            return Ok(());
        }
        self.sink.update(&update)
    }

    fn build(&mut self, state: &State, now: f64) -> Update {
        let updates = self.sync.update(state, now);
        let has_track = state.track.is_some();
        // The OS counts loading as playing; Android shows a spinner for it.
        let buffering = has_track && matches!(state.status, Status::Loading | Status::Buffering);
        let metadata = updates.metadata.as_ref();
        let duration = if state.duration.is_finite() && state.duration > 0.0 {
            state.duration
        } else {
            metadata.and_then(|m| m.as_ref()).map_or(0.0, |m| m.duration)
        };
        let mut playback = updates.playback.map(|p| playback_info(p, duration, buffering));
        if playback.is_none() && has_track && buffering != self.buffering && state.status != Status::Paused {
            // Buffering started or ended without the OS-visible class changing.
            playback = Some(PlaybackInfo {
                state: if buffering { PlaybackKind::Buffering } else { PlaybackKind::Playing },
                position_secs: finite(state.position),
                duration_secs: duration,
                rate: 1.0,
            });
        }
        self.buffering = buffering;
        Update {
            clear_metadata: matches!(metadata, Some(None)),
            metadata: metadata.and_then(|m| m.as_ref()).map(track_metadata),
            playback,
        }
    }
}

fn finite(seconds: f64) -> f64 {
    if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    }
}

fn playback_info(playback: Playback, duration: f64, buffering: bool) -> PlaybackInfo {
    let (state, position) = match playback {
        Playback::Stopped => (PlaybackKind::Stopped, 0.0),
        Playback::Paused(p) => (PlaybackKind::Paused, p),
        Playback::Playing(p) if buffering => (PlaybackKind::Buffering, p),
        Playback::Playing(p) => (PlaybackKind::Playing, p),
    };
    PlaybackInfo { state, position_secs: finite(position), duration_secs: duration, rate: 1.0 }
}

fn track_metadata(m: &Metadata) -> TrackMetadata {
    TrackMetadata {
        title: m.title.clone(),
        artist: m.artist.clone(),
        album: m.album.clone(),
        // Only https artwork leaves Rust; Kotlin checks again.
        artwork_url: m.cover_url.clone().filter(|u| u.starts_with("https://")),
        duration_secs: if m.duration.is_finite() { m.duration.max(0.0) } else { 0.0 },
    }
}

/// The engine action for a system request, or `None` for requests that are not
/// the engine's business (`Duck`).
pub fn to_engine_action(action: MediaAction) -> Option<media_sync::MediaAction> {
    use media_sync::MediaAction as E;
    Some(match action {
        MediaAction::Play => E::Play,
        MediaAction::Pause => E::Pause,
        MediaAction::Toggle => E::Toggle,
        MediaAction::Next => E::Next,
        MediaAction::Previous => E::Previous,
        MediaAction::Stop => E::Stop,
        MediaAction::SeekTo(s) => E::SeekTo(s),
        MediaAction::SeekBy(d) => E::SeekBy(d),
        MediaAction::Duck(_) => return None,
    })
}

/// What the ducker needs from the engine.
pub trait Volume {
    fn volume(&self) -> f32;
    fn set_volume(&self, volume: f32);
}

impl Volume for Controller {
    fn volume(&self) -> f32 {
        self.state().volume
    }
    fn set_volume(&self, volume: f32) {
        Controller::set_volume(self, volume);
    }
}

/// Lowers the volume while another app talks over us and restores it after.
/// If the user moved the volume meanwhile, their choice wins.
#[derive(Debug)]
pub struct Ducker {
    gain: f32,
    /// `(volume before ducking, volume we set)` while ducked.
    ducked: Mutex<Option<(f32, f32)>>,
}

impl Ducker {
    /// `gain` is the fraction of the volume kept while ducked (0.2 is typical).
    pub fn new(gain: f32) -> Self {
        Self { gain: gain.clamp(0.0, 1.0), ducked: Mutex::new(None) }
    }

    /// Idempotent: repeated `true`/`false` are ignored.
    pub fn set(&self, volume: &impl Volume, on: bool) {
        let mut ducked = self.ducked.lock().unwrap_or_else(|p| p.into_inner());
        match (on, *ducked) {
            (true, None) => {
                let before = volume.volume();
                let lowered = before * self.gain;
                volume.set_volume(lowered);
                *ducked = Some((before, lowered));
            }
            (false, Some((before, lowered))) => {
                if (volume.volume() - lowered).abs() < 1e-3 {
                    volume.set_volume(before);
                }
                *ducked = None;
            }
            _ => {}
        }
    }
}

impl Default for Ducker {
    fn default() -> Self {
        Self::new(0.25)
    }
}

/// Applies a request from the system: `Duck` through the ducker, everything
/// else through `pm_engine::media_sync::apply`.
pub fn dispatch(controller: &Controller, ducker: &Ducker, action: MediaAction) {
    match (action, to_engine_action(action)) {
        (MediaAction::Duck(on), _) => ducker.set(controller, on),
        (_, Some(engine)) => media_sync::apply(controller, engine),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use pm_engine::queue::Repeat;
    use pm_engine::vk::Track;

    use super::*;

    #[derive(Default)]
    struct Fake {
        updates: RefCell<Vec<Update>>,
        fail: bool,
    }

    impl MediaSink for &Fake {
        fn update(&self, update: &Update) -> Result<()> {
            self.updates.borrow_mut().push(update.clone());
            if self.fail {
                Err(crate::Error::WorkerGone)
            } else {
                Ok(())
            }
        }
    }

    fn track(n: i64, art: Option<&str>) -> Track {
        Track {
            track_id: n, owner_id: 1, title: format!("T{n}"), artist: "A".into(), album_title: Some("Al".into()),
            duration: 200, stream_url: None, artwork_url: art.map(String::from),
            access_key: None, lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None,
        }
    }

    fn state(status: Status, track: Option<Track>, position: f64) -> State {
        State {
            status, track, position, duration: 200.0, volume: 1.0, shuffle: false, repeat: Repeat::Off,
            queue_index: Some(0), queue_len: 1, radio: None, radio_context: None,
        }
    }

    fn playing(n: i64, p: f64) -> State {
        state(Status::Playing, Some(track(n, Some("https://c.example/x.jpg"))), p)
    }

    #[test]
    fn a_new_track_sends_metadata_and_playback_once() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        bridge.on_state(&playing(1, 0.0), 0.0).unwrap();
        bridge.on_state(&playing(1, 0.1), 0.1).unwrap();
        let sent = fake.updates.borrow();
        assert_eq!(sent.len(), 1);
        let meta = sent[0].metadata.as_ref().unwrap();
        assert_eq!((meta.title.as_str(), meta.artist.as_str(), meta.album.as_deref()), ("T1", "A", Some("Al")));
        assert_eq!(meta.artwork_url.as_deref(), Some("https://c.example/x.jpg"));
        assert_eq!(meta.duration_secs, 200.0);
        let playback = sent[0].playback.unwrap();
        assert_eq!(playback.state, PlaybackKind::Playing);
        assert_eq!((playback.position_secs, playback.duration_secs, playback.rate), (0.0, 200.0, 1.0));
    }

    #[test]
    fn steady_playback_at_four_hz_sends_nothing_after_the_first_update() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        bridge.on_state(&playing(1, 0.0), 0.0).unwrap();
        for tick in 1..=80 {
            let t = f64::from(tick) * 0.25;
            bridge.on_state(&playing(1, t), t).unwrap();
        }
        assert_eq!(fake.updates.borrow().len(), 1);
    }

    #[test]
    fn pause_seek_and_resume_are_sent_without_metadata() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        bridge.on_state(&playing(1, 10.0), 0.0).unwrap();
        bridge.on_state(&state(Status::Paused, Some(track(1, None)), 12.0), 2.0).unwrap();
        bridge.on_state(&playing(1, 12.0), 3.0).unwrap();
        bridge.on_state(&playing(1, 100.0), 4.0).unwrap();
        let sent = fake.updates.borrow();
        let kinds: Vec<_> = sent.iter().map(|u| u.playback.map(|p| (p.state, p.position_secs))).collect();
        assert_eq!(
            kinds,
            vec![
                Some((PlaybackKind::Playing, 10.0)),
                Some((PlaybackKind::Paused, 12.0)),
                Some((PlaybackKind::Playing, 12.0)),
                Some((PlaybackKind::Playing, 100.0)),
            ]
        );
        assert!(sent[1..].iter().all(|u| u.metadata.is_none() && !u.clear_metadata));
    }

    #[test]
    fn loading_and_buffering_are_reported_as_buffering_then_playing() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        let t = || Some(track(1, None));
        bridge.on_state(&state(Status::Loading, t(), 0.0), 0.0).unwrap();
        bridge.on_state(&state(Status::Playing, t(), 0.0), 0.5).unwrap();
        bridge.on_state(&state(Status::Buffering, t(), 5.0), 5.0).unwrap();
        bridge.on_state(&state(Status::Buffering, t(), 5.0), 5.2).unwrap();
        bridge.on_state(&state(Status::Playing, t(), 5.0), 6.0).unwrap();
        let sent = fake.updates.borrow();
        let kinds: Vec<_> = sent.iter().filter_map(|u| u.playback).map(|p| p.state).collect();
        assert_eq!(
            kinds,
            vec![PlaybackKind::Buffering, PlaybackKind::Playing, PlaybackKind::Buffering, PlaybackKind::Playing]
        );
        assert_eq!(sent[2].playback.unwrap().position_secs, 5.0);
    }

    #[test]
    fn buffering_into_pause_reports_paused_only() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        let t = || Some(track(1, None));
        bridge.on_state(&state(Status::Buffering, t(), 3.0), 0.0).unwrap();
        bridge.on_state(&state(Status::Paused, t(), 3.0), 1.0).unwrap();
        let sent = fake.updates.borrow();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].playback.unwrap().state, PlaybackKind::Paused);
    }

    #[test]
    fn stopping_clears_the_metadata_and_stays_quiet() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        bridge.on_state(&playing(1, 0.0), 0.0).unwrap();
        bridge.on_state(&state(Status::Idle, None, 0.0), 1.0).unwrap();
        bridge.on_state(&state(Status::Idle, None, 0.0), 2.0).unwrap();
        let sent = fake.updates.borrow();
        assert_eq!(sent.len(), 2);
        assert!(sent[1].clear_metadata && sent[1].metadata.is_none());
        assert_eq!(sent[1].playback.unwrap().state, PlaybackKind::Stopped);
    }

    #[test]
    fn next_track_replaces_the_metadata() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        bridge.on_state(&playing(1, 0.0), 0.0).unwrap();
        bridge.on_state(&playing(2, 0.0), 200.0).unwrap();
        let sent = fake.updates.borrow();
        assert_eq!(sent[1].metadata.as_ref().unwrap().title, "T2");
        assert_eq!(sent[1].playback.unwrap().state, PlaybackKind::Playing);
    }

    #[test]
    fn plain_http_artwork_never_leaves_rust() {
        let fake = Fake::default();
        let mut bridge = Bridge::new(&fake);
        bridge.on_state(&state(Status::Playing, Some(track(1, Some("http://c.example/x.jpg"))), 0.0), 0.0).unwrap();
        assert_eq!(fake.updates.borrow()[0].metadata.as_ref().unwrap().artwork_url, None);
    }

    #[test]
    fn non_state_events_are_ignored_and_sink_errors_are_returned() {
        let fake = Fake { fail: true, ..Fake::default() };
        let mut bridge = Bridge::new(&fake);
        assert!(bridge.on_event(&UiEvent::SessionExpired, 0.0).is_ok());
        assert!(fake.updates.borrow().is_empty());
        assert!(bridge.on_event(&UiEvent::State(playing(1, 0.0)), 0.0).is_err());
        // The failed update was not lost silently, but the bridge does not resend
        // it either: the next change carries on from the new state.
        assert!(bridge.on_event(&UiEvent::State(playing(1, 0.5)), 0.5).is_ok());
    }

    #[test]
    fn actions_map_onto_the_engine_actions() {
        use media_sync::MediaAction as E;
        assert_eq!(to_engine_action(MediaAction::Play), Some(E::Play));
        assert_eq!(to_engine_action(MediaAction::Pause), Some(E::Pause));
        assert_eq!(to_engine_action(MediaAction::Toggle), Some(E::Toggle));
        assert_eq!(to_engine_action(MediaAction::Next), Some(E::Next));
        assert_eq!(to_engine_action(MediaAction::Previous), Some(E::Previous));
        assert_eq!(to_engine_action(MediaAction::Stop), Some(E::Stop));
        assert_eq!(to_engine_action(MediaAction::SeekTo(3.0)), Some(E::SeekTo(3.0)));
        assert_eq!(to_engine_action(MediaAction::SeekBy(-5.0)), Some(E::SeekBy(-5.0)));
        assert_eq!(to_engine_action(MediaAction::Duck(true)), None);
    }

    struct FakeVolume(RefCell<f32>);
    impl Volume for FakeVolume {
        fn volume(&self) -> f32 {
            *self.0.borrow()
        }
        fn set_volume(&self, volume: f32) {
            *self.0.borrow_mut() = volume;
        }
    }

    #[test]
    fn ducking_lowers_and_restores_the_volume_once() {
        let volume = FakeVolume(RefCell::new(0.8));
        let ducker = Ducker::new(0.25);
        ducker.set(&volume, true);
        ducker.set(&volume, true);
        assert!((volume.volume() - 0.2).abs() < 1e-6, "a second duck must not lower it again");
        ducker.set(&volume, false);
        assert!((volume.volume() - 0.8).abs() < 1e-6);
        ducker.set(&volume, false);
        assert!((volume.volume() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_volume_change_made_while_ducked_is_kept() {
        let volume = FakeVolume(RefCell::new(1.0));
        let ducker = Ducker::default();
        ducker.set(&volume, true);
        volume.set_volume(0.6); // the user dragged the slider
        ducker.set(&volume, false);
        assert!((volume.volume() - 0.6).abs() < 1e-6);
    }
}
