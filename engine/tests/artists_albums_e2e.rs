//! Artist and album pages against a fake VK: the same calls the iOS app
//! makes, its fallbacks when VK refuses or answers thinly, and nothing sent
//! for ids the window should never have produced.

mod common;

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Album, ArtistRef, Session, VkClient};
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

fn method(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("")
}

/// One `audio` item the way VK sends it.
fn audio(id: i64, title: &str, album: Option<(i64, i64, &str)>) -> String {
    let album = album
        .map(|(id, owner, title)| format!(r#","album":{{"id":{id},"owner_id":{owner},"title":"{title}","access_key":"ak"}}"#))
        .unwrap_or_default();
    format!(
        r#"{{"artist":"Монеточка","id":{id},"owner_id":-2001,"title":"{title}","duration":200,"access_key":"t{id}",
            "url":"https://cs1.vkuseraudio.net/s/v1/a/{id}/index.m3u8",
            "main_artists":[{{"name":"Монеточка","domain":"monetochka","id":"1430426429588470911"}}]{album}}}"#
    )
}

fn release(owner: i64) -> Album {
    Album {
        album_id: 13859017, owner_id: owner, title: "Раскраски для взрослых".into(), description: None, count: 0,
        artwork_url: None, access_key: None, artists: vec![ArtistRef { id: None, name: "Монеточка".into() }],
        year: None, is_followed: false, follow_hash: None,
    }
}

const ARTIST: &str = "1430426429588470911";

#[test]
fn artist_tracks_come_from_get_audios_by_artist() {
    let api = FakeApi::start(|url, body, _| {
        assert_eq!(method(url), "audio.getAudiosByArtist");
        assert!(body.contains(&format!("artist_id={ARTIST}")) && body.contains("count=50") && body.contains("offset=0"), "{body}");
        (200, format!(r#"{{"response":{{"count":120,"items":[{},{}]}}}}"#, audio(1, "Каждый раз", None), audio(2, "Нет монет", None)))
    });
    let page = app(&api.base).artist_tracks(ARTIST, 0, 50).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!((page.total, page.next_offset), (120, Some(2)));
    assert_eq!(page.items[0].artists[0].id.as_deref(), Some(ARTIST));
}

#[test]
fn a_refused_artist_list_falls_back_to_the_catalogue_page() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "audio.getAudiosByArtist" => (200, r#"{"error":{"error_code":100,"error_msg":"One of the parameters specified was missing or invalid"}}"#.into()),
        "catalog.getAudioArtist" => {
            assert!(body.contains("need_blocks=1"), "{body}");
            let audios: Vec<String> = (1..=5).map(|n| audio(n, &format!("Песня {n}"), None)).collect();
            (200, format!(
                r#"{{"response":{{"catalog":{{"default_section":"s1","sections":[{{"id":"s1","title":"Монеточка","blocks":[]}}]}},
                    "artists":[{{"id":"{ARTIST}","name":"Монеточка","domain":"monetochka",
                                 "photo":[{{"url":"https://sun9-1.userapi.com/a.jpg","width":600,"height":600}}]}}],
                    "audios":[{}]}}}}"#,
                audios.join(",")
            ))
        }
        other => panic!("unexpected {other}"),
    });
    let page = app(&api.base).artist_tracks(ARTIST, 3, 50).unwrap();
    assert_eq!(page.items.iter().map(|t| t.track_id).collect::<Vec<_>>(), [4, 5], "the offset applies to the catalogue list");
    assert_eq!((page.total, page.next_offset), (5, None));
}

#[test]
fn artist_albums_are_enriched_by_the_catalogue_without_being_extended() {
    let api = FakeApi::start(|url, _, _| match method(url) {
        "audio.getAlbumsByArtist" => (200, r#"{"response":{"count":2,"items":[
            {"id":11,"owner_id":-2001,"title":"Первый","count":10,"year":2017,"photo":{"photo_300":"https://sun9-1.userapi.com/1.jpg"},
             "main_artists":[{"name":"Монеточка","id":"1430426429588470911"}]},
            {"id":12,"owner_id":-2001,"title":"Второй","count":9,"year":2018,
             "main_artists":[{"name":"Монеточка","id":"1430426429588470911"}]}]}}"#.into()),
        "catalog.getAudioArtist" => (200, r#"{"response":{
            "artists":[{"id":"1430426429588470911","name":"Монеточка","photo":[{"url":"https://sun9-1.userapi.com/a.jpg","width":300}]}],
            "playlists":[
              {"id":12,"owner_id":-2001,"title":"Второй","access_key":"key12","year":2018,"main_artists":[{"name":"Монеточка"}]},
              {"id":13,"owner_id":-2001,"title":"Третий","access_key":"key13","count":5,"year":2020,"main_artists":[{"name":"Монеточка"}]}]}}"#.into()),
        other => panic!("unexpected {other}"),
    });
    let result = app(&api.base).artist_albums(ARTIST, 0, 30).unwrap();
    let ids: Vec<_> = result.albums.items.iter().map(|a| a.album_id).collect();
    assert_eq!(ids, [11, 12], "the list already has the offset applied; the catalogue only fills in");
    assert_eq!(result.albums.items[1].access_key.as_deref(), Some("key12"));
    let artist = result.artist.unwrap();
    assert_eq!(artist.photo_url.as_deref(), Some("https://sun9-1.userapi.com/a.jpg"));
}

