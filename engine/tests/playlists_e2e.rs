//! Редактирование собственных плейлистов пользователя: правильные вызовы, по одной попытке и ничего не
//! отправляется для чужих плейлистов.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Playlist, Session, Track, VkClient};
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

fn mine(id: i64) -> Playlist {
    Playlist { playlist_id: id, owner_id: 7, title: "Old".into(), description: None, count: 3, artwork_url: None, access_key: None }
}

fn track() -> Track {
    Track {
        track_id: 456, owner_id: -100, title: "T".into(), artist: "A".into(), album_title: None, duration: 180,
        stream_url: None, artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None,
        artists: Vec::new(), album: None,
    }
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

#[test]
fn creating_returns_the_new_playlist_with_what_vk_left_out() {
    let (api, seen) = recording(r#"{"response":{"id":55,"owner_id":7}}"#);
    let created = app(&api.base).create_playlist("  Дорога  ", "в машину").unwrap();
    assert_eq!((created.playlist_id, created.owner_id, created.title.as_str()), (55, 7, "Дорога"));
    assert_eq!(created.description.as_deref(), Some("в машину"));
    let call = seen.lock().unwrap()[0].clone();
    assert!(call.contains("audio.createPlaylist") && call.contains("owner_id=7"), "{call}");
}

#[test]
fn rename_delete_add_remove_send_the_right_ids() {
    let (api, seen) = recording(r#"{"response":1}"#);
    let app = app(&api.base);
    let renamed = app.edit_playlist(&mine(5), "Новое", "").unwrap();
    assert_eq!((renamed.title.as_str(), renamed.description.clone()), ("Новое", None));
    app.add_to_playlist(&mine(5), &track()).unwrap();
    app.remove_from_playlist(&mine(5), &track()).unwrap();
    app.delete_playlist(&mine(5)).unwrap();
    let calls = seen.lock().unwrap().clone();
    assert!(calls[0].contains("audio.editPlaylist") && calls[0].contains("playlist_id=5"));
    assert!(calls[1].contains("audio.addToPlaylist") && calls[1].contains("audio_ids=-100_456"), "{}", calls[1]);
    assert!(calls[2].contains("audio.removeFromPlaylist"));
    assert!(calls[3].contains("audio.deletePlaylist"));
}

#[test]
fn someone_elses_playlist_and_bad_titles_never_reach_vk() {
    let api = FakeApi::start(|_, _, _| panic!("no request expected"));
    let app = app(&api.base);
    let theirs = Playlist { owner_id: 8, ..mine(5) };
    assert_eq!(app.delete_playlist(&theirs).unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.add_to_playlist(&theirs, &track()).unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.edit_playlist(&mine(5), "   ", "").unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.create_playlist(&"x".repeat(129), "").unwrap_err(), Error::InvalidRequest);
}

#[test]
fn a_failed_change_is_not_retried() {
    let api = FakeApi::start(|_, _, _| (503, "busy".into()));
    let app = app(&api.base);
    assert!(app.add_to_playlist(&mine(5), &track()).is_err());
    assert_eq!(api.call_count("addToPlaylist"), 1, "a retry could add the track twice");
}

#[test]
fn discovery_is_optional_and_sent_with_the_same_metadata_write() {
    let (api, seen) = recording(r#"{"response":{"id":55,"owner_id":7}}"#);
    let app = app(&api.base);
    for hidden in [None, Some(true), Some(false)] {
        app.create_playlist_with_discovery("Test", "Description", hidden).unwrap();
        app.edit_playlist_with_discovery(&mine(5), "Renamed", "Description", hidden).unwrap();
    }
    let calls = seen.lock().unwrap();
    assert_eq!(calls.len(), 6);
    for (index, call) in calls.iter().enumerate() {
        assert!(call.contains("title=") && call.contains("description=Description"), "{call}");
        match index / 2 {
            0 => assert!(!call.contains("no_discover"), "ordinary rename must preserve discovery"),
            1 => assert!(call.contains("no_discover=1"), "{call}"),
            _ => assert!(call.contains("no_discover=0"), "{call}"),
        }
    }
}

#[test]
fn discovery_cannot_change_another_users_playlist() {
    let api = FakeApi::start(|_, _, _| panic!("no request expected"));
    let app = app(&api.base);
    let theirs = Playlist { owner_id: 8, ..mine(5) };
    assert_eq!(app.edit_playlist_with_discovery(&theirs, "Test", "", Some(true)).unwrap_err(), Error::InvalidRequest);
}

#[test]
fn a_discovery_write_is_not_retried_after_an_uncertain_response() {
    let api = FakeApi::start(|_, _, _| (503, "busy".into()));
    let app = app(&api.base);
    assert!(app.edit_playlist_with_discovery(&mine(5), "Test", "", Some(false)).is_err());
    assert_eq!(api.call_count("editPlaylist"), 1);
}

fn cover_session() -> Session {
    Session {access_token:"test-token".into(),user_id:Some(7),expires_at:None,api_user_agent:"ua".into(),refresh_cookie:String::new(),web_user_agent:String::new()}
}
#[test]
fn cover_binding_uses_playlist_owner_and_the_opaque_upload_result() {
    let (api,seen)=recording(r#"{"response":1}"#);
    let client=VkClient::with_base(&api.base).unwrap();
    client.save_playlist_cover(&cover_session(),&mine(5),&serde_json::json!({"photo":"opaque-photo","hash":"opaque-hash","server":123}),&||false).unwrap();
    let calls=seen.lock().unwrap();assert_eq!(calls.len(),1);
    assert!(calls[0].contains("audio.setPlaylistCoverPhoto"));
    assert!(calls[0].contains("playlist_owner_id=7") && calls[0].contains("playlist_id=5"));
    assert!(calls[0].contains("photo=opaque-photo") && calls[0].contains("hash=opaque-hash"));
    assert!(!calls[0].contains("server="));
}
#[test]
fn cover_server_is_vk_https_and_ownership_is_checked_before_uploading() {
    let (api,_)=recording(r#"{"response":{"upload_url":"https://pu.userapi.com/cover"}}"#);
    let client=VkClient::with_base(&api.base).unwrap();
    assert_eq!(client.playlist_cover_server(&cover_session(),&mine(5)).unwrap().host_str(),Some("pu.userapi.com"));
    let api=FakeApi::start(|_,_,_|panic!("another user's playlist must not make a cover request"));
    let client=VkClient::with_base(&api.base).unwrap();
    assert_eq!(client.playlist_cover_server(&cover_session(),&Playlist {owner_id:8,..mine(5)}).unwrap_err(),Error::InvalidRequest);
    assert_eq!(client.save_playlist_cover(&cover_session(),&mine(5),&serde_json::json!({"photo":"p","hash":"h"}),&||true).unwrap_err(),Error::Cancelled);
}
#[test]
fn uncertain_cover_binding_is_not_automatically_replayed() {
    let api=FakeApi::start(|_,_,_|(503,"busy".into()));
    let client=VkClient::with_base(&api.base).unwrap();
    assert!(client.save_playlist_cover(&cover_session(),&mine(5),&serde_json::json!({"photo":"p","hash":"h"}),&||false).is_err());
    assert_eq!(api.call_count("setPlaylistCoverPhoto"),1);
}
#[test]
fn cancellation_during_owner_lookup_prevents_the_cover_binding() {
    use std::sync::atomic::{AtomicBool,Ordering};
    let stopped=Arc::new(AtomicBool::new(false));let flag=stopped.clone();
    let api=FakeApi::start(move |url,_,_|{
        assert!(url.ends_with("users.get"));flag.store(true,Ordering::Release);
        (200,r#"{"response":[{"id":7,"first_name":"A","last_name":"B"}]}"#.into())
    });
    let client=VkClient::with_base(&api.base).unwrap();let session=Session {user_id:None,..cover_session()};
    assert_eq!(client.save_playlist_cover(&session,&mine(5),&serde_json::json!({"photo":"p","hash":"h"}),&||stopped.load(Ordering::Acquire)).unwrap_err(),Error::Cancelled);
    assert_eq!(api.call_count("users.get"),1);assert_eq!(api.call_count("setPlaylistCoverPhoto"),0);
}
