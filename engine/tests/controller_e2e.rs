//! Controller policy against a fake VK API and a simulated sound card:
//! auto-advance, expired-URL refresh, bounded skipping, queue edits.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::{Controller, State, Status, UiEvent};
use pm_engine::queue::Repeat;
use pm_engine::vk::{Session, Track, VkClient};

fn track(n: i64, url: Option<String>) -> Track {
    Track {
        track_id: n,
        owner_id: 7,
        title: format!("Track {n}"),
        artist: "Artist".into(),
        album_title: None,
        duration: 12,
        stream_url: url,
        artwork_url: None,
        access_key: None,
        lyrics_id: None,
        is_hq: false,
        track_code: None,
    }
}

fn session() -> Session {
    Session {
        access_token: "0123456789abcdef0123456789".into(),
        user_id: Some(7),
        expires_at: None,
        api_user_agent: "test-agent".into(),
        refresh_cookie: String::new(),
        web_user_agent: "web".into(),
    }
}

/// A fake `api.vk.ru`: answers `audio.getById` with `fresh_base/index.m3u8`
/// for every requested id, and records the calls.
struct FakeVk {
    base: String,
    calls: Arc<Mutex<Vec<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FakeVk {
    fn start(stream_base: String, working: bool) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (log, flag) = (calls.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(50)) else { continue };
                let mut body = String::new();
                std::io::Read::read_to_string(request.as_reader(), &mut body).unwrap();
                log.lock().unwrap().push(format!("{} {}", request.url(), body));
                let audios = body
                    .split('&')
                    .find_map(|p| p.strip_prefix("audios="))
                    .unwrap_or("")
                    .replace("%5F", "_")
                    .replace("_", "_");
                let (owner, id) = {
                    let mut parts = audios.split('_');
                    (parts.next().unwrap_or("7").to_string(), parts.next().unwrap_or("1").to_string())
                };
                let json = if working {
                    format!(
                        r#"{{"response":[{{"id":{id},"owner_id":{owner},"artist":"Artist","title":"Fresh","duration":12,"url":"{stream_base}/index.m3u8"}}]}}"#
                    )
                } else {
                    r#"{"error":{"error_code":15,"error_msg":"Access denied"}}"#.to_string()
                };
                let mut response = tiny_http::Response::from_string(json);
                response.add_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), calls, stop, handle: Some(handle) }
    }
}

impl Drop for FakeVk {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Rig {
    controller: Controller,
    states: Arc<Mutex<Vec<State>>>,
    events: Arc<Mutex<Vec<UiEvent>>>,
}

fn rig(vk_base: &str, speed: f64) -> Rig {
    let states = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (s, e) = (states.clone(), events.clone());
    let mut slot = Some(SimulatedSink::new(44_100, 2, speed));
    let controller = Controller::with_fetch(
        VkClient::with_base(vk_base).unwrap(),
        move || Ok(Box::new(slot.take().unwrap_or_else(|| SimulatedSink::new(44_100, 2, 20.0))) as Box<dyn Sink>),
        move |event| {
            if let UiEvent::State(state) = &event {
                s.lock().unwrap().push(state.clone());
            }
            e.lock().unwrap().push(event);
        },
        1.0,
        lab_config(),
    );
    controller.set_session(Some(session()));
    Rig { controller, states, events }
}

impl Rig {
    fn wait(&self, what: &str, f: impl Fn(&State) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            if f(&self.controller.state()) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}; state {:?}", self.controller.state());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn titles_played(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for state in self.states.lock().unwrap().iter() {
            if state.status == Status::Playing {
                if let Some(t) = &state.track {
                    if seen.last() != Some(&t.id()) {
                        seen.push(t.id());
                    }
                }
            }
        }
        seen
    }

    fn error_count(&self) -> usize {
        self.events.lock().unwrap().iter().filter(|e| matches!(e, UiEvent::Error { .. })).count()
    }
}

fn media_dir() -> (tempfile::TempDir, Server) {
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(
        dir.path(),
        &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]),
    );
    let server = Server::start(dir.path().to_path_buf());
    (dir, server)
}

#[test]
fn queue_plays_through_and_stops_at_the_end() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 20.0);
    let url = format!("{}/index.m3u8", media.base);
    rig.controller.play_tracks(vec![track(1, Some(url.clone())), track(2, Some(url.clone())), track(3, Some(url))], 0);
    rig.wait("the queue to finish", |s| s.status == Status::Idle && s.position == 0.0 && s.track.as_ref().map(|t| t.track_id) == Some(3));
    assert_eq!(rig.titles_played(), vec!["7_1", "7_2", "7_3"]);
    assert_eq!(rig.error_count(), 0);
    assert!(vk.calls.lock().unwrap().is_empty(), "no refresh needed for working URLs");
}

#[test]
fn an_expired_url_is_refreshed_once_and_then_plays() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 20.0);
    // The held URL 404s; audio.getById supplies a working one.
    rig.controller.play_tracks(vec![track(1, Some(format!("{}/expired.m3u8", media.base)))], 0);
    rig.wait("playback of the refreshed URL", |s| s.status == Status::Playing || (s.status == Status::Idle && s.position == 0.0 && vk_called(&vk)));
    rig.wait("end", |s| s.status == Status::Idle && s.track.as_ref().map(|t| t.title.clone()) == Some("Fresh".into()));
    let calls = vk.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].contains("/method/audio.getById"), "{calls:?}");
    assert!(calls[0].contains("audios=7_1"), "{calls:?}");
    assert!(calls[0].contains("access_token="), "token travels in the body");
    assert!(!calls[0].split_whitespace().next().unwrap().contains("access_token"), "never in the URL");
}