#[test]
fn a_list_of_bare_stubs_is_replaced_by_the_catalogue() {
    let api = FakeApi::start(|url, _, _| match method(url) {
        "audio.getAlbumsByArtist" => (200, r#"{"response":{"count":1,"items":[{"id":11,"owner_id":-2001,"title":"Stub"}]}}"#.into()),
        "catalog.getAudioArtist" => (200, r#"{"response":{"playlists":[
              {"id":13,"owner_id":-2001,"title":"Третий","access_key":"key13","count":5,"year":2020,"main_artists":[{"name":"Монеточка"}]},
              {"artist":"Монеточка","id":99,"owner_id":-2001,"title":"Audio row","duration":180,"main_artists":[]}]}}"#.into()),
        other => panic!("unexpected {other}"),
    });
    let result = app(&api.base).artist_albums(ARTIST, 0, 30).unwrap();
    assert_eq!(result.albums.items.iter().map(|a| a.album_id).collect::<Vec<_>>(), [13], "audio rows are never albums");
    assert!(result.artist.is_none());
}

#[test]
fn a_community_album_without_a_key_is_resolved_through_search_then_opened() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "audio.searchAlbums" => {
            assert!(body.contains("count=30"), "{body}");
            (200, r#"{"response":{"count":2,"items":[
                {"id":500,"owner_id":-9,"title":"Раскраски для взрослых","access_key":"other","main_artists":[{"name":"Кто-то"}]},
                {"id":13859017,"owner_id":-2000859017,"title":"Раскраски для взрослых","access_key":"found","main_artists":[{"name":"Монеточка"}]}]}}"#.into())
        }
        "execute.getPlaylist" => {
            assert!(body.contains("access_key=found") && body.contains("id=13859017") && body.contains("need_playlist=1"), "{body}");
            assert!(body.contains("audio_offset=0") && body.contains("audio_count=2"), "{body}");
            (200, format!(
                r#"{{"response":{{"playlist":{{"id":13859017,"owner_id":-2000859017,"title":"Раскраски для взрослых","count":13,
                    "is_following":true,"year":2018,"photo":{{"photo_600":"https://sun9-1.userapi.com/c.jpg"}},"follow_hash":"fh"}},
                    "audios":[{},{}]}}}}"#,
                audio(1, "Каждый раз", None), audio(2, "Нет монет", None)
            ))
        }
        other => panic!("unexpected {other}"),
    });
    let found = app(&api.base).album_tracks(&release(-2000859017), 0, 2).unwrap();
    assert_eq!(found.tracks.items.len(), 2);
    assert_eq!((found.tracks.total, found.tracks.next_offset), (13, Some(2)));
    assert_eq!(found.album.access_key.as_deref(), Some("found"));
    assert!(found.album.is_followed);
    assert_eq!(found.album.year, Some(2018));
    assert_eq!(found.album.follow_hash.as_deref(), Some("fh"));
    assert_eq!(found.album.artwork_url.as_deref(), Some("https://sun9-1.userapi.com/c.jpg"));
}

#[test]
fn denied_access_looks_the_key_up_again() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "execute.getPlaylist" if body.contains("access_key=stale") => {
            (200, r#"{"error":{"error_code":15,"error_msg":"Access denied: access to users audio is denied"}}"#.into())
        }
        "execute.getPlaylist" => {
            assert!(body.contains("access_key=fresh"), "{body}");
            (200, format!(r#"{{"response":{{"audios":[{}]}}}}"#, audio(1, "Каждый раз", None)))
        }
        "audio.getPlaylistById" => (200, r#"{"error":{"error_code":15,"error_msg":"Access denied"}}"#.into()),
        "audio.searchAlbums" => (200, r#"{"response":{"count":1,"items":[
            {"id":13859017,"owner_id":-2000859017,"title":"Раскраски для взрослых","access_key":"fresh"}]}}"#.into()),
        other => panic!("unexpected {other}"),
    });
    let album = Album { access_key: Some("stale".into()), ..release(-2000859017) };
    let found = app(&api.base).album_tracks(&album, 0, 100).unwrap();
    assert_eq!(found.tracks.items[0].title, "Каждый раз");
    assert_eq!(found.tracks.next_offset, None);
}

