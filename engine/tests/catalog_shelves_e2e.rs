//! "For you" противопоставляется поддельному VK с полезными нагрузками, имеющими форму тех, которые
//! несут собственные тесты клиента iOS (`MixTuningTests`, `AlbumSupportTests`, `VKCatalogArtistTests`):
//! миксы под `blocks[].audio_stream_mixes`, список разделов под `catalog.sections`, выпуск альбомов с
//! сеткой обложки и тонкие корневые ответы, которые заполняются только разделами.

mod common;

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, VkClient};

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

fn param<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    body.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

fn ids(mixes: &[pm_engine::vk::MusicMix]) -> Vec<String> {
    mixes.iter().map(|m| format!("{}{}", m.id, m.options.as_deref().map(|o| format!("#{o}")).unwrap_or_default())).collect()
}

/// iOS `MixTuningTests`: настроенные плитки имеют одинаковый идентификатор `common`.
const TUNED: &str = r#"{"blocks":[{"audio_stream_mixes":[
    {"mix_id":"common","title":"Грустное","options":{"mood":"sad"}},
    {"mix_id":"common","title":"Бодрое","options":{"mood":"energetic"}},
    {"mix_id":"common","title":"Мой микс"}]}]}"#;

#[test]
fn the_personal_mix_leads_even_when_vk_lists_nothing() {
    let api = FakeApi::start(|_, _, _| (200, r#"{"response":{"catalog":{"sections":[]},"audios":[],"playlists":[]}}"#.into()));
    let mixes = app(&api.base).catalog_mixes().unwrap();
    assert_eq!(ids(&mixes), ["common"]);
    assert_eq!(mixes[0].title, "Мой микс");
    assert!(!mixes[0].subtitle.is_empty());
}

#[test]
fn the_personal_mix_survives_a_refused_catalogue() {
    let api = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":100,"error_msg":"bad"}}"#.into()));
    assert_eq!(ids(&app(&api.base).catalog_mixes().unwrap()), ["common"]);
}

#[test]
fn tuned_tiles_keep_their_options_and_the_plain_common_tile_is_first_and_renamed() {
    let api = FakeApi::start(|_, _, _| (200, format!(r#"{{"response":{TUNED}}}"#)));
    let mixes = app(&api.base).catalog_mixes().unwrap();
    assert_eq!(ids(&mixes), ["common", r#"common#{"mood":"sad"}"#, r#"common#{"mood":"energetic"}"#]);
    assert_eq!(mixes[0].title, "Мой микс");
    assert_eq!(mixes[1].title, "Грустное");
}

#[test]
fn a_tile_without_a_title_still_counts_and_covers_are_found_in_every_shape() {
    let api = FakeApi::start(|_, _, _| {
        (200, r#"{"response":{"blocks":[
            {"data_type":"music_audio_stream_mixes","audio_stream_mixes":[
                {"mix_id":"daily"},
                {"id":"x1","data_type":"stream_mix","title":"С обложкой","cover":{"photo_600":"https://cdn.example/c600.jpg"}},
                {"mix_id":"x2","title":"Список","image":[{"width":50,"url":"https://cdn.example/i50.jpg"}]},
                {"mix_id":"x3","title":"Вложенная","cover":{"sizes":[{"url":"//cdn.example/deep.jpg"}]}},
                {"mix_id":"x4","title":"Миниатюры","thumbs":[{"photo_300":"http://cdn.example/t.jpg"}],"owner":{"photo_200":"https://cdn.example/avatar.jpg"}}]}]}}"#.into())
    });
    let mixes = app(&api.base).catalog_mixes().unwrap();
    assert_eq!(ids(&mixes), ["common", "daily", "x1", "x2", "x3", "x4"]);
    assert!(!mixes[1].title.is_empty());
    let cover = |i: usize| mixes[i].artwork_url.as_deref();
    assert_eq!(cover(1), None);
    assert_eq!(cover(2), Some("https://cdn.example/c600.jpg"));
    assert_eq!(cover(3), Some("https://cdn.example/i50.jpg"));
    assert_eq!(cover(4), Some("https://cdn.example/deep.jpg"));
    assert_eq!(cover(5), Some("https://cdn.example/t.jpg"), "never the owner's avatar");
}

#[test]
fn a_thin_root_is_filled_in_from_the_sections_that_look_like_mix_shelves() {
    // Список разделов iOS `VKCatalogArtistTests`, а также полки, не являющиеся миксами.
    let api = FakeApi::start(|url, body, _| match method(url) {
        "catalog.getAudio" => (200, r#"{"response":{"catalog":{"default_section":"sec-home","sections":[
                {"id":"sec-mix","title":"mixes","url":"https://vk.com/audios1?section=mixes"},
                {"id":"sec-new","title":"new_releases","url":"https://vk.com/audios1?section=new"},
                {"id":"sec-pod","title":"Подкасты","url":"https://vk.com/audios1?section=pod"},
                {"id":"456254091","title":"Для вас","url":"https://vk.com/x"}]},
            "audios":[],"playlists":[]}}"#.into()),
        "catalog.getSection" => {
            assert!(!body.contains("need_blocks"), "the iOS client sends the section id alone: {body}");
            match param(body, "section_id") {
                Some("sec-mix") => (200, r#"{"response":{"section":{"id":"sec-mix","blocks":[{"audio_stream_mixes":[
                    {"mix_id":"calm","title":"Спокойный","subtitle":"Для вечера"},{"mix_id":"common","title":"VK Микс"}]}]}}}"#.into()),
                Some("sec-new") => (200, r#"{"response":{"blocks":[{"audio_stream_mixes":[{"mix_id":"fresh","title":"Свежее"}]}]}}"#.into()),
                other => panic!("unexpected section {other:?}"),
            }
        }
        other => panic!("unexpected {other}"),
    });
    let mixes = app(&api.base).catalog_mixes().unwrap();
    // "new_releases" также соответствует маркерам широкого микса ("нов" предназначен только для
    // выпусков, но "mixes" и URL-адрес выполняют свою работу); числовой идентификатор никогда не
    // запрашивается.
    assert!(ids(&mixes).starts_with(&["common".to_string(), "calm".to_string()]), "{:?}", ids(&mixes));
    assert_eq!(mixes[0].title, "Мой микс", "VK's own wording for it is replaced");
    assert_eq!(mixes[1].section_title.as_deref(), Some("mixes"));
    assert_eq!(mixes[1].subtitle, "Для вечера");
    assert_eq!(mixes.iter().filter(|m| m.id == "common").count(), 1);
    assert_eq!(api.call_count("catalog.getSection"), 1, "only the mix-looking section with a real id is hydrated");
}

