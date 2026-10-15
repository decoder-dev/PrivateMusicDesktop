//! The player thread against a simulated sound card: events, pause, seek,
//! stop, track changes, errors and resampling, all on real HLS from ffmpeg.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use pm_engine::audio::{PlayerEvent, PlayerHandle, SimulatedSink, Sink};

struct Rig {
    player: PlayerHandle,
    events: Arc<Mutex<Vec<PlayerEvent>>>,
    captured: Arc<Mutex<Vec<f32>>>,
    volume: Arc<std::sync::atomic::AtomicU32>,
}

fn rig(rate: u32, speed: f64) -> Rig {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = SimulatedSink::new(rate, 2, speed);
    let captured = sink.captured.clone();
    let volume = sink.volume.clone();
    let mut slot = Some(sink);
    let log = events.clone();
    let player = PlayerHandle::spawn(
        move || slot.take().map(|s| Box::new(s) as Box<dyn Sink>).ok_or(pm_engine::Error::Stream("one sink".into())),
        move |event| log.lock().unwrap().push(event),
    );
    Rig { player, events, captured, volume }
}

impl Rig {
    fn wait<F: Fn(&[PlayerEvent]) -> bool>(&self, what: &str, f: F) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if f(&self.events.lock().unwrap()) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}; events: {:?}", self.events.lock().unwrap());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn has(&self, f: impl Fn(&PlayerEvent) -> bool) -> bool {
        self.events.lock().unwrap().iter().any(f)
    }

    fn last_position(&self) -> f64 {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|e| if let PlayerEvent::Position { seconds, .. } = e { Some(*seconds) } else { None })
            .unwrap_or(-1.0)
    }
}

fn hls(dir: &std::path::Path) {
    run_ffmpeg(
        dir,
        &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]),
    );
}

fn pcm_of(rig: &Rig, rate: u32) -> Pcm {
    Pcm { rate, channels: 2, samples: rig.captured.lock().unwrap().clone() }
}

#[test]
fn plays_a_track_to_its_end_and_resamples_to_the_device() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    // The device runs at 48 kHz, the track is 44.1 kHz; 20x real time.
    let rig = rig(48_000, 20.0);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.wait("the end", |e| e.iter().any(|x| matches!(x, PlayerEvent::Ended { .. })));

    let events = rig.events.lock().unwrap().clone();
    assert!(matches!(events.first(), Some(PlayerEvent::Loading { .. })), "{events:?}");
    let loading = events.iter().position(|e| matches!(e, PlayerEvent::Loading { .. })).unwrap();
    let playing = events.iter().position(|e| matches!(e, PlayerEvent::Playing { .. })).unwrap();
    let ended = events.iter().position(|e| matches!(e, PlayerEvent::Ended { .. })).unwrap();
    assert!(loading < playing && playing < ended, "{events:?}");
    assert!(!events.iter().any(|e| matches!(e, PlayerEvent::Failed { .. })), "{events:?}");

    // The sink was flushed at the very start only; everything written is the track.
    let pcm = pcm_of(&rig, 48_000);
    assert!((pcm.seconds() - 12.0).abs() < 0.3, "device audio is {} s", pcm.seconds());
    assert!((pcm.frequency(1.0, 5.0) - 440.0).abs() < 10.0);
    assert!((pcm.frequency(7.0, 11.0) - 880.0).abs() < 20.0);
}

#[test]
fn pause_holds_the_position_and_resume_continues() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 2.0);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.wait("playing past 1 s", |_| true);
    let start = Instant::now();
    while rig.last_position() < 1.0 {
        assert!(start.elapsed() < Duration::from_secs(10), "never reached 1 s");
        std::thread::sleep(Duration::from_millis(10));
    }
    rig.player.pause();
    rig.wait("paused", |e| e.iter().any(|x| matches!(x, PlayerEvent::Paused { .. })));
    std::thread::sleep(Duration::from_millis(100));
    let held = rig.last_position();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(rig.last_position(), held, "position moved while paused");
    rig.player.resume();
    let resumed = Instant::now();
    while rig.last_position() <= held {
        assert!(resumed.elapsed() < Duration::from_secs(10), "did not resume");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn seek_jumps_and_the_audio_after_it_is_the_right_part() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 1.0);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.wait("playing", |e| e.iter().any(|x| matches!(x, PlayerEvent::Playing { .. })));
    rig.player.seek(8.0);
    // The seek reports the new position at once.
    let start = Instant::now();
    while !(rig.last_position() >= 8.0 && rig.last_position() < 9.5) {
        assert!(start.elapsed() < Duration::from_secs(10), "position after seek: {}", rig.last_position());
        std::thread::sleep(Duration::from_millis(10));
    }
    // Wait until about a second of the 880 Hz section is in the sink.
    let wait = Instant::now();
    while rig.captured.lock().unwrap().len() < 44_100 * 2 {
        assert!(wait.elapsed() < Duration::from_secs(10), "no audio after seek");
        std::thread::sleep(Duration::from_millis(10));
    }
    let pcm = pcm_of(&rig, 44_100);
    assert!((pcm.frequency(0.1, 0.9) - 880.0).abs() < 20.0, "audio after seek is {} Hz", pcm.frequency(0.1, 0.9));
}