#[test]
fn when_every_playlist_method_refuses_the_album_is_rebuilt_from_search() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "execute.getPlaylist" | "audio.getPlaylistById" => (200, r#"{"error":{"error_code":201,"error_msg":"Access denied"}}"#.into()),
        "audio.searchAlbums" => (200, r#"{"response":{"count":0,"items":[]}}"#.into()),
        "audio.search" => {
            assert!(body.contains("count=100"), "{body}");
            (200, format!(
                r#"{{"response":{{"count":3,"items":[{},{},{}]}}}}"#,
                audio(1, "Каждый раз", Some((13859017, -2000859017, "Раскраски для взрослых"))),
                audio(2, "Чужая", None).replace(r#""artist":"Монеточка""#, r#""artist":"Другой""#),
                audio(3, "Нимфоманка", Some((13859017, -2000859017, "Раскраски для взрослых")))
            ))
        }
        "audio.searchArtists" | "audio.getAudiosByArtist" => (200, r#"{"response":{"count":0,"items":[]}}"#.into()),
        "catalog.getAudioArtist" => (200, r#"{"response":{}}"#.into()),
        other => panic!("unexpected {other}"),
    });
    let album = Album { access_key: Some("k".into()), ..release(-2000859017) };
    let found = app(&api.base).album_tracks(&album, 0, 100).unwrap();
    assert_eq!(found.tracks.items.iter().map(|t| t.track_id).collect::<Vec<_>>(), [1, 3]);
}

#[test]
fn an_album_nothing_can_open_says_so_in_one_error() {
    let api = FakeApi::start(|url, _, _| match method(url) {
        "execute.getPlaylist" | "audio.getPlaylistById" => (200, r#"{"error":{"error_code":15,"error_msg":"Access denied"}}"#.into()),
        "audio.searchAlbums" | "audio.search" | "audio.searchArtists" => (200, r#"{"response":{"count":0,"items":[]}}"#.into()),
        other => panic!("unexpected {other}"),
    });
    let album = Album { access_key: Some("k".into()), ..release(-2000859017) };
    match app(&api.base).album_tracks(&album, 0, 100) {
        Err(Error::Vk { code: 15, message }) => assert!(message.contains("album"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn sessions_without_execute_use_audio_get() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "execute.getPlaylist" => (200, r#"{"error":{"error_code":3,"error_msg":"Unknown method passed"}}"#.into()),
        "audio.get" => {
            assert!(body.contains("album_id=5") && body.contains("owner_id=7") && !body.contains("access_key"), "{body}");
            (200, format!(r#"{{"response":{{"count":1,"items":[{}]}}}}"#, audio(1, "Своя", None)))
        }
        other => panic!("unexpected {other}"),
    });
    // A person's playlist needs no key, so nothing is resolved first.
    let album = Album { album_id: 5, owner_id: 7, ..release(7) };
    let found = app(&api.base).album_tracks(&album, 0, 100).unwrap();
    assert_eq!(found.tracks.items.len(), 1);
}

#[test]
fn a_lost_connection_is_not_papered_over_by_fallbacks() {
    let cut = CutServer::start();
    let album = Album { access_key: Some("k".into()), ..release(-2000859017) };
    let error = app(&cut.base).album_tracks(&album, 0, 100).unwrap_err();
    assert!(pm_engine::net::is_connectivity(&error), "{error:?}");
}

#[test]
fn followed_albums_ask_for_releases_only_and_keep_paging_on_full_pages() {
    let api = FakeApi::start(|url, body, _| {
        assert_eq!(method(url), "audio.getPlaylists");
        assert!(body.contains("filters=albums") && body.contains("owner_id=7") && body.contains("count=2"), "{body}");
        (200, r#"{"response":{"count":1,"items":[
            {"id":11,"owner_id":-2001,"title":"Альбом","album_type":"main_only","year":2017,"main_artists":[{"name":"A","id":"1"}],"is_following":true},
            {"id":40,"owner_id":7,"title":"Сохранённый плейлист","original":{"owner_id":8,"playlist_id":3}}]}}"#.into())
    });
    let page = app(&api.base).followed_albums(0, 2).unwrap();
    assert_eq!(page.items.iter().map(|a| a.album_id).collect::<Vec<_>>(), [11]);
    assert_eq!(page.next_offset, Some(2), "a full page continues even when count says otherwise");
}

#[test]
fn playlists_leave_out_only_clear_cut_followed_releases() {
    let api = FakeApi::start(|_, _, _| {
        (200, r#"{"response":{"count":3,"items":[
            {"id":1,"owner_id":-2001,"title":"Релиз","album_type":"album","type":1,"year":2019,"main_artists":[{"name":"A"}]},
            {"id":2,"owner_id":7,"title":"С артистом","main_artists":[{"name":"A"}],"year":2019},
            {"id":3,"owner_id":7,"title":"Обычный"}]}}"#.into())
    });
    let page = app(&api.base).playlists(0, 100).unwrap();
    assert_eq!(page.items.iter().map(|p| p.playlist_id).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(page.next_offset, None);
}

#[test]
fn follow_and_unfollow_send_the_locator_once() {
    let api = FakeApi::start(|url, body, _| {
        assert!(body.contains("owner_id=-2000859017") && body.contains("playlist_id=13859017"), "{body}");
        assert!(body.contains("hash=fh") && body.contains("access_key=k"), "{body}");
        match method(url) {
            "audio.followPlaylist" => (200, r#"{"response":{"owner_id":7,"playlist_id":77}}"#.into()),
            "audio.deletePlaylist" => (503, "busy".into()),
            other => panic!("unexpected {other}"),
        }
    });
    let app = app(&api.base);
    let album = Album { access_key: Some("k".into()), follow_hash: Some("fh".into()), ..release(-2000859017) };
    app.follow_album(&album, true).unwrap();
    assert!(app.follow_album(&album, false).is_err());
    assert_eq!(api.call_count("deletePlaylist"), 1, "a change is never retried");
}

#[test]
fn search_finds_artists_and_albums_and_matches_names() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "audio.searchArtists" => {
            assert!(body.contains("q=") && body.contains("count=8"), "{body}");
            (200, r#"{"response":{"count":3,"items":[
                {"name":"Монеточка и Ко","id":"2","domain":"x"},
                {"name":"Монеточка","id":"1430426429588470911","domain":"monetochka","photo":[{"url":"https://sun9-1.userapi.com/p.jpg","width":135}]},
                {"name":"Монеточка","id":"1430426429588470911"}]}}"#.into())
        }
        "audio.searchAlbums" => (200, r#"{"response":{"count":40,"items":[
            {"id":1,"owner_id":-2,"title":"Раскраски","access_key":"k","main_artists":[{"name":"Монеточка","id":"1430426429588470911"}]},
            {"oops":"ad"}]}}"#.into()),
        other => panic!("unexpected {other}"),
    });
    let app = app(&api.base);
    let artists = app.search_artists("монеточка", 0, 8).unwrap();
    assert_eq!(artists.len(), 2, "the same artist twice is one");
    let found = app.find_artist("Монёточка").unwrap().unwrap();
    assert_eq!(found.id, ARTIST);
    let albums = app.search_albums("раскраски", 0, 2).unwrap();
    assert_eq!(albums.items.len(), 1);
    assert_eq!(albums.next_offset, Some(2), "paging advances over the ad entry too");
    assert!(app.search_albums("   ", 0, 20).unwrap().items.is_empty());
}

