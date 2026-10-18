//! Клиент VK и уровень приложения, когда сеть изменяется под ними: мертвые соединения в пуле, повторные
//! попытки, которые остаются ограниченными, записи, которые никогда не повторяются, сеанс VK перестает
//! принимать новый IP, детектор изменений.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::UiEvent;
use pm_engine::net::{AddressSource, Fingerprint, MonitorConfig, NetState};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Session, VkClient};
use pm_engine::Error;

/// Подсчеты подключений имеют смысл только в том случае, если никакой другой тест одновременно не
/// удаляет соединения из пула всего процесса.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

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

/// Альтернатива VK на простых сокетах с поддержкой активности, поэтому соединения можно подсчитывать и
/// отключать: считываются первые запросы `cut`, а затем соединение закрывается без ответа (что
/// коммутатор VPN делает с сокетом в пуле).
struct RawVk {
    base: String,
    accepted: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl RawVk {
    fn start(cut: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let (accepted, requests) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let cut = Arc::new(AtomicUsize::new(cut));
        let stop = Arc::new(AtomicBool::new(false));
        let (a, r, c, s) = (accepted.clone(), requests.clone(), cut.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                a.fetch_add(1, Ordering::SeqCst);
                let (r, c) = (r.clone(), c.clone());
                std::thread::spawn(move || serve(stream, &r, &c));
            }
        });
        Self { base, accepted, requests, stop }
    }
}

impl Drop for RawVk {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(mut stream: TcpStream, requests: &AtomicUsize, cut: &AtomicUsize) {
    let _ = stream.set_nonblocking(false);
    loop {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                return;
            }
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head).to_lowercase();
        let length: usize = text
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        requests.fetch_add(1, Ordering::SeqCst);
        if cut.load(Ordering::SeqCst) > 0 {
            cut.fetch_sub(1, Ordering::SeqCst);
            return; // упал: сокет закрывается без слов
        }
        let json = r#"{"response":[{"id":7,"first_name":"A","last_name":"B","photo_200":""}]}"#;
        let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{json}", json.len());
        if stream.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

#[test]
fn connections_are_pooled_and_a_network_change_makes_the_next_call_dial_again() {
    let _serial = serial();
    let vk = RawVk::start(0);
    let client = VkClient::with_base(&vk.base).unwrap();
    let s = session("tokentokentoken-1");
    client.call(&s, "users.get", &[]).unwrap();
    client.call(&s, "users.get", &[]).unwrap();
    assert_eq!(vk.accepted.load(Ordering::SeqCst), 1, "keep-alive: one connection for two calls");
    pm_engine::net::drop_connections(); // что делает детектор при изменении
    client.call(&s, "users.get", &[]).unwrap();
    assert_eq!(vk.accepted.load(Ordering::SeqCst), 2, "a fresh connection after the change");
}

#[test]
fn a_read_after_a_dead_connection_is_retried_on_a_fresh_one() {
    let _serial = serial();
    let vk = RawVk::start(1);
    let client = VkClient::with_base(&vk.base).unwrap();
    let started = Instant::now();
    let value = client.call(&session("tokentokentoken-1"), "users.get", &[]).unwrap();
    assert!(value.is_array());
    assert_eq!(vk.requests.load(Ordering::SeqCst), 2, "the cut request, then the retry");
    assert_eq!(vk.accepted.load(Ordering::SeqCst), 2, "the retry did not reuse the dead socket");
    assert!(started.elapsed() < Duration::from_secs(5), "bounded wait: {:?}", started.elapsed());
}

#[test]
fn reads_give_up_after_a_bounded_number_of_tries() {
    let _serial = serial();
    let vk = RawVk::start(100);
    let client = VkClient::with_base(&vk.base).unwrap();
    let started = Instant::now();
    let error = client.call(&session("tokentokentoken-1"), "users.get", &[]).unwrap_err();
    assert!(matches!(error, Error::Transport(_) | Error::Offline), "{error:?}");
    assert_eq!(vk.requests.load(Ordering::SeqCst), 3, "three tries in all, never more");
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
}

#[test]
fn a_write_is_never_repeated_after_a_dead_connection() {
    let _serial = serial();
    let vk = RawVk::start(1);
    let client = VkClient::with_base(&vk.base).unwrap();
    let s = session("tokentokentoken-1");
    let error = client.call_once(&s, "audio.add", &[("audio_id", "1".into()), ("owner_id", "-3".into())]).unwrap_err();
    assert!(matches!(error, Error::Transport(_) | Error::Offline), "{error:?}");
    assert_eq!(vk.requests.load(Ordering::SeqCst), 1, "a retried write could be applied twice");
    // Сбой все-таки устранили: следующий звонок начнется по новому соединению.
    client.call(&s, "users.get", &[]).unwrap();
    assert_eq!(vk.accepted.load(Ordering::SeqCst), 2);
}

fn app_for(api_base: &str, token_url: String) -> (App, Arc<Mutex<Vec<UiEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let app = App::new(
        VkClient::with_base(api_base).unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 1.0)) as Box<dyn Sink>),
        Box::new(MemoryStore::default()),
        move |e| log.lock().unwrap().push(e),
        AppOptions { fetch: lab_config(), token_url, watch_network: false, ..AppOptions::default() },
    );
    (app, events)
}

