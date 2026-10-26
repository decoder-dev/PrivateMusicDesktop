//! Optional phone integrations preserve account boundaries and never retry writes.

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


fn event()->pm_engine::vk::playback::ListeningEvent {
    pm_engine::vk::playback::ListeningEvent{e:"music_start_playback".into(),audio_id:"-100_456".into(),uuid:12,start_time:1792800000,playback_started_at:1792800000,duration:0,track_code:"code".into(),streaming_type:"online".into(),shuffle:false,repeat_mode:"off".into(),reason:"user".into(),state:"app".into()}
}
#[test]
fn broadcast_can_set_and_clear_status_without_sending_stream_urls(){
 let api=FakeApi::start(|path,body,_|{assert!(path.ends_with("audio.setBroadcast"));assert!(!body.contains("cdn.example"));(200,r#"{"response":1}"#.into())});
 let app=app(&api.base);app.vk_broadcast(7,Some(&foreign())).unwrap();app.vk_broadcast(7,None).unwrap();
 let calls=api.calls.lock().unwrap();assert!(calls[0].1.contains("audio=-100_456"));assert!(!calls[1].1.contains("audio="));
}
#[test]
fn listening_events_use_vk_wire_fields_and_do_not_retry_uncertain_writes(){
 let api=FakeApi::start(|path,body,_|{assert!(path.ends_with("stats.trackEvents"));let params:std::collections::HashMap<_,_>=url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();let values:serde_json::Value=serde_json::from_str(&params["events"]).unwrap();assert_eq!(values[0]["repeat"],"off");assert!(values[0].get("repeat_mode").is_none());(502,"unavailable".into())});
 assert_eq!(app(&api.base).vk_report_playback(7,&[event()]).unwrap_err(),Error::UnconfirmedWrite);assert_eq!(api.call_count("stats.trackEvents"),1);
}
#[test]
fn foreign_accounts_and_invalid_payloads_never_reach_vk(){
 let api=FakeApi::start(|_,_,_|panic!("no request expected"));let app=app(&api.base);
 assert_eq!(app.vk_broadcast(8,Some(&foreign())).unwrap_err(),Error::Unauthorized);
 assert_eq!(app.vk_report_playback(8,&[event()]).unwrap_err(),Error::Unauthorized);
 let mut invalid=event();invalid.audio_id="invalid/id".into();assert_eq!(app.vk_report_playback(7,&[invalid]).unwrap_err(),Error::InvalidRequest);
 assert_eq!(app.vk_report_playback(7,&vec![event();33]).unwrap_err(),Error::InvalidRequest);
}

#[test]
fn local_audio_never_reaches_vk_even_when_native_commands_are_called_directly() {
    let api = FakeApi::start(|_, _, _| panic!("local metadata must stay on device"));
    let app = app(&api.base);
    let local = Track { owner_id: 0, ..foreign() };
    assert_eq!(app.vk_broadcast(7, Some(&local)).unwrap_err(), Error::InvalidRequest);
    for invalid_id in ["0_456", "00_456", "7_0", "7_-1", "7_1_2"] {
        let mut invalid = event(); invalid.audio_id = invalid_id.into();
        assert_eq!(app.vk_report_playback(7, &[invalid]).unwrap_err(), Error::InvalidRequest);
    }
    let client = VkClient::with_base(&api.base).unwrap();
    let session = Session { access_token: "tok".into(), user_id: Some(7), expires_at: None,
        api_user_agent: "test".into(), refresh_cookie: String::new(), web_user_agent: "test".into() };
    assert_eq!(client.recommendations(&session, Some(&local)).unwrap_err(), Error::InvalidRequest);
    assert_eq!(client.shuffled_recommendations(&session, &local).unwrap_err(), Error::InvalidRequest);
    assert!(api.calls.lock().unwrap().is_empty());
}