#[test]
fn start_offset_begins_mid_track() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 1.0);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 9.0, lab_config());
    rig.wait("playing", |e| e.iter().any(|x| matches!(x, PlayerEvent::Playing { .. })));
    let wait = Instant::now();
    while rig.captured.lock().unwrap().len() < 44_100 * 2 {
        assert!(wait.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!((pcm_of(&rig, 44_100).frequency(0.1, 0.9) - 880.0).abs() < 20.0);
    assert!(rig.last_position() >= 9.0);
}

#[test]
fn stop_is_immediate_and_is_not_reported_as_the_end() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 1.0);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.wait("playing", |e| e.iter().any(|x| matches!(x, PlayerEvent::Playing { .. })));
    rig.player.stop();
    rig.wait("stopped", |e| e.iter().any(|x| matches!(x, PlayerEvent::Stopped)));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!rig.has(|e| matches!(e, PlayerEvent::Ended { .. })));
}

#[test]
fn a_new_track_replaces_the_old_one() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "libmp3lame", "-b:a", "128k", "second.mp3"]));
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 20.0);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.player.play("2_2", &format!("{}/second.mp3", server.base), 12.0, 0.0, lab_config());
    rig.wait("second track ending", |e| e.iter().any(|x| matches!(x, PlayerEvent::Ended { track_id } if track_id == "2_2")));
    assert!(!rig.has(|e| matches!(e, PlayerEvent::Ended { track_id } if track_id == "1_1")), "{:?}", rig.events.lock().unwrap());
    let pcm = pcm_of(&rig, 44_100);
    assert!((pcm.seconds() - 12.0).abs() < 0.3, "{} s", pcm.seconds());
}

#[test]
fn a_dead_stream_fails_cleanly_and_the_player_stays_usable() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 20.0);
    rig.player.play("9_9", &format!("{}/nothing.m3u8", server.base), 10.0, 0.0, lab_config());
    rig.wait("failure", |e| e.iter().any(|x| matches!(x, PlayerEvent::Failed { track_id, .. } if track_id == "9_9")));
    std::thread::sleep(Duration::from_millis(200));
    assert!(!rig.has(|e| matches!(e, PlayerEvent::Ended { track_id } if track_id == "9_9")), "a failed track must not also end");
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.wait("recovery", |e| e.iter().any(|x| matches!(x, PlayerEvent::Ended { track_id } if track_id == "1_1")));
}

#[test]
fn volume_reaches_the_device() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    hls(dir.path());
    let server = Server::start(dir.path().to_path_buf());
    let rig = rig(44_100, 20.0);
    rig.player.set_volume(0.25);
    rig.player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    rig.wait("end", |e| e.iter().any(|x| matches!(x, PlayerEvent::Ended { .. })));
    assert_eq!(f32::from_bits(rig.volume.load(Ordering::Relaxed)), 0.25);
}

#[test]
fn no_audio_device_is_reported_as_a_failure() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let player = PlayerHandle::spawn(
        || Err(pm_engine::Error::Stream("no audio output device".into())),
        move |e| log.lock().unwrap().push(e),
    );
    player.play("1_1", "https://cdn.example/a/index.m3u8", 10.0, 0.0, lab_config());
    let start = Instant::now();
    while events.lock().unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(events.lock().unwrap()[0], PlayerEvent::Failed { .. }));
}

#[test]
fn a_nan_volume_never_reaches_the_device() {
    let rig = rig(44_100, 1.0);
    rig.player.set_volume(f32::NAN);
    rig.player.set_volume(f32::INFINITY);
    std::thread::sleep(Duration::from_millis(100));
    // The sink is created lazily at the first Play, so ask the thread to open it.
    rig.player.play("1_1", "http://127.0.0.1:9/nothing.m3u8", 1.0, 0.0, lab_config());
    let start = Instant::now();
    while !rig.has(|e| matches!(e, PlayerEvent::Failed { .. })) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    let gain = f32::from_bits(rig.volume.load(Ordering::Relaxed));
    assert!(gain.is_finite() && (0.0..=1.0).contains(&gain), "gain {gain}");
}
