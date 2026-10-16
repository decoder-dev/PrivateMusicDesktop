//! The app object's side of the local history: it follows the signed-in
//! account, validates what the window sends and keeps the file out of reach
//! of anyone else's data.

mod common;

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, VkClient};
use pm_engine::Error;

fn app(dir: &std::path::Path) -> App {
    App::new(
        VkClient::with_base("http://127.0.0.1:9").unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 1.0)) as Box<dyn Sink>),
        Box::new(MemoryStore::default()),
        |_| {},
        AppOptions { fetch: lab_config(), history_dir: Some(dir.to_path_buf()), ..AppOptions::default() },
    )
}

fn session(user: i64) -> Session {
    Session {
        access_token: "tok".into(), user_id: Some(user), expires_at: None, api_user_agent: "ua".into(),
        refresh_cookie: "c".into(), web_user_agent: "w".into(),
    }
}

#[test]
fn a_fresh_app_has_an_empty_history_and_empty_statistics() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.sign_in(session(7));
    assert!(app.history().is_empty());
    let view = app.listening_stats("month", None).unwrap();
    assert!(view.summary.is_empty && view.months.is_empty() && view.years.is_empty());
    assert_eq!(view.summary.hour_seconds.len(), 24);
    assert_eq!(view.summary.top_tracks.len() + view.summary.top_artists.len(), 0);
}

#[test]
fn the_window_cannot_send_odd_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.sign_in(session(7));
    for bad in ["", "../x", "7_1;rm", &"1".repeat(65), "7 1"] {
        assert_eq!(app.history_remove(bad).unwrap_err(), Error::InvalidRequest, "{bad}");
    }
    assert_eq!(app.history_remove("7_1"), Ok(false));
    assert_eq!(app.listening_stats("month", Some("2026-00")).unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.listening_stats("week", None).unwrap_err(), Error::InvalidRequest);
    assert_eq!(app.set_utc_offset(24 * 60).unwrap_err(), Error::InvalidRequest);
    assert!(app.set_utc_offset(5 * 60 + 30).is_ok());
}

#[test]
fn signing_out_stops_recording_and_clear_reset_do_not_fail_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.sign_in(session(7));
    app.sign_out();
    app.history_clear();
    app.reset_listening_stats();
    app.flush_history();
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing was written for nobody");
}
