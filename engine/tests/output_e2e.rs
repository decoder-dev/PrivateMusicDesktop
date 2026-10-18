//! Переключение вывода звука во время воспроизведения музыки.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::{Controller, Status};
use pm_engine::vk::{Session, Track, VkClient};

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn switching_output_reopens_the_device_and_keeps_the_place() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]));
    let media = Server::start(dir.path().to_path_buf());
    let opened = Arc::new(AtomicUsize::new(0));
    let count = opened.clone();
    let controller = Controller::with_fetch(
        VkClient::with_base("http://127.0.0.1:9").unwrap(),
        move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(SimulatedSink::new(48_000, 2, 1.0)) as Box<dyn Sink>)
        },
        |_| {},
        1.0,
        lab_config(),
    );
    controller.set_session(Some(Session {
        access_token: "x".repeat(20), user_id: Some(7), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: String::new(), web_user_agent: "w".into(),
    }));
    let track = Track {
        track_id: 1, owner_id: 7, title: "T".into(), artist: "A".into(), album_title: None, duration: 12,
        stream_url: Some(format!("{}/index.m3u8", media.base)), artwork_url: None, access_key: None,
        lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None,
    };
    controller.play_tracks(vec![track], 0);
    wait_for("playing", || controller.state().status == Status::Playing);
    controller.seek(6.0);
    wait_for("past six seconds", || controller.state().position >= 6.0);
    assert_eq!(opened.load(Ordering::SeqCst), 1);

    controller.reopen_output();
    wait_for("playing again", || controller.state().status == Status::Playing && opened.load(Ordering::SeqCst) == 2);
    assert!(controller.state().position >= 5.9, "carries on, not from 0:00: {:?}", controller.state().position);

    // Пауза: ничего не воспроизводится, пока не спросят, затем с того же места.
    controller.pause();
    wait_for("paused", || controller.state().status == Status::Paused);
    let at = controller.state().position;
    controller.reopen_output();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(controller.state().status, Status::Idle);
    controller.toggle();
    wait_for("resumed", || controller.state().status == Status::Playing);
    assert!(controller.state().position >= at - 0.1);
}