#[test]
fn root_mixes_are_topped_up_by_the_sections_and_never_repeated() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "catalog.getAudio" => (200, r#"{"response":{"sections":[{"id":"opaque_mix_section","title":"Миксы","url":"/audios","blocks":[]}],
                "blocks":[{"audio_stream_mixes":[{"mix_id":"daily","title":"Микс дня"},{"mix_id":"x1","title":"Подборка"}]}]}}"#.into()),
        "catalog.getSection" => {
            assert_eq!(param(body, "section_id"), Some("opaque_mix_section"));
            (200, r#"{"response":{"blocks":[{"audio_stream_mixes":[
                {"mix_id":"daily","title":"Микс дня","cover":{"photo_300":"https://cdn.example/d.jpg"}},{"mix_id":"x9","title":"Новый"}]}]}}"#.into())
        }
        other => panic!("unexpected {other}"),
    });
    let mixes = app(&api.base).catalog_mixes().unwrap();
    assert_eq!(ids(&mixes), ["common", "daily", "x1", "x9"], "even a root with two mixes asks the sections");
    assert_eq!(mixes[1].artwork_url.as_deref(), Some("https://cdn.example/d.jpg"), "the repeat fills what the first lacked");
    assert_eq!(mixes[3].section_title.as_deref(), Some("Миксы"));
}

