//! New releases from the catalogue, and the endless radio: a queue started
//! from a mix, the personal stream or "more like this" refills itself from
//! the same source against a fake VK and a simulated sound card.

mod common;

use std::time::{Duration, Instant};

use common::*;
use pm_engine::app::{App, AppOptions, RadioSource};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::{State, Status};
use pm_engine::radio::RadioKind;
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, Track, VkClient};
use pm_engine::Error;

fn app(vk: &str) -> App {
    let app = App::new(
        VkClient::with_base(vk).unwrap(),
        // 12 s tracks, played 40 times faster than real time.
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 40.0)) as Box<dyn Sink>),
        Box::new(MemoryStore::default()),
        |_| {},
        AppOptions { fetch: lab_config(), ..AppOptions::default() },
    );
    app.sign_in(Session {
        access_token: "tok".into(), user_id: Some(7), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: "c".into(), web_user_agent: "w".into(),
    });
    app
}

fn method(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("")
}

fn param<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    body.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

// ------------------------------------------------------------ new releases

fn release(id: i64, title: &str) -> String {
    format!(
        r#"{{"id":{id},"owner_id":-2000,"title":"{title}","year":2024,"access_key":"k{id}",
            "main_artists":[{{"name":"Артист","id":"a1"}}],
            "thumb":{{"photo_300":"https://cdn.example/{id}.jpg"}}}}"#
    )
}

/// An audio row has the same keys as a release plus a duration.
fn audio_row(id: i64) -> String {
    format!(r#"{{"id":{id},"owner_id":-5,"title":"Трек {id}","artist":"Артист","duration":200,"main_artists":[{{"name":"Артист"}}]}}"#)
}

#[test]
fn releases_in_the_root_catalogue_are_listed_once_without_audio_rows() {
    let api = FakeApi::start(|url, body, _| {
        assert_eq!(method(url), "catalog.getAudio", "no section is needed when the root has releases");
        assert!(body.contains("need_blocks=1"), "{body}");
        (200, format!(
            r#"{{"response":{{"catalog":{{"sections":[{{"id":"sec_rel","title":"Новые релизы","blocks":[]}}]}},
                "blocks":[{{"albums":[{},{}]}},{{"audios":[{}]}},{{"albums":[{}]}}]}}}}"#,
            release(11, "Первый"), release(12, "Второй"), audio_row(99), release(11, "Первый")
        ))
    });
    let releases = app(&api.base).new_releases().unwrap();
    assert_eq!(releases.iter().map(|a| a.album_id).collect::<Vec<_>>(), vec![11, 12]);
    assert_eq!(releases[0].title, "Первый");
    assert_eq!(releases[0].year, Some(2024));
    assert_eq!(releases[0].artists[0].name, "Артист");
    assert_eq!(api.call_count("catalog.getSection"), 0);
}

