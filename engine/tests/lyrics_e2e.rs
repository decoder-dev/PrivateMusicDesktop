//! Lyrics through the app layer: LRCLIB first, VK's own text second, and the
//! difference between "nothing exists" and "could not ask".

mod common;

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, Track, VkClient};
use pm_engine::Error;

fn track(lyrics_id: Option<i64>) -> Track {
    Track {
        track_id: 5, owner_id: 7, title: "Дорога".into(), artist: "Группа".into(), album_title: None, duration: 200,
        stream_url: None, artwork_url: None, access_key: None, lyrics_id, is_hq: false, track_code: None,
        artists: Vec::new(), album: None,
    }
}

fn app(vk: &str, lrclib: &str) -> App {
    // No Genius: these tests never reach the real site.
    app_with_genius(vk, lrclib, "")
}

fn app_with_genius(vk: &str, lrclib: &str, genius: &str) -> App {
    let store = MemoryStore::default();
    let app = App::new(
        VkClient::with_base(vk).unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 1.0)) as Box<dyn Sink>),
        Box::new(store),
        |_| {},
        AppOptions { fetch: lab_config(), lyrics_base: format!("{lrclib}/"), genius_base: genius.to_string(), ..AppOptions::default() },
    );
    app.sign_in(Session {
        access_token: "tok".into(), user_id: Some(7), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: "c".into(), web_user_agent: "w".into(),
    });
    app
}

const SYNCED: &str = r#"{"trackName":"Дорога","artistName":"Группа","duration":201,"plainLyrics":"раз\nдва","syncedLyrics":"[00:01.00] раз\n[00:02.50] два"}"#;
const PLAIN_ONLY: &str = r#"{"trackName":"Дорога","artistName":"Группа","duration":200,"plainLyrics":"раз\nдва","syncedLyrics":null}"#;