#[test]
fn nonsense_from_the_window_never_reaches_vk() {
    let api = FakeApi::start(|_, _, _| panic!("no request expected"));
    let app = app(&api.base);
    let long = "9".repeat(200);
    for id in ["", "../../method/x", "a&b=c", long.as_str()] {
        assert_eq!(app.artist_tracks(id, 0, 50).unwrap_err(), Error::InvalidRequest, "{id}");
        assert_eq!(app.artist_albums(id, 0, 50).unwrap_err(), Error::InvalidRequest, "{id}");
    }
    assert_eq!(app.artist_tracks(ARTIST, 5_000_000, 50).unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.search_artists(&"q".repeat(501), 0, 10).unwrap_err(), Error::InvalidRequest);
    for bad in [
        Album { album_id: 0, ..release(-2) },
        Album { owner_id: 0, ..release(-2) },
        Album { access_key: Some("k&owner_id=1".into()), ..release(-2) },
        Album { follow_hash: Some("h h".into()), ..release(-2) },
    ] {
        assert_eq!(app.album_tracks(&bad, 0, 100).unwrap_err(), Error::InvalidRequest);
        assert_eq!(app.follow_album(&bad, true).unwrap_err(), Error::InvalidRequest);
    }
    assert_eq!(app.album_tracks(&release(-2), 100_001, 100).unwrap_err(), Error::InvalidRequest);
}
