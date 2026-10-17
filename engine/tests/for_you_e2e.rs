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
    // Four pages at once, as the iOS app asks: a stream mix answers a page with few tracks.
    assert_eq!(api.call_count("getStreamMixAudios"), 4);
}

#[test]
fn the_mix_window_asks_pages_of_100_at_offsets_0_100_200_300_and_keeps_at_most_80() {
    let api = FakeApi::start(|_, body, _| {
        let offset: i64 = body.split('&').find_map(|p| p.strip_prefix("offset=")).and_then(|v| v.parse().ok()).unwrap();
        assert!(body.contains("count=100"), "{body}");
        let items: Vec<String> = (0..40).map(|i| audio(offset + i + 1)).collect();
        (200, format!(r#"{{"response":[{}]}}"#, items.join(",")))
    });
    let tracks = app(&api.base).mix().unwrap();
    assert_eq!(tracks.len(), 80);
    assert_eq!(tracks[0].track_id, 1);
    assert_eq!(tracks[40].track_id, 101, "pages are joined in offset order");
    let mut offsets: Vec<String> = api.calls.lock().unwrap().iter()
        .filter(|(url, _)| url.contains("getStreamMixAudios"))
        .filter_map(|(_, b)| b.split('&').find_map(|p| p.strip_prefix("offset=")).map(String::from)).collect();
    offsets.sort();
    assert_eq!(offsets, ["0", "100", "200", "300"]);
}

#[test]
fn a_track_vk_gives_no_stream_for_is_unavailable_not_a_protocol_error() {
    let api = FakeApi::start(|url, _, _| {
        assert!(url.ends_with("audio.getById"));
        (200, r#"{"response":[{"id":1,"owner_id":-5,"artist":"A","title":"T","duration":200,"url":""}]}"#.into())
    });
    let client = VkClient::with_base(&api.base).unwrap();
    let session = Session {
        access_token: "tok".into(), user_id: Some(7), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: "c".into(), web_user_agent: "w".into(),
    };
    let track = Track {
        track_id: 1, owner_id: -5, title: "T".into(), artist: "A".into(), album_title: None, duration: 200,
        stream_url: None, artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None,
        artists: Vec::new(), album: None,
    };
    let error = client.refreshed_track_once(&session, &track).unwrap_err();
    assert_eq!(error, pm_engine::Error::Unavailable);
    assert_eq!(error.kind(), "unavailable");
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