#[test]
fn a_failing_section_loses_only_itself_and_a_dead_session_is_reported() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "catalog.getAudio" => (200, r#"{"response":{"sections":[
            {"id":"good_mix","title":"Миксы","blocks":[]},{"id":"bad_mix","title":"Настроение","blocks":[]}]}}"#.into()),
        _ => match param(body, "section_id") {
            Some("good_mix") => (200, r#"{"response":{"audio_stream_mixes":[{"mix_id":"ok","title":"Живой"}]}}"#.into()),
            _ => (200, r#"{"error":{"error_code":104,"error_msg":"Not found"}}"#.into()),
        },
    });
    assert_eq!(ids(&app(&api.base).catalog_mixes().unwrap()), ["common", "ok"]);
}

/// Релиз под названием `playlists` содержит реальный ответ, покрытый сеткой (iOS `AlbumSupportTests`).
fn release(id: i64, title: &str) -> String {
    format!(
        r#"{{"id":{id},"owner_id":-8,"title":"{title}","year":2024,"main_artists":[{{"name":"Decoder"}}],"photo":[],
            "grid":{{"items":[{{"width":270,"url":""}},{{"width":600,"height":600,"url":"https://cdn.example/{id}-600.jpg"}}]}}}}"#
    )
}

#[test]
fn releases_are_found_in_blocks_items_and_in_the_flat_lists_next_to_them() {
    let api = FakeApi::start(|_, _, _| {
        (200, format!(
            r#"{{"response":{{"blocks":[{{"items":[{}]}}],
                "playlists":[{},{{"id":3,"owner_id":5,"title":"Плейлист","count":4}}],
                "audios":[{{"id":9,"owner_id":-8,"title":"Трек","duration":99,"year":2024,"main_artists":[{{"name":"x"}}]}}]}}}}"#,
            release(44, "Mosaic"), release(45, "Другой")))
    });
    let releases = app(&api.base).new_releases().unwrap();
    assert_eq!(releases.iter().map(|a| a.album_id).collect::<Vec<_>>(), [44, 45]);
    assert_eq!(releases[0].artwork_url.as_deref(), Some("https://cdn.example/44-600.jpg"));
}

#[test]
fn a_thin_root_gets_its_releases_from_sections_named_in_any_of_the_usual_ways() {
    let api = FakeApi::start(|url, body, _| match method(url) {
        "catalog.getAudio" => (200, r#"{"response":{"catalog":{"sections":[
                {"id":"sec-new","title":"new_releases","url":"https://vk.com/audios1?section=new"},
                {"id":"sec-alb","title":"Альбомы","url":"https://vk.com/audios1?section=albums"},
                {"id":"sec-pod","title":"Подкасты","url":"https://vk.com/audios1?section=pod"}]}}}"#.into()),
        _ => match param(body, "section_id") {
            Some("sec-new") => (200, format!(r#"{{"response":{{"section":{{"blocks":[{{"items":[{}]}}]}}}}}}"#, release(1, "А"))),
            Some("sec-alb") => (200, format!(r#"{{"response":{{"playlists":[{},{}]}}}}"#, release(1, "А"), release(2, "Б"))),
            other => panic!("unexpected section {other:?}"),
        },
    });
    let releases = app(&api.base).new_releases().unwrap();
    assert_eq!(releases.iter().map(|a| a.album_id).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn an_unreadable_root_does_not_hide_the_releases_the_sections_have() {
    // Ошибка VK в корне не является фатальной: попытка гидратации все равно производится из ничего, а
    // пустой результат — это пустая полка, а не ошибка.
    let api = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":100,"error_msg":"bad"}}"#.into()));
    assert!(app(&api.base).new_releases().unwrap().is_empty());
}
