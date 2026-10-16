//! Friends' music: the right VK calls, VK-shaped answers parsed into friends,
//! tracks and playlists, and what the engine refuses before asking VK.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, VkClient};
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

fn recording(answer: &'static str) -> (FakeApi, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let api = FakeApi::start(move |url, body, _| {
        log.lock().unwrap().push(format!("{url} {body}"));
        (200, answer.to_string())
    });
    (api, seen)
}

// Shape of a real `friends.get` answer with `fields=photo_200`.
const FRIENDS: &str = r#"{"response":{"count":4,"items":[
    {"id":11,"first_name":"Анна","last_name":"Смирнова","photo_200":"https://sun9-1.userapi.com/a.jpg","can_access_closed":true,"is_closed":false},
    {"id":12,"first_name":"DELETED","last_name":"","deactivated":"deleted"},
    {"id":13,"first_name":"Борис","last_name":"","can_access_closed":false,"is_closed":true},
    {"id":14,"first_name":"Ёлка","last_name":"Иванова","photo_200":"javascript:alert(1)"}
]}}"#;

#[test]
fn friends_are_listed_by_name_with_photos_and_without_deleted_accounts() {
    let (api, seen) = recording(FRIENDS);
    let page = app(&api.base).friends(0, 100).unwrap();
    let names: Vec<String> = page.items.iter().map(|p| p.display_name()).collect();
    assert_eq!(names, ["Анна Смирнова", "Борис", "Ёлка Иванова"]);
    assert_eq!(page.items[0].photo_url.as_deref(), Some("https://sun9-1.userapi.com/a.jpg"));
    assert_eq!(page.items[2].photo_url, None, "a photo that is not http(s) is dropped");
    assert_eq!((page.total, page.next_offset), (4, None), "4 raw entries cover the total of 4");
    let call = seen.lock().unwrap()[0].clone();
    assert!(call.contains("/method/friends.get") && call.contains("fields=photo_200") && call.contains("order=name"), "{call}");
    assert!(call.contains("offset=0") && call.contains("count=100"), "{call}");
}

#[test]
fn a_long_friend_list_pages_by_the_raw_count() {
    let api = FakeApi::start(|_, body, _| {
        let first = body.contains("offset=0");
        let items = if first {
            r#"{"id":1,"first_name":"А","last_name":"А"},{"id":2,"first_name":"B","last_name":"B","deactivated":"banned"}"#
        } else {
            r#"{"id":3,"first_name":"В","last_name":"В"}"#
        };
        (200, format!(r#"{{"response":{{"count":3,"items":[{items}]}}}}"#))
    });
    let app = app(&api.base);
    let first = app.friends(0, 2).unwrap();
    assert_eq!((first.items.len(), first.next_offset), (1, Some(2)), "the banned account still used a slot");
    let second = app.friends(2, 2).unwrap();
    assert_eq!((second.items.len(), second.next_offset), (1, None));
}

// Shape of an `audio.get` answer for another user's page.
const TRACKS: &str = r#"{"response":{"count":2,"items":[
    {"artist":"Кино","id":456239017,"owner_id":11,"title":"Группа крови","duration":284,"access_key":"abc123","is_explicit":false,"url":"https://cs1.vkuseraudio.net/s/v1/ac/x/index.m3u8","track_code":"tc1"},
    {"artist":"Нет ссылки","id":2,"owner_id":11,"title":"Без URL","duration":10}
]}}"#;

#[test]
fn a_friends_tracks_use_their_id_and_keep_the_keys_needed_to_play_them() {
    let (api, seen) = recording(TRACKS);
    let page = app(&api.base).friend_tracks(11, 0, 100).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!((page.items[0].owner_id, page.items[0].track_id), (11, 456239017));
    assert_eq!(page.items[0].access_key.as_deref(), Some("abc123"));
    assert!(page.items[0].stream_url.is_some() && page.items[1].stream_url.is_none());
    let call = seen.lock().unwrap()[0].clone();
    assert!(call.contains("/method/audio.get") && call.contains("owner_id=11"), "{call}");
    assert!(!call.contains("owner_id=7"), "never the user's own list: {call}");
}