#[test]
fn without_root_releases_only_release_looking_sections_are_hydrated() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "catalog.getAudio" => (200, r#"{"response":{"catalog":{"sections":[
                {"id":"sec_rel","title":"Новые релизы","blocks":[]},
                {"id":"sec_mix","title":"Миксы для вас","blocks":[]},
                {"id":"sec_alb","title":"Свежие альбомы","url":"/audios","blocks":[]},
                {"id":"sec_bad","title":"Премьеры","blocks":[]},
                {"id":"2000123","title":"Новинки","url":"/x"}]}}}"#.into()),
        "catalog.getSection" => {
            // The iOS client sends the section id alone.
            assert!(!body.contains("need_blocks"), "{body}");
            match param(body, "section_id") {
                Some("sec_rel") => (200, format!(r#"{{"response":{{"blocks":[{{"albums":[{},{}]}}]}}}}"#, release(1, "А"), release(2, "Б"))),
                Some("sec_alb") => (200, format!(r#"{{"response":{{"albums":[{},{}]}}}}"#, release(2, "Б"), release(3, "В"))),
                Some("sec_bad") => (200, r#"{"error":{"error_code":100,"error_msg":"gone"}}"#.into()),
                other => panic!("unexpected section {other:?}"),
            }
        }
        other => panic!("unexpected {other}"),
    });
    let releases = app(&api.base).new_releases().unwrap();
    // Section order, no repeats; the broken section and the mix shelf add nothing.
    assert_eq!(releases.iter().map(|a| a.album_id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(api.call_count("catalog.getSection"), 3, "the mix shelf and the numeric id are not asked");
}

#[test]
fn a_refused_catalogue_gives_an_empty_shelf_and_unusable_albums_are_dropped() {
    let api = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":100,"error_msg":"bad"}}"#.into()));
    assert!(app(&api.base).new_releases().unwrap().is_empty());
    let api = FakeApi::start(|_, _, _| {
        (200, format!(r#"{{"response":{{"albums":[{},{}]}}}}"#, release(0, "Пустой"), release(5, "Целый")))
    });
    let releases = app(&api.base).new_releases().unwrap();
    assert_eq!(releases.iter().map(|a| a.album_id).collect::<Vec<_>>(), vec![5]);
}

// ------------------------------------------------------------------- radio

fn tracks_json(media: &str, ids: &[i64]) -> String {
    let rows: Vec<String> = ids
        .iter()
        .map(|id| {
            format!(r#"{{"id":{id},"owner_id":-5,"artist":"A{id}","title":"T{id}","duration":12,"url":"{media}/index.m3u8"}}"#)
        })
        .collect();
    format!(r#"{{"response":{{"items":[{}]}}}}"#, rows.join(","))
}

fn ids(tracks: &[Track]) -> Vec<i64> {
    tracks.iter().map(|t| t.track_id).collect()
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

fn wait(app: &App, what: &str, f: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let state = app.controller().state();
        if f(&state) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; state {state:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_mix_radio_refills_from_the_same_mix_and_plays_every_track_once() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |url, body, _| {
        assert_eq!(method(url), "audio.getStreamMixAudios");
        assert_eq!(param(body, "mix_id"), Some("m1"));
        assert!(body.contains("mix_options"), "the tuning travels with every page: {body}");
        match param(body, "offset") {
            Some("0") => (200, tracks_json(&base, &[1, 2])),
            Some("50") => (200, tracks_json(&base, &[3, 4])),
            // The first refill repeats two tracks the queue already holds.
            Some("100") => (200, tracks_json(&base, &[3, 4, 5, 6, 7, 8, 9, 10])),
            Some("200") => (200, tracks_json(&base, &[10, 11, 12, 13, 14])),
            _ => (200, r#"{"response":{"items":[]}}"#.into()),
        }
    });
    let app = app(&api.base);
    let options = r#"{"mood":["calm"]}"#;
    let start = app.stream_mix("m1", Some(options)).unwrap();
    assert_eq!(ids(&start), vec![1, 2, 3, 4]);
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: Some(options.into()) }, start, 0).unwrap();
    assert_eq!(app.controller().state().radio, Some(RadioKind::Catalog));

    // Every track plays exactly once, in source order, then the queue ends.
    let mut played = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let state = app.controller().state();
        if let Some(track) = &state.track {
            if state.status == Status::Playing && played.last() != Some(&track.track_id) {
                played.push(track.track_id);
            }
        }
        if state.status == Status::Idle && state.track.as_ref().is_some_and(|t| t.track_id == 14) {
            break;
        }
        assert!(Instant::now() < deadline, "timed out; played {played:?}; state {state:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(played, (1..=14).collect::<Vec<i64>>());
    assert_eq!(app.controller().state().queue_len, 14, "repeats never entered the queue");
    // The window moved on page by page, only forward.
    let offsets: Vec<String> = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, body)| param(body, "offset").map(str::to_string))
        .collect();
    assert_eq!(&offsets[..2], ["0", "50"]);
    let refills: Vec<u32> = offsets[2..].iter().map(|o| o.parse().unwrap()).collect();
    assert!(refills.windows(2).all(|w| w[0] <= w[1]), "{refills:?}");
    assert!(refills.contains(&100) && refills.contains(&200), "{refills:?}");
}

#[test]
fn the_personal_mix_radio_asks_for_the_common_stream_beyond_what_was_loaded() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |_, body, _| {
        assert_eq!(param(body, "mix_id"), Some("common"));
        assert_eq!(param(body, "count"), Some("100"));
        match param(body, "offset") {
            Some("100") => (200, tracks_json(&base, &[3, 4, 5, 6, 7, 8, 9])),
            _ => (200, r#"{"response":{"items":[]}}"#.into()),
        }
    });
    let app = app(&api.base);
    let first: Vec<Track> = {
        let json = tracks_json(&media.base, &[1, 2]);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["response"]["items"].as_array().unwrap().iter().filter_map(|o| pm_engine::vk::model::parse_track(o, Some(7))).collect()
    };
    app.play_radio(RadioSource::Personal { options: None }, first, 0).unwrap();
    wait(&app, "the refill", |s| s.queue_len == 9);
    assert_eq!(app.controller().state().radio, Some(RadioKind::Personal));
}

#[test]
fn a_source_that_only_repeats_itself_is_given_up_on() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |_, _, _| (200, tracks_json(&base, &[1, 2, 3, 4])));
    let app = app(&api.base);
    let start = app.stream_mix("m1", None).unwrap();
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: None }, start, 0).unwrap();
    wait(&app, "the queue to play out", |s| s.status == Status::Idle && s.track.as_ref().is_some_and(|t| t.track_id == 4));
    std::thread::sleep(Duration::from_millis(400));
    let state = app.controller().state();
    assert_eq!((state.status, state.queue_len), (Status::Idle, 4), "nothing new, so nothing was added");
    // Two start-up pages, then at most three rounds of three requests.
    let total = api.call_count("getStreamMixAudios");
    assert!(total <= 2 + 3 * 3, "the radio must not hammer a source with nothing new: {total} calls");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(api.call_count("getStreamMixAudios"), total, "a finished radio stays quiet");
}

#[test]
fn errors_count_as_bad_rounds_and_stop_after_three() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |_, body, _| match param(body, "offset") {
        Some("0") => (200, tracks_json(&base, &[1, 2, 3, 4])),
        Some("50") => (200, r#"{"response":{"items":[]}}"#.into()),
        _ => (200, r#"{"error":{"error_code":100,"error_msg":"nope"}}"#.into()),
    });
    let app = app(&api.base);
    let start = app.stream_mix("m1", None).unwrap();
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: None }, start, 0).unwrap();
    wait(&app, "the queue to play out", |s| s.status == Status::Idle && s.track.as_ref().is_some_and(|t| t.track_id == 4));
    std::thread::sleep(Duration::from_millis(300));
    // One failed request per round, a round per started track, three rounds at most;
    // a failed request is asked again at the same offset, never skipped.
    let failed: Vec<String> = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, b)| param(b, "offset"))
        .filter(|o| *o != "0" && *o != "50")
        .map(str::to_string)
        .collect();
    assert_eq!(failed, vec!["100", "100", "100"], "{failed:?}");
    assert_eq!(app.controller().state().queue_len, 4);
}

