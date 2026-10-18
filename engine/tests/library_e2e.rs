//! Добавление и удаление из «Моя музыка»: правильные параметры, пользовательская копия возвращается, а
//! изменение никогда не отправляется дважды.

mod common;

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, Track, VkClient};
use pm_engine::Error;

fn app(vk: &str) -> App {
    let app = App::new(
        VkClient::with_base(vk).unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 1.0)) as Box<dyn Sink>),
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

fn foreign() -> Track {
    Track {
        track_id: 456, owner_id: -100, title: "T".into(), artist: "A".into(), album_title: None, duration: 180,
        stream_url: Some("https://cdn.example/x.m3u8".into()), artwork_url: None, access_key: Some("key".into()),
        lyrics_id: Some(3), is_hq: false, track_code: None, artists: Vec::new(), album: None,
    }
}

#[test]
fn adding_returns_the_users_copy() {
    let api = FakeApi::start(|url, body, _| {
        assert!(url.ends_with("/method/audio.add"), "{url}");
        assert!(body.contains("audio_id=456") && body.contains("owner_id=-100") && body.contains("access_key=key"), "{body}");
        (200, r#"{"response":789}"#.into())
    });
    let copy = app(&api.base).add_to_library(&foreign()).unwrap();
    assert_eq!((copy.track_id, copy.owner_id), (789, 7));
    assert_eq!(copy.title, "T");
    assert_eq!(copy.access_key, None, "the user's own copy needs no key");
}

#[test]
fn a_failed_add_is_not_retried() {
    let api = FakeApi::start(|_, _, _| (502, "bad gateway".into()));
    let error = app(&api.base).add_to_library(&foreign()).unwrap_err();
    assert_eq!(error,Error::UnconfirmedWrite);
    assert_eq!(api.call_count("audio.add"), 1, "a retry could add the track twice");
}

#[test]
fn removing_sends_the_copy_and_reports_vk_errors() {
    let api = FakeApi::start(|url, body, _| {
        assert!(url.ends_with("/method/audio.delete"));
        if body.contains("audio_id=1&") || body.ends_with("audio_id=1") {
            return (200, r#"{"error":{"error_code":15,"error_msg":"Access denied"}}"#.into());
        }
        assert!(body.contains("audio_id=789") && body.contains("owner_id=7"), "{body}");
        (200, r#"{"response":1}"#.into())
    });
    let app = app(&api.base);
    let mine = Track { track_id: 789, owner_id: 7, ..foreign() };
    app.remove_from_library(&mine).unwrap();
    let denied = Track { track_id: 1, owner_id: 7, ..foreign() };
    assert!(matches!(app.remove_from_library(&denied), Err(Error::Vk { code: 15, .. })));
}

#[test]
fn nonsense_ids_never_reach_vk() {
    let api = FakeApi::start(|_, _, _| panic!("no request expected"));
    let app = app(&api.base);
    for bad in [Track { track_id: 0, ..foreign() }, Track { owner_id: 0, ..foreign() }] {
        assert!(matches!(app.add_to_library(&bad), Err(Error::InvalidRequest)));
        assert!(matches!(app.remove_from_library(&bad), Err(Error::InvalidRequest)));
    }
}