#[test]
fn private_audio_is_a_vk_error_the_window_can_explain() {
    let api = FakeApi::start(|_, _, _| {
        (200, r#"{"error":{"error_code":201,"error_msg":"Access denied: user hid his audio"}}"#.into())
    });
    let error = app(&api.base).friend_tracks(11, 0, 100).unwrap_err();
    assert!(matches!(error, Error::Vk { code: 201, .. }), "{error:?}");
    assert_eq!(error.kind(), "vk");
    assert_eq!(api.call_count("audio.get"), 1, "an answer is final, not retried");
    // A friend with nothing public is an empty page, not an error.
    let empty = FakeApi::start(|_, _, _| (200, r#"{"response":{"count":0,"items":[]}}"#.into()));
    let page = app(&empty.base).friend_tracks(11, 0, 100).unwrap();
    assert!(page.items.is_empty() && page.next_offset.is_none());
}

// Shape of an `audio.getPlaylists` answer: a plain playlist and a followed release.
const PLAYLISTS: &str = r#"{"response":{"count":2,"items":[
    {"id":5,"owner_id":11,"title":"Дорога","description":"в машину","count":3,"access_key":"pk","type":0,
     "photo":{"photo_300":"https://sun9-2.userapi.com/p.jpg"}},
    {"id":9,"owner_id":-2000,"title":"Альбом","count":10,"type":1,"album_type":"main_only","year":2010,
     "main_artists":[{"name":"Кино","id":"1"}]}
]}}"#;

#[test]
fn a_friends_playlists_open_with_their_own_locator() {
    let (api, seen) = recording(PLAYLISTS);
    let page = app(&api.base).friend_playlists(11, 0, 100).unwrap();
    assert_eq!(page.items.len(), 1, "followed releases belong on the albums shelf");
    let playlist = &page.items[0];
    assert_eq!((playlist.owner_id, playlist.playlist_id, playlist.title.as_str()), (11, 5, "Дорога"));
    assert_eq!(playlist.access_key.as_deref(), Some("pk"));
    let call = seen.lock().unwrap()[0].clone();
    assert!(call.contains("/method/audio.getPlaylists") && call.contains("owner_id=11"), "{call}");

    // Opening it asks for that owner's list with the playlist's key.
    let (api, seen) = recording(TRACKS);
    app(&api.base).playlist_tracks(playlist, 0, 100).unwrap();
    let call = seen.lock().unwrap()[0].clone();
    assert!(call.contains("owner_id=11") && call.contains("album_id=5") && call.contains("access_key=pk"), "{call}");
}

#[test]
fn bad_owners_and_offsets_never_reach_vk() {
    let api = FakeApi::start(|_, _, _| panic!("no request expected"));
    let app = app(&api.base);
    for owner in [0, -1, -2_000_000_000, i64::MIN] {
        assert_eq!(app.friend_tracks(owner, 0, 10).unwrap_err(), Error::InvalidRequest, "{owner}");
        assert_eq!(app.friend_playlists(owner, 0, 10).unwrap_err(), Error::InvalidRequest, "{owner}");
    }
    assert_eq!(app.friends(u32::MAX, 10).unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.friend_tracks(11, u32::MAX, 10).unwrap_err(), Error::InvalidRequest);
}

#[test]
fn page_sizes_are_clamped() {
    let (api, seen) = recording(r#"{"response":{"count":0,"items":[]}}"#);
    let app = app(&api.base);
    app.friends(0, 100_000).unwrap();
    app.friend_tracks(11, 0, 0).unwrap();
    let calls = seen.lock().unwrap().clone();
    assert!(calls[0].contains("count=200"), "{}", calls[0]);
    assert!(calls[1].contains("count=1"), "{}", calls[1]);
}
