//! What the operating system's media controls (media keys, lock screen,
//! "Now Playing" widgets) should be told, and how to react to their buttons.
//! Pure logic: the OS bindings live in the app shell.
//!
//! The OS needs few messages: a new title when the track changes, and a
//! playback state when it changes or the position jumps. Sending the position
//! four times a second would wake the whole desktop for nothing; the OS
//! extrapolates between messages.

use crate::controller::{Controller, State, Status};

/// OS media/notification APIs may block. Never call them on the audio thread:
/// it must keep processing Pause/Stop even when the desktop stops answering.
/// One pending state is enough; slow consumers must not accumulate positions.
pub struct StatePublisher {
    latest: std::sync::Arc<std::sync::Mutex<Option<State>>>,
    wake: std::sync::mpsc::SyncSender<()>,
}

impl StatePublisher {
    pub fn spawn(mut update: impl FnMut(State) + Send + 'static) -> std::io::Result<Self> {
        let latest = std::sync::Arc::new(std::sync::Mutex::new(None));
        let pending = latest.clone();
        let (wake, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new().name("pm-system-media".into()).spawn(move || {
            while receiver.recv().is_ok() {
                let state = pending.lock().unwrap_or_else(|p| p.into_inner()).take();
                if let Some(state) = state {
                    update(state);
                }
            }
        })?;
        Ok(Self { latest, wake })
    }

    pub fn publish(&self, state: State) {
        *self.latest.lock().unwrap_or_else(|p| p.into_inner()) = Some(state);
        // Full means a wake is already queued. Disconnected means the OS
        // integration stopped; neither case may stop playback.
        let _ = self.wake.try_send(());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub cover_url: Option<String>,
    pub duration: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Playback {
    Stopped,
    Paused(f64),
    Playing(f64),
}

#[derive(Debug, Default, PartialEq)]
pub struct Updates {
    pub metadata: Option<Option<Metadata>>,
    pub playback: Option<Playback>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    SeekTo(f64),
    SeekBy(f64),
}

#[derive(Default)]
pub struct MediaSync {
    track: Option<String>,
    sent: Option<Class>,
    /// Position and time of the last playback message, for extrapolation.
    last_position: f64,
    last_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Stopped,
    Paused,
    Playing,
}

fn class(status: Status, has_track: bool) -> Class {
    match status {
        _ if !has_track => Class::Stopped,
        Status::Idle => Class::Stopped,
        Status::Paused => Class::Paused,
        // Loading and buffering are "playing" to the OS: the user wants sound.
        Status::Loading | Status::Playing | Status::Buffering => Class::Playing,
    }
}

impl MediaSync {
    /// `now` is any monotonic clock in seconds.
    pub fn update(&mut self, state: &State, now: f64) -> Updates {
        let mut updates = Updates::default();
        let id = state.track.as_ref().map(|t| t.id());
        if id != self.track {
            self.track = id;
            updates.metadata = Some(state.track.as_ref().map(|t| Metadata {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album_title.clone(),
                cover_url: t.artwork_url.clone(),
                duration: f64::from(t.duration),
            }));
            // A new track always restates the playback.
            self.sent = None;
        }
        let current = class(state.status, state.track.is_some());
        let expected = match self.sent {
            Some(Class::Playing) => self.last_position + (now - self.last_at),
            _ => self.last_position,
        };
        let jumped = (state.position - expected).abs() > 1.5;
        if self.sent != Some(current) || (current != Class::Stopped && jumped) {
            self.sent = Some(current);
            self.last_position = state.position;
            self.last_at = now;
            updates.playback = Some(match current {
                Class::Stopped => Playback::Stopped,
                Class::Paused => Playback::Paused(state.position),
                Class::Playing => Playback::Playing(state.position),
            });
        }
        updates
    }
}

/// Carries out a button press from the OS.
pub fn apply(controller: &Controller, action: MediaAction) {
    let state = controller.state();
    match action {
        MediaAction::Play => match state.status {
            Status::Paused => controller.resume(),
            Status::Idle => controller.toggle(),
            _ => {}
        },
        MediaAction::Pause => {
            if matches!(state.status, Status::Playing | Status::Buffering) {
                controller.pause();
            }
        }
        MediaAction::Toggle => controller.toggle(),
        MediaAction::Next => controller.next(),
        MediaAction::Previous => controller.previous(),
        MediaAction::Stop => controller.stop(),
        MediaAction::SeekTo(seconds) => controller.seek(seconds),
        MediaAction::SeekBy(delta) => {
            if state.track.is_some() && delta.is_finite() {
                let target = (state.position + delta).clamp(0.0, state.duration.max(0.0));
                controller.seek(target);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::Repeat;
    use crate::vk::Track;

    fn track(n: i64) -> Track {
        Track {
            track_id: n, owner_id: 1, title: format!("T{n}"), artist: "A".into(), album_title: Some("Al".into()),
            duration: 200, stream_url: None, artwork_url: Some("https://c.example/x.jpg".into()),
            access_key: None, lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None,
        }
    }

    fn state(status: Status, track: Option<Track>, position: f64) -> State {
        State {
            status, track, position, duration: 200.0, volume: 1.0, shuffle: false, repeat: Repeat::Off,
            queue_index: Some(0), queue_len: 1, radio: None,
        }
    }

    #[test]
    fn blocked_system_media_cannot_block_publishers_or_accumulate_old_states() {
        use std::sync::mpsc::sync_channel;
        use std::time::Duration;
        let (entered, entry) = sync_channel(1);
        let (release, gate) = sync_channel(1);
        let (sent, received) = sync_channel(2);
        let mut first = true;
        let publisher = StatePublisher::spawn(move |state| {
            if first {
                first = false;
                entered.send(()).unwrap();
                gate.recv().unwrap();
            }
            sent.send(state.position).unwrap();
        }).unwrap();
        publisher.publish(state(Status::Playing, Some(track(1)), 0.0));
        entry.recv_timeout(Duration::from_secs(2)).unwrap();
        // The OS handler is blocked, as a stuck SMTC call would be. Publishing
        // Pause and many subsequent positions must still return immediately.
        let (finished, completion) = sync_channel(1);
        std::thread::spawn(move || {
            for n in 1..=1000 {
                publisher.publish(state(Status::Paused, Some(track(1)), f64::from(n)));
            }
            finished.send(()).unwrap();
        });
        let published = completion.recv_timeout(Duration::from_secs(2));
        release.send(()).unwrap();
        published.expect("a blocked OS handler must not block the audio publisher");
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), 0.0);
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), 1000.0);
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn a_new_track_sends_metadata_and_playback_once() {
        let mut sync = MediaSync::default();
        let u = sync.update(&state(Status::Loading, Some(track(1)), 0.0), 0.0);
        assert_eq!(u.metadata.as_ref().unwrap().as_ref().unwrap().title, "T1");
        assert_eq!(u.playback, Some(Playback::Playing(0.0)));
        // The same track, loading → playing: nothing new to say.
        let u = sync.update(&state(Status::Playing, Some(track(1)), 0.0), 0.1);
        assert_eq!(u, Updates::default());
    }

    #[test]
    fn steady_playback_sends_nothing_even_at_four_hz() {
        let mut sync = MediaSync::default();
        sync.update(&state(Status::Playing, Some(track(1)), 0.0), 0.0);
        let mut sent = 0;
        for tick in 1..=80 {
            let t = f64::from(tick) * 0.25;
            if sync.update(&state(Status::Playing, Some(track(1)), t), t).playback.is_some() {
                sent += 1;
            }
        }
        assert_eq!(sent, 0, "20 seconds of normal playback must not message the OS");
    }

    #[test]
    fn pause_resume_and_seek_are_sent() {
        let mut sync = MediaSync::default();
        sync.update(&state(Status::Playing, Some(track(1)), 10.0), 0.0);
        let u = sync.update(&state(Status::Paused, Some(track(1)), 12.0), 2.0);
        assert_eq!(u.playback, Some(Playback::Paused(12.0)));
        // Paused time does not advance the extrapolation.
        assert_eq!(sync.update(&state(Status::Paused, Some(track(1)), 12.0), 30.0), Updates::default());
        let u = sync.update(&state(Status::Playing, Some(track(1)), 12.0), 31.0);
        assert_eq!(u.playback, Some(Playback::Playing(12.0)));
        // A jump of a minute is a seek.
        let u = sync.update(&state(Status::Playing, Some(track(1)), 100.0), 32.0);
        assert_eq!(u.playback, Some(Playback::Playing(100.0)));
    }

    #[test]
    fn buffering_still_counts_as_playing() {
        let mut sync = MediaSync::default();
        sync.update(&state(Status::Playing, Some(track(1)), 5.0), 0.0);
        assert_eq!(sync.update(&state(Status::Buffering, Some(track(1)), 5.0), 0.5), Updates::default());
    }

    #[test]
    fn stopping_clears_the_metadata() {
        let mut sync = MediaSync::default();
        sync.update(&state(Status::Playing, Some(track(1)), 0.0), 0.0);
        let u = sync.update(&state(Status::Idle, None, 0.0), 1.0);
        assert_eq!(u.metadata, Some(None));
        assert_eq!(u.playback, Some(Playback::Stopped));
        // Stopped stays quiet however the position wiggles.
        assert_eq!(sync.update(&state(Status::Idle, None, 0.0), 2.0), Updates::default());
    }

    #[test]
    fn next_track_replaces_the_metadata() {
        let mut sync = MediaSync::default();
        sync.update(&state(Status::Playing, Some(track(1)), 0.0), 0.0);
        let u = sync.update(&state(Status::Playing, Some(track(2)), 0.0), 200.0);
        assert_eq!(u.metadata.unwrap().unwrap().title, "T2");
        assert_eq!(u.playback, Some(Playback::Playing(0.0)));
    }
}
