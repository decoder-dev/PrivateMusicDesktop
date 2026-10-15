//! Bugs found in review, each pinned by the scenario that showed it.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{PlayerEvent, PlayerHandle, SimulatedSink, Sink};
use pm_engine::controller::{Status, UiEvent};
use pm_engine::queue::Repeat;
use pm_engine::store::{MemoryStore, SecretStore};
use pm_engine::vk::{Session, Track, VkClient};
use pm_engine::Error;

fn session(token: &str) -> Session {
    Session {
        access_token: token.into(), user_id: Some(7), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: "remixsid=c".into(), web_user_agent: "w".into(),
    }
}

fn track(n: i64, url: Option<String>) -> Track {
    Track {
        track_id: n, owner_id: 7, title: format!("T{n}"), artist: "A".into(), album_title: None, duration: 12,
        stream_url: url, artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None,
    }
}

struct Shared(Arc<MemoryStore>);
impl SecretStore for Shared {
    fn is_durable(&self) -> bool { true }
    fn load(&self) -> pm_engine::Result<Option<Session>> { self.0.load() }
    fn save(&self, s: &Session) -> pm_engine::Result<()> { self.0.save(s) }
    fn clear(&self) -> pm_engine::Result<()> { self.0.clear() }
}

fn app(vk: &str, token_url: String, store: Arc<MemoryStore>, speed: f64) -> (App, Arc<Mutex<Vec<UiEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let app = App::new(
        VkClient::with_base(vk).unwrap(),
        move || Ok(Box::new(SimulatedSink::new(44_100, 2, speed)) as Box<dyn Sink>),
        Box::new(Shared(store)),
        move |e| log.lock().unwrap().push(e),
        AppOptions { fetch: lab_config(), token_url, ..AppOptions::default() },
    );
    (app, events)
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn hls() -> (tempfile::TempDir, Server) {
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]));
    let server = Server::start(dir.path().to_path_buf());
    (dir, server)
}

