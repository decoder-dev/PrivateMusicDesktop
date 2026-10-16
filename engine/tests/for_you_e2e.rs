//! "For you": the personal mix, recommendations and radio from a track.

mod common;

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, Track, VkClient};

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

fn audio(id: i64) -> String {
    format!(r#"{{"id":{id},"owner_id":-5,"artist":"A{id}","title":"T{id}","duration":200,"url":"https://cdn.example/{id}.m3u8"}}"#)
}

#[test]
fn the_mix_collects_tracks_wherever_vk_nests_them_without_duplicates() {
    let api = FakeApi::start(|url, body, _| {
        assert!(url.ends_with("audio.getStreamMixAudios"));
        assert!(body.contains("mix_id=common"), "{body}");
        if body.contains("offset=0") {
            (200, format!(r#"{{"response":{{"items":[{},{}],"blocks":[{{"audios":[{}]}}]}}}}"#, audio(1), audio(2), audio(1)))
        } else {
            (200, format!(r#"{{"response":[{},{}]}}"#, audio(2), audio(3)))
        }
    });
    let tracks = app(&api.base).mix().unwrap();
    let ids: Vec<i64> = tracks.iter().map(|t| t.track_id).collect();
    assert_eq!(ids, vec![1, 2, 3]);
    assert_eq!(api.call_count("getStreamMixAudios"), 2);
}

#[test]
fn a_broken_second_mix_page_still_plays_the_first() {
    let api = FakeApi::start(|_, body, _| {
        if body.contains("offset=0") { (200, format!(r#"{{"response":[{}]}}"#, audio(1))) } else { (500, "x".into()) }
    });
    assert_eq!(app(&api.base).mix().unwrap().len(), 1);
}

#[test]
fn similar_tracks_are_seeded_and_never_repeat_the_seed() {
    let api = FakeApi::start(|url, body, _| {
        assert!(url.ends_with("audio.getRecommendations"));
        if body.contains("target_audio") {
            assert!(body.contains("target_audio=-5_9"), "{body}");
            assert!(body.contains("user_id=7"), "{body}");
            (200, format!(r#"{{"response":{{"count":3,"items":[{},{},{}]}}}}"#, audio(9), audio(4), audio(4)))
        } else {
            (200, format!(r#"{{"response":{{"count":1,"items":[{}]}}}}"#, audio(8)))
        }
    });
    let app = app(&api.base);
    let seed = Track {
        track_id: 9, owner_id: -5, title: "T9".into(), artist: "A9".into(), album_title: None, duration: 200,
        stream_url: None, artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None,
        artists: Vec::new(), album: None,
    };
    let similar = app.similar(&seed).unwrap();
    assert_eq!(similar.iter().map(|t| t.track_id).collect::<Vec<_>>(), vec![4]);
    assert_eq!(app.recommendations().unwrap()[0].track_id, 8);
}
