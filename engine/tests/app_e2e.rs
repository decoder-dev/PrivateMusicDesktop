//! The app layer against a fake VK API and a fake token endpoint.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::UiEvent;
use pm_engine::store::{MemoryStore, SecretStore};
use pm_engine::vk::{Session, VkClient};
use pm_engine::Error;

fn session(token: &str) -> Session {
    Session {
        access_token: token.into(),
        user_id: Some(7),
        expires_at: None,
        api_user_agent: "agent-for-api".into(),
        refresh_cookie: "remixsid=cookie".into(),
        web_user_agent: "agent-for-web".into(),
    }
}

struct Shared(Arc<MemoryStore>);
impl SecretStore for Shared {
    fn load(&self) -> pm_engine::Result<Option<Session>> { self.0.load() }
    fn save(&self, s: &Session) -> pm_engine::Result<()> { self.0.save(s) }
    fn clear(&self) -> pm_engine::Result<()> { self.0.clear() }
}

fn app(api_base: &str, token_url: String, store: Arc<MemoryStore>) -> (App, Arc<Mutex<Vec<UiEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let app = App::new(
        VkClient::with_base(api_base).unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 1.0)) as Box<dyn Sink>),
        Box::new(Shared(store)),
        move |e| log.lock().unwrap().push(e),
        AppOptions { fetch: lab_config(), token_url, volume: 1.0 },
    );
    (app, events)
}

fn page_json(ids: &[i64], count: i64) -> String {
    let items: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":{id},"owner_id":7,"artist":"A{id}","title":"T{id}","duration":100,"url":"https://cdn.example/{id}/index.m3u8"}}"#))
        .collect();
    format!(r#"{{"response":{{"count":{count},"items":[{}]}}}}"#, items.join(","))
}

fn token_ok(token: &str) -> String {
    format!(r#"{{"type":"okay","data":{{"access_token":"{token}","user_id":7,"expires":0}}}}"#)
}

#[test]
fn signed_out_calls_are_unauthorized_and_sign_in_persists() {
    let api = FakeApi::start(|_, _, _| (200, page_json(&[1], 1)));
    let store = Arc::new(MemoryStore::default());
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), store.clone());
    assert!(!app.session_state().signed_in);
    assert_eq!(app.library(0, 10).unwrap_err(), Error::Unauthorized);
    assert_eq!(api.call_count("/method/"), 0, "no request without a token");

    app.sign_in(session("tokentokentoken-1"));
    assert!(app.session_state().signed_in);
    assert_eq!(store.load().unwrap().unwrap().access_token, "tokentokentoken-1");
    assert_eq!(app.library(0, 10).unwrap().items.len(), 1);

    app.sign_out();
    assert!(!app.session_state().signed_in);
    assert!(store.load().unwrap().is_none());
}

#[test]
fn a_stored_session_is_picked_up_at_launch() {
    let api = FakeApi::start(|_, _, _| (200, page_json(&[1], 1)));
    let store = Arc::new(MemoryStore::default());
    store.save(&session("remembered-token-1234")).unwrap();
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), store);
    assert!(app.session_state().signed_in);
}

#[test]
fn library_pages_skip_bad_entries_but_advance_by_the_raw_count() {
    let api = FakeApi::start(|_, body, _| {
        let offset: usize = body.split('&').find_map(|p| p.strip_prefix("offset=")).unwrap().parse().unwrap();
        let json = if offset == 0 {
            // An ad object between two tracks.
            r#"{"response":{"count":5,"items":[
              {"id":1,"owner_id":7,"artist":"A","title":"T1"},
              {"type":"ad"},
              {"id":3,"owner_id":7,"artist":"A","title":"T3"}]}}"#
                .to_string()
        } else {
            page_json(&[4, 5], 5)
        };
        (200, json)
    });
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    let first = app.library(0, 3).unwrap();
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.total, 5);
    assert_eq!(first.next_offset, Some(3), "advance by the three raw entries, not the two tracks");
    let second = app.library(first.next_offset.unwrap(), 3).unwrap();
    assert_eq!(second.items.len(), 2);
    assert_eq!(second.next_offset, None);
}

#[test]
fn requests_carry_the_token_in_the_body_and_the_api_user_agent() {
    let api = FakeApi::start(|_, _, _| (200, page_json(&[1], 1)));
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    app.search("Кино", 0, 20).unwrap();
    let calls = api.calls.lock().unwrap().clone();
    let (url, body) = &calls[0];
    assert_eq!(url, "/method/audio.search");
    assert!(body.contains("access_token=tokentokentoken-1"));
    assert!(body.contains("q=%D0%9A%D0%B8%D0%BD%D0%BE"), "{body}");
    assert!(body.contains("v=5.199"));
}

#[test]
fn blank_search_makes_no_request() {
    let api = FakeApi::start(|_, _, _| (200, page_json(&[1], 1)));
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    assert!(app.search("   ", 0, 20).unwrap().items.is_empty());
    assert_eq!(api.call_count("/method/"), 0);
}