/// An expired token while a restored queue asks VK for stream addresses is
/// renewed from the cookie; it is not a sign-out.
#[test]
fn an_expired_token_during_playback_is_renewed_not_signed_out() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = hls();
    let stream = format!("{}/index.m3u8", media.base);
    let vk = FakeApi::start(move |_, body, _| {
        if body.contains("access_token=OLD") {
            return (200, r#"{"error":{"error_code":5,"error_msg":"expired"}}"#.into());
        }
        (200, format!(r#"{{"response":[{{"id":1,"owner_id":7,"artist":"A","title":"T1","duration":12,"url":"{stream}"}}]}}"#))
    });
    let token = FakeApi::start(|_, _, _| (200, r#"{"type":"okay","data":{"access_token":"NEWNEWNEWNEWNEWNEW","user_id":7,"expires":0}}"#.into()));
    let store = Arc::new(MemoryStore::default());
    let (app, events) = app(&vk.base, format!("{}/?act=web_token", token.base), store.clone(), 20.0);
    app.sign_in(session("OLD"));
    // Saved queues carry no stream address, so this goes through audio.getById.
    app.controller().load_queue(vec![track(1, None)], 0, 0.0);
    app.controller().toggle();
    wait_for("playback", || app.controller().state().status == Status::Playing);
    assert!(app.session_state().signed_in);
    assert_eq!(store.load().unwrap().unwrap().access_token, "NEWNEWNEWNEWNEWNEW");
    assert!(!events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::SessionExpired)));
}

/// Signing out while a token refresh is still waiting for VK must stick.
#[test]
fn signing_out_during_a_refresh_stays_signed_out() {
    let vk = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":5,"error_msg":"expired"}}"#.into()));
    let token = FakeApi::start(|_, _, _| {
        std::thread::sleep(Duration::from_millis(400));
        (200, r#"{"type":"okay","data":{"access_token":"NEWNEWNEWNEWNEWNEW","user_id":7,"expires":0}}"#.into())
    });
    let store = Arc::new(MemoryStore::default());
    let (app, _) = app(&vk.base, format!("{}/?act=web_token", token.base), store.clone(), 20.0);
    let app = Arc::new(app);
    app.sign_in(session("OLD"));
    let worker = { let app = app.clone(); std::thread::spawn(move || app.library(0, 10)) };
    wait_for("the refresh to start", || token.call_count("web_token") == 1);
    app.sign_out();
    assert_eq!(worker.join().unwrap().unwrap_err(), Error::Unauthorized);
    assert!(!app.session_state().signed_in, "the late refresh must not sign the user back in");
    assert!(store.load().unwrap().is_none(), "nor save the session again");
}

/// VK rounds the length up; a seek into the last second may land past the
/// real end of the audio. That is the end of the track, not a failure.
#[test]
fn a_seek_past_the_real_end_ends_the_track() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = hls();
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let mut slot = Some(SimulatedSink::new(44_100, 2, 4.0));
    let player = PlayerHandle::spawn(
        move || slot.take().map(|s| Box::new(s) as Box<dyn Sink>).ok_or(Error::Stream("one".into())),
        move |e| log.lock().unwrap().push(e),
    );
    // The audio is 12.0 s; VK says 13.
    player.play("1_1", &format!("{}/index.m3u8", media.base), 13.0, 0.0, lab_config());
    wait_for("playing", || events.lock().unwrap().iter().any(|e| matches!(e, PlayerEvent::Playing { .. })));
    player.seek(12.6);
    wait_for("the end", || {
        events.lock().unwrap().iter().any(|e| matches!(e, PlayerEvent::Ended { .. } | PlayerEvent::Failed { .. }))
    });
    let failed = events.lock().unwrap().iter().any(|e| matches!(e, PlayerEvent::Failed { .. }));
    assert!(!failed, "{:?}", events.lock().unwrap());
}

/// Removing the playing track when it is the last one must not replay the
/// one before it.
#[test]
fn removing_the_playing_last_track_stops_or_wraps() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = hls();
    let url = format!("{}/index.m3u8", media.base);
    let vk = FakeApi::start(|_, _, _| (200, "{}".into()));
    for (repeat, expect) in [(Repeat::Off, None), (Repeat::All, Some(1))] {
        let (app, _) = app(&vk.base, "http://127.0.0.1:9/x".into(), Arc::new(MemoryStore::default()), 1.0);
        app.sign_in(session("TOKENTOKENTOKENTOKEN"));
        let c = app.controller();
        c.set_repeat(repeat);
        c.play_tracks((1..=3).map(|n| track(n, Some(url.clone()))).collect(), 2);
        wait_for("track 3", || c.state().status == Status::Playing && c.state().track.as_ref().map(|t| t.track_id) == Some(3));
        c.remove_from_queue(2);
        std::thread::sleep(Duration::from_millis(100));
        let state = c.state();
        match expect {
            None => assert_eq!(state.status, Status::Idle, "{state:?}"),
            Some(id) => assert_eq!(state.track.map(|t| t.track_id), Some(id), "repeat-all wraps to the first"),
        }
    }
}

/// A seek while nothing plays is where the next Play starts.
#[test]
fn a_seek_while_idle_is_kept_for_the_next_play() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = hls();
    let url = format!("{}/index.m3u8", media.base);
    let vk = FakeApi::start(|_, _, _| (200, "{}".into()));
    let (app, _) = app(&vk.base, "http://127.0.0.1:9/x".into(), Arc::new(MemoryStore::default()), 1.0);
    app.sign_in(session("TOKENTOKENTOKENTOKEN"));
    let c = app.controller();
    c.load_queue(vec![track(1, Some(url))], 0, 2.0);
    c.seek(8.0);
    c.toggle();
    wait_for("playing", || c.state().status == Status::Playing);
    assert!(c.state().position >= 7.9, "{:?}", c.state());
}