fn vk_called(vk: &FakeVk) -> bool {
    !vk.calls.lock().unwrap().is_empty()
}

#[test]
fn a_track_without_a_url_is_looked_up() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 20.0);
    rig.controller.play_tracks(vec![track(4, None)], 0);
    rig.wait("end", |s| s.status == Status::Idle && s.track.as_ref().map(|t| t.title.clone()) == Some("Fresh".into()));
    assert_eq!(vk.calls.lock().unwrap().len(), 1);
}

#[test]
fn dead_tracks_are_skipped_but_only_a_few_in_a_row() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), false); // refresh also fails
    let rig = rig(&vk.base, 20.0);
    let dead = |n| track(n, Some(format!("{}/dead{n}.m3u8", media.base)));
    rig.controller.play_tracks((1..=8).map(dead).collect(), 0);
    rig.wait("giving up", |s| s.status == Status::Idle && rig_has_errors(&s.track));
    std::thread::sleep(Duration::from_millis(300));
    let at = rig.controller.state().track.unwrap().track_id;
    assert!(at <= 3, "skipped past the limit: stopped at track {at}");
    assert!(rig.error_count() >= 3);
    assert_eq!(rig.controller.state().status, Status::Idle);
}

fn rig_has_errors(track: &Option<Track>) -> bool {
    track.is_some()
}

#[test]
fn next_previous_repeat_and_jump() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 1.0);
    let url = format!("{}/index.m3u8", media.base);
    rig.controller.play_tracks((1..=3).map(|n| track(n, Some(url.clone()))).collect(), 0);
    rig.wait("first track", |s| s.status == Status::Playing);

    rig.controller.next();
    rig.wait("second", |s| s.track.as_ref().map(|t| t.track_id) == Some(2));
    // Within 4 s Previous goes to the previous track.
    rig.controller.previous();
    rig.wait("back to first", |s| s.track.as_ref().map(|t| t.track_id) == Some(1));
    rig.controller.jump(2);
    rig.wait("third", |s| s.track.as_ref().map(|t| t.track_id) == Some(3));
    // Next on the last track with repeat off does nothing.
    rig.controller.next();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(rig.controller.state().track.unwrap().track_id, 3);
    rig.controller.set_repeat(Repeat::All);
    rig.controller.next();
    rig.wait("wrap to the first", |s| s.track.as_ref().map(|t| t.track_id) == Some(1));
    assert_eq!(rig.controller.state().repeat, Repeat::All);
}

#[test]
fn removing_the_current_track_starts_the_next_and_removing_all_stops() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 1.0);
    let url = format!("{}/index.m3u8", media.base);
    rig.controller.play_tracks((1..=2).map(|n| track(n, Some(url.clone()))).collect(), 0);
    rig.wait("playing", |s| s.status == Status::Playing);
    rig.controller.remove_from_queue(0);
    rig.wait("second track", |s| s.track.as_ref().map(|t| t.track_id) == Some(2) && s.status == Status::Playing);
    assert_eq!(rig.controller.state().queue_len, 1);
    rig.controller.remove_from_queue(0);
    rig.wait("idle", |s| s.status == Status::Idle && s.queue_len == 0 && s.track.is_none());
}

#[test]
fn pause_resume_seek_volume_and_sign_out() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 1.0);
    let url = format!("{}/index.m3u8", media.base);
    rig.controller.play_tracks(vec![track(1, Some(url))], 0);
    rig.wait("playing", |s| s.status == Status::Playing);
    rig.controller.toggle();
    rig.wait("paused", |s| s.status == Status::Paused);
    rig.controller.toggle();
    rig.wait("playing again", |s| s.status == Status::Playing);
    rig.controller.seek(8.0);
    rig.wait("position after seek", |s| s.position >= 8.0 && s.position < 10.0);
    rig.controller.set_volume(0.4);
    assert_eq!(rig.controller.state().volume, 0.4);
    rig.controller.set_volume(f32::NAN);
    assert_eq!(rig.controller.state().volume, 1.0, "a NaN volume must not reach the device");
    rig.controller.set_session(None);
    rig.wait("signed out", |s| s.status == Status::Idle && s.queue_len == 0);
    assert!(rig.controller.session().is_none());
}

#[test]
fn shuffle_keeps_the_playing_track() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let vk = FakeVk::start(media.base.clone(), true);
    let rig = rig(&vk.base, 1.0);
    let url = format!("{}/index.m3u8", media.base);
    rig.controller.play_tracks((1..=10).map(|n| track(n, Some(url.clone()))).collect(), 3);
    rig.wait("playing", |s| s.status == Status::Playing);
    rig.controller.set_shuffle(true);
    let state = rig.controller.state();
    assert!(state.shuffle);
    assert_eq!(state.track.unwrap().track_id, 4);
}

