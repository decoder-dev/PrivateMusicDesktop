//! Проверки новых релизов и бесконечного радио: выбранный источник пополняет
//! очередь через тестовый API VK и имитатор звукового устройства.

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
        // 12-секундные треки, воспроизводимые в 40 раз быстрее реального времени.
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

// -------------------------------------------- новые выпуски

fn release(id: i64, title: &str) -> String {
    format!(
        r#"{{"id":{id},"owner_id":-2000,"title":"{title}","year":2024,"access_key":"k{id}",
            "main_artists":[{{"name":"Артист","id":"a1"}}],
            "thumb":{{"photo_300":"https://cdn.example/{id}.jpg"}}}}"#
    )
}

/// Аудиострока имеет те же клавиши, что и релиз, плюс продолжительность.
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
            // Клиент iOS отправляет только идентификатор раздела.
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
    // Порядок разделов, никаких повторов; сломанная секция и полка для смешивания ничего не добавляют.
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

// ------------------------------------------------------------------ Радио

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
            Some("100") => (200, tracks_json(&base, &[3, 4])),
            // Первая заправка повторяет два трека, которые уже есть в очереди.
            Some("400") => (200, tracks_json(&base, &[3, 4, 5, 6, 7, 8, 9, 10])),
            Some("500") => (200, tracks_json(&base, &[10, 11, 12, 13, 14])),
            _ => (200, r#"{"response":{"items":[]}}"#.into()),
        }
    });
    let app = app(&api.base);
    let options = r#"{"mood":["calm"]}"#;
    let start = app.stream_mix("m1", Some(options)).unwrap();
    assert_eq!(ids(&start), vec![1, 2, 3, 4]);
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: Some(options.into()) }, start, 0).unwrap();
    assert_eq!(app.controller().state().radio, Some(RadioKind::Catalog));

    // Каждый трек воспроизводится ровно один раз в исходном порядке, затем очередь заканчивается.
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
    // Окно перемещалось страница за страницей, только вперед.
    let offsets: Vec<String> = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, body)| param(body, "offset").map(str::to_string))
        .collect();
    // Четыре стартовые страницы открываются одновременно, поэтому их порядок не фиксирован.
    let mut window: Vec<&str> = offsets[..4].iter().map(String::as_str).collect();
    window.sort();
    assert_eq!(window, ["0", "100", "200", "300"]);
    let refills: Vec<u32> = offsets[4..].iter().map(|o| o.parse().unwrap()).collect();
    assert!(refills.windows(2).all(|w| w[0] <= w[1]), "{refills:?}");
    assert!(refills.contains(&400) && refills.contains(&500), "{refills:?}");
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
            Some("400") => (200, tracks_json(&base, &[3, 4, 5, 6, 7, 8, 9])),
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
    // Четыре стартовые страницы, затем максимум три раунда по три запроса.
    let total = api.call_count("getStreamMixAudios");
    assert!(total <= 4 + 3 * 3, "the radio must not hammer a source with nothing new: {total} calls");
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
        Some("100" | "200" | "300") => (200, r#"{"response":{"items":[]}}"#.into()),
        _ => (200, r#"{"error":{"error_code":100,"error_msg":"nope"}}"#.into()),
    });
    let app = app(&api.base);
    let start = app.stream_mix("m1", None).unwrap();
    app.play_radio(RadioSource::Catalog { mix_id: "m1".into(), options: None }, start, 0).unwrap();
    wait(&app, "the queue to play out", |s| s.status == Status::Idle && s.track.as_ref().is_some_and(|t| t.track_id == 4));
    std::thread::sleep(Duration::from_millis(300));
    // Один неудавшийся запрос за раунд, раунд за запущенный трек, максимум три раунда; неудачный запрос
    // запрашивается снова с тем же смещением и никогда не пропускается.
    let failed: Vec<String> = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, b)| param(b, "offset"))
        .filter(|o| !matches!(*o, "0" | "100" | "200" | "300"))
        .map(str::to_string)
        .collect();
    assert_eq!(failed, vec!["400", "400", "400"], "{failed:?}");
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
        // Каждое семя знает два следующих следа и само семя.
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
        Some("400") => (200, tracks_json(&base, &(2..=200).collect::<Vec<i64>>())),
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
        assert_eq!(app.restore_radio(source.clone(), one.clone(), 0, 0.0, Some(400)), Err(Error::InvalidRequest), "{source:?}");
    }
    assert_eq!(app.play_radio(RadioSource::Personal { options: None }, Vec::new(), 0), Err(Error::InvalidRequest));
    for position in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(app.restore_radio(RadioSource::Personal {options:None},one.clone(),0,position,Some(400)),Err(Error::InvalidRequest));
    }
    assert_eq!(app.restore_radio(RadioSource::Personal {options:None},one.clone(),1,0.0,Some(400)),Err(Error::InvalidRequest));
    assert_eq!(app.restore_radio(RadioSource::Personal {options:None},one,0,0.0,Some(1_000_001)),Err(Error::InvalidRequest));
    assert_eq!(app.controller().state().queue_len, 0);
    assert_eq!(app.controller().state().radio, None);
}