fn token_ok(token: &str) -> String {
    format!(r#"{{"type":"okay","data":{{"access_token":"{token}","user_id":7,"expires":0}}}}"#)
}

#[test]
fn a_write_through_the_app_is_not_retried_either() {
    let _serial = serial();
    let vk = RawVk::start(1);
    let (app, _) = app_for(&vk.base, "http://127.0.0.1:9/never".into());
    app.sign_in(session("tokentokentoken-1"));
    let track = pm_engine::vk::Track {
        track_id: 5, owner_id: -3, title: "T".into(), artist: "A".into(), album_title: None, duration: 10, stream_url: None,
        artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None,
    };
    assert!(app.add_to_library(&track).is_err());
    assert_eq!(vk.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn a_session_vk_stops_accepting_is_renewed_once_and_the_call_asked_again_once() {
    let _serial = serial();
    for (code, text) in [(5, "User authorization failed"), (17, "Validation required")] {
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        let api = FakeApi::start(move |_, body, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            if body.contains("tokentokentoken-OLD") {
                (200, format!(r#"{{"error":{{"error_code":{code},"error_msg":"{text}"}}}}"#))
            } else {
                (200, r#"{"response":{"count":0,"items":[]}}"#.into())
            }
        });
        let token = FakeApi::start(|_, _, _| (200, token_ok("tokentokentoken-NEW")));
        let (app, events) = app_for(&api.base, format!("{}/?act=web_token", token.base));
        app.sign_in(session("tokentokentoken-OLD"));
        assert!(app.library(0, 10).is_ok(), "error {code}");
        assert_eq!(token.call_count("web_token"), 1, "error {code}: one renewal");
        assert_eq!(hits.load(Ordering::SeqCst), 2, "error {code}: asked again once");
        assert!(!events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::SessionExpired)));
    }
}

#[test]
fn when_the_renewed_session_is_refused_too_it_gives_up_without_looping() {
    let _serial = serial();
    for (code, expected) in [(5, Error::Unauthorized), (17, Error::Vk { code: 17, message: "Validation required".into() })] {
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        let api = FakeApi::start(move |_, _, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            (200, format!(r#"{{"error":{{"error_code":{code},"error_msg":"Validation required"}}}}"#))
        });
        let token = FakeApi::start(|_, _, _| (200, token_ok("tokentokentoken-NEW")));
        let (app, _) = app_for(&api.base, format!("{}/?act=web_token", token.base));
        app.sign_in(session("tokentokentoken-OLD"));
        assert_eq!(app.library(0, 10).unwrap_err(), expected);
        assert_eq!(token.call_count("web_token"), 1, "error {code}: renewed once");
        assert_eq!(hits.load(Ordering::SeqCst), 2, "error {code}: two calls, no loop");
    }
}

#[test]
fn a_failed_renewal_for_a_validation_error_keeps_the_original_answer() {
    let _serial = serial();
    let api = FakeApi::start(|_, _, _| (200, r#"{"error":{"error_code":17,"error_msg":"Validation required"}}"#.into()));
    let token = FakeApi::start(|_, _, _| (500, "{}".into()));
    let (app, _) = app_for(&api.base, format!("{}/?act=web_token", token.base));
    app.sign_in(session("tokentokentoken-OLD"));
    match app.library(0, 10).unwrap_err() {
        Error::Vk { code, .. } => assert_eq!(code, 17),
        other => panic!("{other:?}"),
    }
    assert!(app.session_state().signed_in, "a validation error is not a sign-out");
}

/// Скриптовый источник локальных адресов.
struct Script(Mutex<Vec<Fingerprint>>);

impl AddressSource for Script {
    fn read(&self) -> Fingerprint {
        let mut prints = self.0.lock().unwrap();
        if prints.len() > 1 { prints.remove(0) } else { prints[0] }
    }
}

fn print(ip: &str) -> Fingerprint {
    let ip = Some(ip.parse().unwrap());
    Fingerprint { v4: ip, v4_vk: ip, v6: None }
}

#[test]
fn the_app_tells_the_window_when_the_network_changes_and_settles() {
    let _serial = serial();
    let source = Arc::new(Script(Mutex::new(vec![print("192.168.1.5"), print("10.8.0.2")])));
    let events: Arc<Mutex<Vec<UiEvent>>> = Arc::default();
    let log = events.clone();
    let before = pm_engine::net::epoch();
    let app = App::new(
        VkClient::with_base("http://127.0.0.1:9").unwrap(),
        || Ok(Box::new(SimulatedSink::new(44_100, 2, 1.0)) as Box<dyn Sink>),
        Box::new(MemoryStore::default()),
        move |e| log.lock().unwrap().push(e),
        AppOptions {
            address_source: Some(source),
            network_config: MonitorConfig { poll: Duration::from_millis(20), confirm: Duration::from_millis(10) },
            ..AppOptions::default()
        },
    );
    // Ничего не играет и окно не сфокусировано: никто не смотрит.
    std::thread::sleep(Duration::from_millis(150));
    assert!(!events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::Network { .. })));

    app.set_window_focused(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    let states = || -> Vec<NetState> {
        events.lock().unwrap().iter().filter_map(|e| if let UiEvent::Network { state } = e { Some(*state) } else { None }).collect()
    };
    while states().len() < 2 {
        assert!(Instant::now() < deadline, "no network events: {:?}", states());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(states(), vec![NetState::Changed, NetState::Ok]);
    assert!(pm_engine::net::epoch() > before, "pooled connections were dropped");
}