#[test]
fn more_like_this_rotates_seeds_and_adopts_new_tracks_as_seeds() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |url, body, _| {
        assert_eq!(method(url), "audio.getRecommendations");
        assert_eq!(param(body, "shuffle"), Some("1"), "radio refills are shuffled: {body}");
        let target = param(body, "target_audio").expect("seeded").replace("%5F", "_");
        let seed: i64 = target.rsplit('_').next().unwrap().parse().unwrap();
        // Each seed knows the next two tracks and the seed itself.
        let next: Vec<i64> = if seed < 9 { vec![seed, seed + 1, seed + 2] } else { vec![] };
        (200, tracks_json(&base, &next))
    });
    let app = app(&api.base);
    let seed: Track = {
        let value: serde_json::Value = serde_json::from_str(&tracks_json(&media.base, &[1])).unwrap();
        pm_engine::vk::model::parse_track(&value["response"]["items"][0], Some(7)).unwrap()
    };
    app.play_radio(RadioSource::Similar { seed: seed.clone() }, vec![seed], 0).unwrap();
    assert_eq!(app.controller().state().radio, Some(RadioKind::Similar));
    wait(&app, "the radio to run out", |s| s.status == Status::Idle && s.track.as_ref().is_some_and(|t| t.track_id >= 9));
    let state = app.controller().state();
    assert!(state.queue_len >= 9, "{state:?}");
    let targets: Vec<String> = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, b)| param(b, "target_audio").map(|t| t.replace("%5F", "_")))
        .collect();
    assert_eq!(targets[0], "-5_1", "the first round is seeded with the track it started from");
    assert_ne!(targets[1], "-5_1", "later rounds seed with tracks the radio brought in: {targets:?}");
}