#[test]
fn an_expired_token_is_refreshed_once_and_the_call_retried() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let api = FakeApi::start(move |_, body, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        if body.contains("access_token=tokentokentoken-OLD") {
            (200, r#"{"error":{"error_code":5,"error_msg":"User authorization failed"}}"#.into())
        } else {
            (200, page_json(&[1, 2], 2))
        }
    });
    let token = FakeApi::start(|_, body, headers| {
        assert!(body.contains("app_id=6287487"), "{body}");
        assert!(headers.to_lowercase().contains("cookie: remixsid=cookie"), "{headers}");
        assert!(headers.to_lowercase().contains("user-agent: agent-for-web"), "{headers}");
        (200, token_ok("tokentokentoken-NEW"))
    });
    let store = Arc::new(MemoryStore::default());
    let (app, events) = app(&api.base, format!("{}/?act=web_token", token.base), store.clone());
    app.sign_in(session("tokentokentoken-OLD"));
    let page = app.library(0, 10).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(token.call_count("web_token"), 1);
    assert_eq!(store.load().unwrap().unwrap().access_token, "tokentokentoken-NEW", "refreshed token is persisted");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(!events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::SessionExpired)));
}

#[test]
fn a_refused_refresh_signs_the_user_out_and_says_so() {
    let api = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":5,"error_msg":"x"}}"#.into()));
    let token = FakeApi::start(|_, _, _| (200, r#"{"type":"error","error_info":"not signed in"}"#.into()));
    let store = Arc::new(MemoryStore::default());
    let (app, events) = app(&api.base, format!("{}/?act=web_token", token.base), store.clone());
    app.sign_in(session("tokentokentoken-OLD"));
    assert_eq!(app.library(0, 10).unwrap_err(), Error::Unauthorized);
    assert!(!app.session_state().signed_in);
    assert!(store.load().unwrap().is_none(), "dead credentials are removed from the secret store");
    assert!(events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::SessionExpired)));
}

#[test]
fn a_token_close_to_expiry_is_refreshed_before_the_call() {
    let api = FakeApi::start(|_, body, _| {
        assert!(body.contains("tokentokentoken-NEW"), "the call must use the refreshed token: {body}");
        (200, page_json(&[1], 1))
    });
    let token = FakeApi::start(|_, _, _| (200, token_ok("tokentokentoken-NEW")));
    let (app, _) = app(&api.base, format!("{}/?act=web_token", token.base), Arc::new(MemoryStore::default()));
    let mut s = session("tokentokentoken-OLD");
    s.expires_at = Some(1); // long past
    app.sign_in(s);
    app.library(0, 10).unwrap();
    assert_eq!(token.call_count("web_token"), 1);
}

#[test]
fn the_profile_is_asked_for_once() {
    let api = FakeApi::start(|_, _, _| {
        (200, r#"{"response":[{"id":7,"first_name":"Иван","last_name":"Петров","photo_200":"//cdn.example/p.jpg"}]}"#.into())
    });
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    let first = app.profile().unwrap();
    assert_eq!(first.display_name(), "Иван Петров");
    assert_eq!(first.photo_url.as_deref(), Some("https://cdn.example/p.jpg"));
    app.profile().unwrap();
    assert_eq!(api.call_count("users.get"), 1);
}

#[test]
fn playlists_and_their_tracks() {
    let api = FakeApi::start(|url, body, _| {
        if url.contains("audio.getPlaylists") {
            (200, r#"{"response":{"count":2,"items":[
                {"id":11,"owner_id":7,"title":"Дорога","count":2,"access_key":"k1","photo_300":"https://cdn.example/a.jpg"},
                {"id":12,"owner_id":7}]}}"#.into())
        } else {
            assert!(body.contains("album_id=11") && body.contains("access_key=k1"), "{body}");
            (200, page_json(&[1, 2], 2))
        }
    });
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    let lists = app.playlists(0, 50).unwrap();
    assert_eq!(lists.items.len(), 2);
    assert_eq!(lists.items[1].title, "Playlist");
    let tracks = app.playlist_tracks(&lists.items[0], 0, 100).unwrap();
    assert_eq!(tracks.items.len(), 2);
}

#[test]
fn vk_api_errors_other_than_auth_are_passed_through() {
    let api = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":201,"error_msg":"Access denied"}}"#.into()));
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    match app.library(0, 10).unwrap_err() {
        Error::Vk { code, .. } => assert_eq!(code, 201),
        other => panic!("{other:?}"),
    }
    assert!(app.session_state().signed_in, "an ordinary API error is not a sign-out");
}

#[test]
fn rate_limit_is_retried_with_a_bounded_wait() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let api = FakeApi::start(move |_, _, _| {
        if counter.fetch_add(1, Ordering::SeqCst) == 0 {
            (200, r#"{"error":{"error_code":6,"error_msg":"Too many requests per second"}}"#.into())
        } else {
            (200, page_json(&[1], 1))
        }
    });
    let (app, _) = app(&api.base, "http://127.0.0.1:9/never".into(), Arc::new(MemoryStore::default()));
    app.sign_in(session("tokentokentoken-1"));
    let started = std::time::Instant::now();
    assert_eq!(app.library(0, 10).unwrap().items.len(), 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}