#[test]
fn restoring_radio_keeps_position_and_source_without_starting_network_work() {
    let api = FakeApi::start(|_, _, _| panic!("a restored radio must stay idle"));
    let app = app(&api.base);
    let seed = Track { track_id: 1, owner_id: 7, title: "Saved".into(), artist: "Artist".into(),
        album_title: None, duration: 100, stream_url: Some("https://media.invalid/private-stream".into()),
        artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None, artists: vec![], album: None };
    for source in [RadioSource::Personal {options:None}, RadioSource::Catalog {mix_id:"saved-mix".into(),options:Some(r#"{"mood":["calm"]}"#.into())}, RadioSource::Similar {seed:seed.clone()}] {
        app.restore_radio(source, vec![seed.clone()], 0, 42.0, Some(700)).unwrap();
        let state=app.controller().state();
        assert_eq!(state.status,Status::Idle);
        assert_eq!(state.position,42.0);
        assert_eq!(state.track.as_ref().unwrap().track_id,1);
        let context=state.radio_context.unwrap();
        assert_eq!(context.next_offset,if matches!(context.source,RadioSource::Similar {..}) {0} else {700});
        let json=serde_json::to_string(&context).unwrap();
        assert!(!json.contains("private-stream"));
        assert!(!json.contains("access_token"));
    }
    app.controller().load_queue(vec![seed],0,10.0);
    assert!(app.controller().state().radio_context.is_none());
}

#[test]
fn resumed_radio_refills_from_the_saved_cursor_without_repeating_saved_tracks() {
    if !require_ffmpeg() { return; }
    let (_dir,media)=media_dir();
    let base=media.base.clone();
    let api=FakeApi::start(move |url,body,_| {
        assert_eq!(method(url),"audio.getStreamMixAudios");
        assert_eq!(param(body,"mix_id"),Some("saved"));
        if param(body,"offset")==Some("700") {(200,tracks_json(&base,&[1,2,3,4,5,6,7,8,9]))}
        else {(200,r#"{"response":{"items":[]}}"#.into())}
    });
    let app=app(&api.base);
    let value:serde_json::Value=serde_json::from_str(&tracks_json(&media.base,&[1,2])).unwrap();
    let first=value["response"]["items"].as_array().unwrap().iter().filter_map(|item|pm_engine::vk::model::parse_track(item,Some(7))).collect();
    app.restore_radio(RadioSource::Catalog {mix_id:"saved".into(),options:None},first,0,0.0,Some(700)).unwrap();
    assert_eq!(api.call_count("getStreamMixAudios"),0);
    app.controller().toggle();
    wait(&app,"restored radio refill",|state|state.queue_len==9);
    assert_eq!(app.controller().state().queue_len,9,"saved tracks are deduplicated from the refill");
    let calls=api.calls.lock().unwrap();
    assert_eq!(param(&calls[0].1,"offset"),Some("700"));
}