#[test]
fn a_radio_never_queues_more_than_eighty_upcoming_tracks() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |_, body, _| match param(body, "offset") {
        Some("100") => (200, tracks_json(&base, &(2..=200).collect::<Vec<i64>>())),
        _ => (200, tracks_json(&base, &[1])),
    });
    let app = app(&api.base);
    let start = app.stream_mix("m1", None).unwrap();
    assert_eq!(ids(&start), vec![1]);
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: None }, start, 0).unwrap();
    wait(&app, "the refill", |s| s.queue_len > 1);
    std::thread::sleep(Duration::from_millis(200));
    let state = app.controller().state();
    assert_eq!(state.queue_len, 81, "{state:?}");
    app.controller().stop();
}

#[test]
fn playing_something_else_ends_the_radio() {
    if !require_ffmpeg() { return; }
    let (_dir, media) = media_dir();
    let base = media.base.clone();
    let api = FakeApi::start(move |_, body, _| match param(body, "offset") {
        Some("0") => (200, tracks_json(&base, &[1, 2, 3, 4, 5, 6, 7, 8, 9])),
        _ => (200, r#"{"response":{"items":[]}}"#.into()),
    });
    let app = app(&api.base);
    let start = app.stream_mix("m1", None).unwrap();
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: None }, start.clone(), 0).unwrap();
    assert!(app.controller().state().radio.is_some());
    app.controller().play_tracks(start, 0);
    assert_eq!(app.controller().state().radio, None);
    let before = api.call_count("getStreamMixAudios");
    wait(&app, "the end of the plain queue", |s| s.status == Status::Idle && s.track.as_ref().is_some_and(|t| t.track_id == 9));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(api.call_count("getStreamMixAudios"), before, "a plain queue never asks for more");
    assert_eq!(app.controller().state().queue_len, 9);
}

#[test]
fn radio_requests_are_validated_before_anything_changes() {
    let api = FakeApi::start(|_, _, _| panic!("no call is expected"));
    let app = app(&api.base);
    let seed = |track_id, owner_id| Track {
        track_id, owner_id, title: "T".into(), artist: "A".into(), album_title: None, duration: 10, stream_url: None,
        artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None,
    };
    let one = vec![seed(1, 7)];
    let bad = [
        RadioSource::Catalog { mix_id: "  ".into(), options: None },
        RadioSource::Catalog { mix_id: "a\nb".into(), options: None },
        RadioSource::Catalog { mix_id: "x".repeat(300), options: None },
        RadioSource::Catalog { mix_id: "m".into(), options: Some("[1]".into()) },
        RadioSource::Personal { options: Some("not json".into()) },
        RadioSource::Similar { seed: seed(0, 7) },
        RadioSource::Similar { seed: seed(1, 0) },
    ];
    for source in bad {
        assert_eq!(app.play_radio(source.clone(), one.clone(), 0), Err(Error::InvalidRequest), "{source:?}");
    }
    assert_eq!(app.play_radio(RadioSource::Personal { options: None }, Vec::new(), 0), Err(Error::InvalidRequest));
    assert_eq!(app.controller().state().queue_len, 0);
    assert_eq!(app.controller().state().radio, None);
}