#[test]
fn timed_lyrics_from_lrclib_win_and_vk_is_not_asked() {
    let lrclib = FakeApi::start(|_, _, _| (200, SYNCED.to_string()));
    let vk = FakeApi::start(|_, _, _| (200, r#"{"response":{"text":"vk text"}}"#.to_string()));
    let app = app(&vk.base, &lrclib.base);
    let lyrics = app.lyrics(&track(Some(9))).unwrap().unwrap();
    assert_eq!(lyrics.source, "LRCLIB");
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[1].time, 2.5);
    assert_eq!(vk.call_count("getLyrics"), 0);
    // The exact lookup carries the length, and nothing but the track's own fields.
    let calls = lrclib.calls.lock().unwrap();
    assert!(calls[0].0.starts_with("/api/get?") && calls[0].0.contains("duration=200"), "{calls:?}");
}

#[test]
fn without_timing_vk_text_is_preferred_over_plain_lrclib() {
    let lrclib = FakeApi::start(|url, _, _| if url.starts_with("/api/get") { (404, "{}".into()) } else { (200, format!("[{PLAIN_ONLY}]")) });
    let vk = FakeApi::start(|_, body, _| {
        assert!(body.contains("lyrics_id=9"), "{body}");
        (200, r#"{"response":{"text":"  слова из VK \n"}}"#.to_string())
    });
    let app = app(&vk.base, &lrclib.base);
    let lyrics = app.lyrics(&track(Some(9))).unwrap().unwrap();
    assert_eq!((lyrics.source.as_str(), lyrics.text.as_str()), ("VK", "слова из VK"));
    assert!(lyrics.lines.is_empty());
}

#[test]
fn plain_lrclib_text_is_the_last_resort_and_nothing_found_is_not_an_error() {
    let lrclib = FakeApi::start(|url, _, _| if url.starts_with("/api/get") { (404, "{}".into()) } else { (200, format!("[{PLAIN_ONLY}]")) });
    let vk = FakeApi::start(|_, _, _| (200, r#"{"response":{"text":""}}"#.to_string()));
    let app = app(&vk.base, &lrclib.base);
    let lyrics = app.lyrics(&track(Some(9))).unwrap().unwrap();
    assert_eq!((lyrics.source.as_str(), lyrics.text.as_str()), ("LRCLIB", "раз\nдва"));

    let empty = FakeApi::start(|_, _, _| (200, "[]".to_string()));
    let app = self::app(&vk.base, &empty.base);
    assert!(app.lyrics(&track(None)).unwrap().is_none());
}

#[test]
fn a_failing_source_is_reported_only_when_nothing_else_answered() {
    let broken = FakeApi::start(|_, _, _| (500, "oops".to_string()));
    let vk = FakeApi::start(|_, _, _| (200, r#"{"response":{"text":"vk text"}}"#.to_string()));
    // LRCLIB down, VK has text: the user gets the text.
    let found = app(&vk.base, &broken.base).lyrics(&track(Some(9))).unwrap().unwrap();
    assert_eq!(found.source, "VK");
    // LRCLIB down and no other source: say so instead of "no lyrics".
    let error = app(&vk.base, &broken.base).lyrics(&track(None)).unwrap_err();
    assert!(matches!(error, Error::Http(500)), "{error:?}");
}

#[test]
fn a_different_song_is_never_returned() {
    let wrong = r#"[{"trackName":"Совсем другая","artistName":"Другой","duration":200,"plainLyrics":"чужие слова","syncedLyrics":null}]"#;
    let lrclib = FakeApi::start(move |_, _, _| (200, wrong.to_string()));
    let vk = FakeApi::start(|_, _, _| (200, "{}".to_string()));
    assert!(app(&vk.base, &lrclib.base).lyrics(&track(None)).unwrap().is_none());
}

fn query_of(url: &str) -> std::collections::HashMap<String, String> {
    url::Url::parse(&format!("http://x{url}")).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

fn speed_up_track(lyrics_id: Option<i64>) -> Track {
    Track {
        title: "Дорога в ад (speed up)".into(), artist: "Чучело".into(), album_title: Some("A & B".into()),
        ..track(lyrics_id)
    }
}

#[test]
fn not_found_is_never_an_error() {
    let lrclib = FakeApi::start(|_, _, _| (404, r#"{"code":404,"name":"TrackNotFound"}"#.to_string()));
    let vk = FakeApi::start(|_, _, _| (200, "{}".to_string()));
    assert!(app(&vk.base, &lrclib.base).lyrics(&track(None)).unwrap().is_none());
    // The search endpoint answering 404 or an empty list is the same.
    assert!(lrclib.call_count("/api/search") >= 1);
}

#[test]
fn a_503_from_lrclib_is_an_error_when_nothing_else_could_answer() {
    let lrclib = FakeApi::start(|_, _, _| (503, "down".to_string()));
    let vk = FakeApi::start(|_, _, _| (200, "{}".to_string()));
    let error = app(&vk.base, &lrclib.base).lyrics(&track(None)).unwrap_err();
    assert!(matches!(error, Error::Http(503)), "{error:?}");
}

#[test]
fn lrclib_down_but_vk_and_genius_say_none_is_not_found() {
    let lrclib = FakeApi::start(|_, _, _| (503, "down".to_string()));
    let vk = FakeApi::start(|_, _, _| (200, r#"{"response":{"text":""}}"#.to_string()));
    let genius = FakeApi::start(|_, _, _| (200, r#"{"response":{"sections":[]}}"#.to_string()));
    let app = app_with_genius(&vk.base, &lrclib.base, &format!("{}/", genius.base));
    assert!(app.lyrics(&track(Some(9))).unwrap().is_none());
    assert_eq!(genius.call_count("/api/search/song"), 1);
    // Without a definitive answer from anyone it is still a failure.
    let broken_genius = FakeApi::start(|_, _, _| (403, "no".to_string()));
    let app = app_with_genius(&vk.base, &lrclib.base, &format!("{}/", broken_genius.base));
    let vk_down = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":15,"error_msg":"Access denied"}}"#.to_string()));
    let app_down = self::app_with_genius(&vk_down.base, &lrclib.base, &format!("{}/", broken_genius.base));
    assert!(matches!(app_down.lyrics(&track(Some(9))).unwrap_err(), Error::Http(503)));
    // VK saying "none" is enough on its own.
    assert!(app.lyrics(&track(Some(9))).unwrap().is_none());
}

#[test]
fn unauthorized_from_vk_still_propagates() {
    let lrclib = FakeApi::start(|_, _, _| (404, "{}".to_string()));
    let vk = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":5,"error_msg":"User authorization failed"}}"#.to_string()));
    let error = app(&vk.base, &lrclib.base).lyrics(&track(Some(9))).unwrap_err();
    assert!(matches!(error, Error::Unauthorized), "{error:?}");
}

#[test]
fn a_failed_exact_lookup_does_not_stop_the_searches() {
    // /api/get is broken, /api/search works: the text is found.
    let lrclib = FakeApi::start(|url, _, _| if url.starts_with("/api/get") { (500, "oops".into()) } else { (200, format!("[{PLAIN_ONLY}]")) });
    let vk = FakeApi::start(|_, _, _| (200, "{}".to_string()));
    let lyrics = app(&vk.base, &lrclib.base).lyrics(&track(None)).unwrap().unwrap();
    assert_eq!(lyrics.text, "раз\nдва");
}

#[test]
fn a_title_with_a_speed_up_tag_is_found_and_the_query_is_encoded() {
    let entry = r#"[{"trackName":"Дорога в ад","artistName":"Чучело","duration":150,"plainLyrics":"слова дороги","syncedLyrics":"[00:01.00] слова\n[00:02.00] дороги"}]"#;
    let lrclib = FakeApi::start(move |url, _, _| {
        let q = query_of(url);
        // Only the cleaned title is known to LRCLIB.
        if url.starts_with("/api/search") && q.get("track_name").map(String::as_str) == Some("Дорога в ад") {
            (200, entry.to_string())
        } else if url.starts_with("/api/get") {
            (404, "{}".to_string())
        } else {
            (200, "[]".to_string())
        }
    });
    let vk = FakeApi::start(|_, _, _| (200, "{}".to_string()));
    let mut t = speed_up_track(None);
    t.duration = 150;
    let lyrics = app(&vk.base, &lrclib.base).lyrics(&t).unwrap().unwrap();
    assert_eq!(lyrics.source, "LRCLIB");
    assert_eq!(lyrics.lines.len(), 2);
    let calls = lrclib.calls.lock().unwrap();
    // Raw characters never reach the wire: Cyrillic, parentheses and & are escaped.
    for (url, _) in calls.iter() {
        assert!(url.is_ascii() && !url.contains('(') && !url.contains(' '), "{url}");
    }
    let get = calls.iter().find(|(u, _)| u.starts_with("/api/get")).unwrap();
    let q = query_of(&get.0);
    assert_eq!(q["track_name"], "Дорога в ад (speed up)");
    assert_eq!(q["album_name"], "A & B");
    assert_eq!(q["duration"], "150");
}

#[test]
fn vk_text_is_used_when_lrclib_has_nothing() {
    let lrclib = FakeApi::start(|_, _, _| (200, "[]".to_string()));
    let vk = FakeApi::start(|_, _, _| (200, r#"{"response":{"text":"слова ВК"}}"#.to_string()));
    let lyrics = app(&vk.base, &lrclib.base).lyrics(&speed_up_track(Some(3))).unwrap().unwrap();
    assert_eq!((lyrics.source.as_str(), lyrics.text.as_str()), ("VK", "слова ВК"));
}
