//! Downloads: a track is saved through the real HLS path, then plays with the
//! network gone; a lost file falls back to streaming.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::{Status, UiEvent};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, Track, VkClient};

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn hls_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]));
    dir
}

fn track() -> Track {
    Track {
        track_id: 1, owner_id: -3, title: "Saved".into(), artist: "A".into(), album_title: None, duration: 12,
        stream_url: None, artwork_url: Some("https://cdn.example/c.jpg".into()), access_key: None, lyrics_id: None,
        is_hq: false, track_code: None,
    }
}

fn app(vk: &str, downloads: &std::path::Path) -> (App, Arc<Mutex<Vec<UiEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let app = App::new(
        VkClient::with_base(vk).unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 20.0)) as Box<dyn Sink>),
        Box::new(MemoryStore::default()),
        move |e| log.lock().unwrap().push(e),
        AppOptions { fetch: lab_config(), offline_dir: Some(downloads.to_path_buf()), ..AppOptions::default() },
    );
    app.sign_in(Session {
        access_token: "tok".into(), user_id: Some(7), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: "c".into(), web_user_agent: "w".into(),
    });
    (app, events)
}

#[test]
fn a_downloaded_track_plays_without_the_network() {
    if !require_ffmpeg() { return; }
    let media_dir = hls_dir();
    let media = Server::start(media_dir.path().to_path_buf());
    let stream = format!("{}/index.m3u8", media.base);
    let vk = FakeApi::start(move |_, _, _| {
        (200, format!(r#"{{"response":[{{"id":1,"owner_id":-3,"artist":"A","title":"Saved","duration":12,"url":"{stream}"}}]}}"#))
    });
    let downloads = tempfile::tempdir().unwrap();
    let (app, _) = app(&vk.base, downloads.path());

    let saved = app.download(&track()).unwrap();
    assert!(saved.bytes > 50_000, "about 12 s of AAC: {} bytes", saved.bytes);
    assert_eq!(saved.track.artwork_url.as_deref(), Some("https://cdn.example/c.jpg"));
    assert_eq!(app.downloads().len(), 1);
    assert_eq!(app.download(&track()).unwrap(), saved, "a second download is the same file");

    // The CDN and VK go away.
    drop(media);
    drop(vk);
    app.controller().play_tracks(vec![track()], 0);
    wait_for("playing from disk", || app.controller().state().status == Status::Playing);
    wait_for("the end", || app.controller().state().status == Status::Idle);
}

#[test]
fn a_lost_file_falls_back_to_streaming_and_removal_forgets_it() {
    if !require_ffmpeg() { return; }
    let media_dir = hls_dir();
    let media = Server::start(media_dir.path().to_path_buf());
    let stream = format!("{}/index.m3u8", media.base);
    let vk = FakeApi::start(move |_, _, _| {
        (200, format!(r#"{{"response":[{{"id":1,"owner_id":-3,"artist":"A","title":"Saved","duration":12,"url":"{stream}"}}]}}"#))
    });
    let downloads = tempfile::tempdir().unwrap();
    let (app, events) = app(&vk.base, downloads.path());
    let saved = app.download(&track()).unwrap();
    std::fs::remove_file(downloads.path().join(&saved.file)).unwrap();

    app.controller().play_tracks(vec![track()], 0);
    wait_for("playing from VK", || app.controller().state().status == Status::Playing);
    assert!(!events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::Error { .. })), "the fallback is silent");

    assert!(app.remove_download(&track().id()).unwrap());
    assert!(app.downloads().is_empty());
}
