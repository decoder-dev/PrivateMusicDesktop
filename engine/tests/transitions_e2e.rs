//! Реальное MP3-декодирование и один непрерывный аудиовыход при подготовленном переходе.
mod common;
use common::*;
use pm_engine::audio::{Command, PlayerEvent, PlayerHandle, SimulatedSink, Sink};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait(events: &Arc<Mutex<Vec<PlayerEvent>>>, predicate: impl Fn(&PlayerEvent) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if events.lock().unwrap().iter().any(&predicate) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "events: {:?}",
            events.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn transition(crossfade: f64) -> usize {
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(
        dir.path(),
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "libmp3lame",
            "tone.mp3",
        ],
    );
    let server = Server::start(dir.path().into());
    let sink = SimulatedSink::new(48000, 2, 3.0);
    let captured = sink.captured.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let mut slot = Some(sink);
    let player = PlayerHandle::spawn(
        move || Ok(Box::new(slot.take().expect("must keep one output")) as Box<dyn Sink>),
        move |event| log.lock().unwrap().push(event),
    );
    player.send(Command::SetTransitions {
        gapless: true,
        crossfade,
    });
    player.set_visualizer(true);
    player.play(
        "7_1",
        &format!("{}/tone.mp3", server.base),
        3.0,
        0.0,
        lab_config(),
    );
    wait(
        &events,
        |event| matches!(event,PlayerEvent::Playing{track_id}if track_id=="7_1"),
    );
    player.send(Command::Prepare {
        track_id: "7_2".into(),
        url: format!("{}/tone.mp3", server.base),
        duration: 3.0,
        fetch: lab_config(),
    });
    wait(
        &events,
        |event| matches!(event,PlayerEvent::Transition{from,to}if from=="7_1"&&to=="7_2"),
    );
    wait(
        &events,
        |event| matches!(event,PlayerEvent::Ended{track_id}if track_id=="7_2"),
    );
    assert!(!events
        .lock()
        .unwrap()
        .iter()
        .any(|event| matches!(event, PlayerEvent::Failed { .. })));
    let log = events.lock().unwrap();
    let at = log
        .iter()
        .position(|event| matches!(event, PlayerEvent::Transition { .. }))
        .unwrap();
    assert!(log[at+1..].iter().any(|event|matches!(event,PlayerEvent::Spectrum{bands}if bands.iter().any(|value|*value>0.1))),"visualizer must follow the next track");
    drop(log);
    let len = captured.lock().unwrap().len();
    len
}
#[test]
fn gapless_keeps_both_tracks_and_crossfade_overlaps_the_actual_tail() {
    if !require_ffmpeg() {
        return;
    }
    let continuous = transition(0.0);
    let mixed = transition(0.75);
    assert!(
        (continuous as f64 / 96000.0 - 6.0).abs() < 0.15,
        "{continuous}"
    );
    assert!((mixed as f64 / 96000.0 - 5.25).abs() < 0.15, "{mixed}");
}
#[test]
fn cancelling_preparation_never_starts_the_next_track() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(
        dir.path(),
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
            "-c:a",
            "libmp3lame",
            "tone.mp3",
        ],
    );
    let server = Server::start(dir.path().into());
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let player = PlayerHandle::spawn(
        || Ok(Box::new(SimulatedSink::new(48000, 2, 2.0)) as Box<dyn Sink>),
        move |event| log.lock().unwrap().push(event),
    );
    player.play(
        "1",
        &format!("{}/tone.mp3", server.base),
        2.0,
        0.0,
        lab_config(),
    );
    wait(&events, |event| {
        matches!(event, PlayerEvent::Playing { .. })
    });
    player.send(Command::Prepare {
        track_id: "2".into(),
        url: format!("{}/tone.mp3", server.base),
        duration: 2.0,
        fetch: lab_config(),
    });
    player.send(Command::ClearPrepared);
    wait(
        &events,
        |event| matches!(event,PlayerEvent::Ended{track_id}if track_id=="1"),
    );
    assert!(!events
        .lock()
        .unwrap()
        .iter()
        .any(|event| matches!(event, PlayerEvent::Transition { .. })));
}
