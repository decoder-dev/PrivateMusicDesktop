//! Воспроизведение, которое самовосстанавливается при изменении сети или потока URLs под ним (включение
//! или выключение VPN): истекший URLs, разрыв соединений, ограниченные повторы, состояние паузы с
//! повтором, загрузки, которые никогда не замечаются.

mod common;

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use pm_engine::audio::{SimulatedSink, Sink};
use pm_engine::controller::{Controller, RecoveryPhase, RecoveryPolicy, State, Status, UiEvent};
use pm_engine::net::NetState;
use pm_engine::offline::OfflineStore;
use pm_engine::stream::fetch::fetch_to_end;
use pm_engine::vk::{Session, Track, VkClient};

fn track(n: i64, url: Option<String>) -> Track {
    Track {
        track_id: n,
        owner_id: 7,
        title: format!("Track {n}"),
        artist: "Artist".into(),
        album_title: None,
        duration: 12,
        stream_url: url,
        artwork_url: None,
        access_key: None,
        lyrics_id: None,
        is_hq: false,
        track_code: None,
        artists: Vec::new(),
        album: None,
    }
}

fn session() -> Session {
    Session {
        access_token: "0123456789abcdef0123456789".into(),
        user_id: Some(7),
        expires_at: None,
        api_user_agent: "test-agent".into(),
        refresh_cookie: String::new(),
        web_user_agent: "web".into(),
    }
}

/// Медиа-сервер с `old/` и `new/` обращен к одним и тем же файлам HLS. `old/` прекращает обслуживать
/// сегменты после `old_segments` (срок действия подписанного URL истек в середине дорожки) и может
/// разорвать соединение в середине сегмента.
struct Media {
    base: String,
    old_segments: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Media {
    fn start(root: PathBuf, old_limit: usize) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let old_segments = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (served, log, flag) = (old_segments.clone(), requests.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(30)) else { continue };
                let path = request.url().split('?').next().unwrap_or("").trim_start_matches('/').to_string();
                log.lock().unwrap().push(path.clone());
                let (face, file) = path.split_once('/').unwrap_or(("", ""));
                let target = root.join(file);
                if file.contains("..") || !target.is_file() || !matches!(face, "old" | "new") {
                    let _ = request.respond(tiny_http::Response::empty(404));
                    continue;
                }
                let segment = file.ends_with(".ts");
                if face == "old" && segment && served.fetch_add(1, Ordering::SeqCst) >= old_limit {
                    let _ = request.respond(tiny_http::Response::empty(403));
                    continue;
                }
                let data = std::fs::read(&target).unwrap();
                let _ = request.respond(tiny_http::Response::from_data(data));
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), old_segments, requests, stop, handle: Some(handle) }
    }

    fn url(&self, face: &str) -> String {
        format!("{}/{face}/index.m3u8", self.base)
    }

    fn count(&self, needle: &str) -> usize {
        self.requests.lock().unwrap().iter().filter(|p| p.contains(needle)).count()
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Обычные сокеты, так что соединение действительно можно обрезать: обещает целый файл, отправляет
/// половину файлов, названных в `cut` (по одному разу) и вешает трубку.
struct CutMedia {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl CutMedia {
    fn start(root: PathBuf, cut: &[&str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let cut: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(cut.iter().map(|c| c.to_string()).collect()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, flag) = (requests.clone(), stop.clone());
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let (root, log, cut) = (root.clone(), log.clone(), cut.clone());
                std::thread::spawn(move || {
                    let _ = stream.set_nonblocking(false);
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&head).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                    log.lock().unwrap().push(path.clone());
                    let file = path.split_once('/').map(|(_, f)| f).unwrap_or("");
                    let Ok(data) = std::fs::read(root.join(file)) else {
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    };
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                    if cut.lock().unwrap().remove(file) {
                        let _ = stream.write_all(&data[..data.len() / 2]);
                        let _ = stream.flush();
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                    } else {
                        let _ = stream.write_all(&data);
                    }
                });
            }
        });
        Self { base, requests, stop }
    }

    fn url(&self) -> String {
        format!("{}/old/index.m3u8", self.base)
    }

    fn count(&self, needle: &str) -> usize {
        self.requests.lock().unwrap().iter().filter(|p| p.contains(needle)).count()
    }
}

impl Drop for CutMedia {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn hls_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(
        dir.path(),
        &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]),
    );
    dir
}

/// Поддельный `api.vk.ru`, `audio.getById` которого отвечает `new/` лицом `stream_base` или цифрой 503,
/// в зависимости от `healthy`.
fn fake_vk(stream_url: String, healthy: Arc<AtomicBool>) -> FakeApi {
    FakeApi::start(move |_, body, _| {
        if !healthy.load(Ordering::SeqCst) {
            return (503, "{}".into());
        }
        let audios = body.split('&').find_map(|p| p.strip_prefix("audios=")).unwrap_or("").replace("%5F", "_");
        let mut parts = audios.split('_');
        let (owner, id) = (parts.next().unwrap_or("7").to_string(), parts.next().unwrap_or("1").to_string());
        (
            200,
            format!(r#"{{"response":[{{"id":{id},"owner_id":{owner},"artist":"Artist","title":"Fresh","duration":12,"url":"{stream_url}"}}]}}"#),
        )
    })
}

struct Rig {
    controller: Controller,
    events: Arc<Mutex<Vec<UiEvent>>>,
    states: Arc<Mutex<Vec<State>>>,
}

fn rig(vk_base: &str, speed: f64) -> Rig {
    let states = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (s, e) = (states.clone(), events.clone());
    let mut slot = Some(SimulatedSink::new(44_100, 2, speed));
    let controller = Controller::with_fetch(
        VkClient::with_base(vk_base).unwrap(),
        move || Ok(Box::new(slot.take().unwrap_or_else(|| SimulatedSink::new(44_100, 2, 20.0))) as Box<dyn Sink>),
        move |event| {
            if let UiEvent::State(state) = &event {
                s.lock().unwrap().push(state.clone());
            }
            e.lock().unwrap().push(event);
        },
        1.0,
        lab_config(),
    );
    // Ожидание сокращается в пятьдесят раз; форма графика сохраняется.
    controller.set_recovery_policy(RecoveryPolicy { time_scale: 0.02, stall_after: Duration::from_secs(100), ..RecoveryPolicy::default() });
    controller.set_session(Some(session()));
    Rig { controller, events, states }
}

impl Rig {
    fn wait_state(&self, what: &str, f: impl Fn(&State) -> bool) {
        wait_for(what, || f(&self.controller.state()));
    }

    fn recoveries(&self, wanted: RecoveryPhase) -> Vec<(u32, u32)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                UiEvent::Recovery { phase, attempt, max, .. } if *phase == wanted => Some((*attempt, *max)),
                _ => None,
            })
            .collect()
    }

    fn errors(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                UiEvent::Error { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect()
    }
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_url_that_expires_mid_track_is_replaced_and_playback_continues_from_the_position() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    // Проходят два из шести сегментов (4 с), затем знак URL "expires".
    let media = Media::start(dir.path().to_path_buf(), 2);
    let healthy = Arc::new(AtomicBool::new(true));
    let vk = fake_vk(media.url("new"), healthy);
    let rig = rig(&vk.base, 8.0);
    rig.controller.play_tracks(vec![track(1, Some(media.url("old"))), track(2, Some(media.url("new")))], 0);

    wait_for("the recovery to be reported", || !rig.recoveries(RecoveryPhase::Recovered).is_empty());
    // Трек 1 заканчивается и воспроизводится трек 2: очередь ни разу не потерялась.
    rig.wait_state("the end of the queue", |s| s.status == Status::Idle && s.track.as_ref().map(|t| t.track_id) == Some(2));

    assert_eq!(rig.recoveries(RecoveryPhase::Retrying), vec![(1, 3)], "one attempt, from a content failure");
    assert_eq!(vk.call_count("/method/audio.getById"), 1, "exactly one fresh URL asked for");
    assert!(rig.errors().is_empty(), "healing is silent: {:?}", rig.errors());
    // Воспроизведение продолжилось с того места, где оно было прервано, а не с самого начала.
    let states = rig.states.lock().unwrap().clone();
    let broke_at = states.iter().filter(|s| s.track.as_ref().map(|t| t.track_id) == Some(1)).map(|s| s.position).fold(0.0, f64::max);
    assert!(broke_at >= 3.0, "the stream broke around 4 s, saw {broke_at}");
    let queue_len = states.last().map(|s| s.queue_len);
    assert_eq!(queue_len, Some(2));
    assert!(media.count("new/index") >= 1 && media.old_segments.load(Ordering::SeqCst) >= 3);
}

#[test]
fn the_position_survives_the_recovery() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), 3);
    let vk = fake_vk(media.url("new"), Arc::new(AtomicBool::new(true)));
    let rig = rig(&vk.base, 6.0);
    rig.controller.play_tracks(vec![track(1, Some(media.url("old")))], 0);
    wait_for("the retry", || !rig.recoveries(RecoveryPhase::Retrying).is_empty());
    // Первое состояние воспроизведения после повторной попытки начинается около перерыва (6 секунд из
    // 12 были буферизованы), а не в 0:00.
    let retry_at = rig.states.lock().unwrap().len();
    wait_for("playing again", || rig.states.lock().unwrap().iter().skip(retry_at).any(|s| s.status == Status::Playing));
    let resumed = rig.states.lock().unwrap().iter().skip(retry_at).find(|s| s.status == Status::Playing).map(|s| s.position).unwrap();
    assert!(resumed >= 4.0, "resumed at {resumed} s");
    rig.wait_state("the end", |s| s.status == Status::Idle && s.position == 0.0);
}

#[test]
fn a_connection_dropped_in_the_middle_of_a_segment_is_fetched_again() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = CutMedia::start(dir.path().to_path_buf(), &["index3.ts"]);
    let pcm = decode_all(&media.url()).expect("the stream recovers by itself");
    assert!((pcm.seconds() - 12.0).abs() < 0.5, "the whole track arrived: {} s", pcm.seconds());
    assert!(media.count("index3.ts") >= 2, "the broken segment was asked for again");
    assert!((pcm.frequency(7.0, 11.0) - 880.0).abs() < 15.0, "and the audio after it is intact");
}

#[test]
fn a_dead_url_is_not_retried_by_the_fetcher_itself() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), 0);
    // 403 в каждом сегменте: по одному запросу для первого сегмента, без забивки.
    let error = decode_all(&media.url("old")).err().expect("fails");
    assert_eq!(error, pm_engine::Error::Http(403));
    assert_eq!(media.count("index0.ts"), 1);
}

#[test]
fn repeated_connection_failures_are_bounded_then_playback_pauses_and_offers_a_retry() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let healthy = Arc::new(AtomicBool::new(false));
    let vk = fake_vk(media.url("new"), healthy.clone());
    let rig = rig(&vk.base, 20.0);
    let cut = CutServer::start();
    rig.controller.play_tracks(vec![track(1, Some(cut.base.clone())), track(2, Some(media.url("new")))], 0);

    wait_for("the pause", || !rig.recoveries(RecoveryPhase::Failed).is_empty());
    rig.wait_state("idle", |s| s.status == Status::Idle);
    let state = rig.controller.state();
    assert_eq!(state.track.as_ref().map(|t| t.track_id), Some(1), "a connection problem never skips tracks");
    assert_eq!(state.queue_len, 2);
    let errors = rig.errors();
    assert_eq!(errors.len(), 1, "one clear error, not one per attempt: {errors:?}");
    assert!(matches!(errors[0].as_str(), "http" | "network"));
    let attempts: Vec<u32> = rig.recoveries(RecoveryPhase::Retrying).iter().map(|(a, _)| *a).collect();
    assert_eq!(attempts, vec![1, 2, 3, 4, 5, 6], "six attempts, then it stops");
    assert_eq!(vk.call_count("/method/audio.getById"), 6, "one request per attempt, none extra");

    // Он остается остановленным: в фоновом режиме не вращается.
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(vk.call_count("/method/audio.getById"), 6);
    assert_eq!(rig.recoveries(RecoveryPhase::Retrying).len(), 6);
    assert_eq!(rig.controller.state().status, Status::Idle);

    // Пользователь исправляет VPN и нажимает «Повторить»: новый URL, тот же трек.
    healthy.store(true, Ordering::SeqCst);
    rig.controller.retry();
    wait_for("track 1 to play again", || rig.states.lock().unwrap().iter().any(|s| s.status == Status::Playing));
    rig.wait_state("the queue to finish", |s| s.status == Status::Idle && s.track.as_ref().map(|t| t.track_id) == Some(2));
    assert_eq!(vk.call_count("/method/audio.getById"), 8, "one for each track, after the retry");
}

#[test]
fn playback_that_gave_up_resumes_by_itself_when_the_network_settles() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let healthy = Arc::new(AtomicBool::new(false));
    let vk = fake_vk(media.url("new"), healthy.clone());
    let rig = rig(&vk.base, 20.0);
    let cut = CutServer::start();
    rig.controller.play_tracks(vec![track(1, Some(cut.base.clone()))], 0);
    wait_for("the pause", || !rig.recoveries(RecoveryPhase::Failed).is_empty());

    // Сеть возвращается: "changed", затем "ok", как только она замирает.
    healthy.store(true, Ordering::SeqCst);
    rig.controller.on_network(NetState::Changed);
    rig.controller.on_network(NetState::Ok);
    wait_for("playing again without any button", || rig.states.lock().unwrap().iter().any(|s| s.status == Status::Playing));
    rig.wait_state("the end", |s| s.status == Status::Idle && s.position == 0.0);
    assert!(!rig.recoveries(RecoveryPhase::Recovered).is_empty());
}

#[test]
fn a_network_change_makes_cached_urls_stale_and_the_next_track_asks_for_a_fresh_one() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let vk = fake_vk(media.url("new"), Arc::new(AtomicBool::new(true)));
    let rig = rig(&vk.base, 20.0);
    rig.controller.play_tracks(vec![track(1, Some(media.url("old")))], 0);
    rig.wait_state("a normal play", |s| s.status == Status::Idle && s.position == 0.0 && s.track.is_some());
    assert_eq!(vk.call_count("/method/audio.getById"), 0, "before any change the held URL is used as is");

    rig.controller.on_network(NetState::Changed);
    rig.controller.play_tracks(vec![track(2, Some(media.url("old")))], 0);
    wait_for("the fresh URL", || vk.call_count("/method/audio.getById") == 1);
    rig.wait_state("the end", |s| s.status == Status::Idle && s.position == 0.0 && s.track.as_ref().map(|t| t.title.clone()) == Some("Fresh".into()));
    assert_eq!(media.count("old/index.m3u8"), 1, "the stale URL was not used again");
}

/// Драйвер может прекратить потребление звука, не возвращая ошибку. Как только сторожевой таймер
/// раковины отметит его как неработоспособный, восстановление должно снова открыть его по тому же пути.
struct StalledOutput {
    sink: SimulatedSink,
    stalled: Arc<AtomicBool>,
}

impl Sink for StalledOutput {
    fn sample_rate(&self) -> u32 { self.sink.sample_rate() }
    fn channels(&self) -> usize { self.sink.channels() }
    fn writable(&self) -> usize { self.sink.writable() }
    fn write(&mut self, samples: &[f32]) -> usize { self.sink.write(samples) }
    fn played_frames(&self) -> u64 { self.sink.played_frames() }
    fn queued_frames(&self) -> usize { self.sink.queued_frames() }
    fn flush(&mut self) { self.sink.flush(); }
    fn set_paused(&mut self, paused: bool) { self.sink.set_paused(paused); }
    fn set_volume(&mut self, volume: f32) { self.sink.set_volume(volume); }
    fn is_healthy(&self) -> bool { !self.stalled.load(Ordering::Relaxed) }
}

#[test]
fn an_unresponsive_output_is_reopened_without_restarting_the_app() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let vk = fake_vk(media.url("new"), Arc::new(AtomicBool::new(true)));
    let stalled = Arc::new(AtomicBool::new(false));
    let opened = Arc::new(AtomicUsize::new(0));
    let (flag, count) = (stalled.clone(), opened.clone());
    let controller = Controller::with_fetch(
        VkClient::with_base(&vk.base).unwrap(),
        move || {
            let first = count.fetch_add(1, Ordering::SeqCst) == 0;
            Ok(Box::new(StalledOutput {
                sink: SimulatedSink::new(48_000, 2, 1.0),
                stalled: if first { flag.clone() } else { Arc::new(AtomicBool::new(false)) },
            }) as Box<dyn Sink>)
        },
        |_| {},
        1.0,
        lab_config(),
    );
    controller.set_session(Some(session()));
    controller.play_tracks(vec![track(1, Some(media.url("new")))], 0);
    wait_for("audio before the output stalls", || controller.state().position >= 1.0);
    let before = controller.state().position;
    stalled.store(true, Ordering::Relaxed);
    wait_for("a reopened output playing the same track", || {
        opened.load(Ordering::SeqCst) == 2 && controller.state().status == Status::Playing
    });
    assert_eq!(controller.state().track.unwrap().track_id, 1);
    assert!(controller.state().position >= before - 0.1, "keeps the playback position");
    wait_for("audio advancing after recovery", || controller.state().position >= before + 0.5);
    assert_eq!(opened.load(Ordering::SeqCst), 2);
}

#[test]
fn downloaded_tracks_never_notice_the_network() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let healthy = Arc::new(AtomicBool::new(false));
    let vk = fake_vk(media.url("new"), healthy);
    let rig = rig(&vk.base, 4.0);
    let downloads = tempfile::tempdir().unwrap();
    let store = Arc::new(OfflineStore::open(downloads.path()).unwrap());
    let (kind, audio) = fetch_to_end(&media.url("new"), &lab_config()).unwrap();
    let saved = track(1, None);
    store.save(&saved, kind, &audio).unwrap();
    rig.controller.set_offline(store);
    drop(media); // не к чему дотянуться, а VK все равно отвечает на 503

    rig.controller.play_tracks(vec![track(1, Some("https://cdn.example/stale.m3u8".into()))], 0);
    rig.wait_state("playing from disk", |s| s.status == Status::Playing);
    // VPN включается и выключается во время воспроизведения.
    rig.controller.on_network(NetState::Changed);
    rig.controller.on_network(NetState::Unreachable);
    rig.controller.on_network(NetState::Changed);
    rig.controller.on_network(NetState::Ok);
    rig.wait_state("the end", |s| s.status == Status::Idle && s.position == 0.0);

    assert_eq!(vk.call_count("/method/"), 0, "VK was never asked");
    assert!(rig.recoveries(RecoveryPhase::Retrying).is_empty());
    assert!(rig.errors().is_empty());
}

#[test]
fn a_dead_track_is_still_skipped_once_its_fresh_urls_fail_too() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    // Свежий URLs так же мёртв, как и старый (403 на каждом сегменте).
    let media = Media::start(dir.path().to_path_buf(), 0);
    let vk = fake_vk(media.url("old"), Arc::new(AtomicBool::new(true)));
    let rig = rig(&vk.base, 20.0);
    rig.controller.play_tracks((1..=6).map(|n| track(n, Some(media.url("old")))).collect(), 0);
    wait_for("giving up on the queue", || {
        let s = rig.controller.state();
        s.status == Status::Idle && !rig.errors().is_empty()
    });
    std::thread::sleep(Duration::from_millis(300));
    let at = rig.controller.state().track.unwrap().track_id;
    assert!(at <= 3, "bounded skipping, stopped at track {at}");
    assert!(rig.recoveries(RecoveryPhase::Retrying).len() <= 3 * 3, "at most three fresh URLs per track");
}

#[test]
fn pausing_while_vk_resolves_a_track_rejects_the_late_reply() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let url = media.url("new");
    let vk = FakeApi::start(move |_, _, _| {
        std::thread::sleep(Duration::from_millis(180));
        (200, format!(r#"{{"response":[{{"id":1,"owner_id":7,"artist":"A","title":"Fresh","duration":12,"url":"{url}"}}]}}"#))
    });
    let rig = rig(&vk.base, 1.0);
    rig.controller.play_tracks(vec![track(1, None)], 0);
    wait_for("the pending URL request", || vk.call_count("/method/audio.getById") == 1);
    rig.controller.toggle(); // Загрузку необходимо отменить той же кнопкой.
    rig.wait_state("cancelled loading", |s| s.status == Status::Idle);
    rig.controller.on_network(NetState::Changed);
    rig.controller.on_network(NetState::Ok);
    std::thread::sleep(Duration::from_millis(450));
    assert_eq!(rig.controller.state().status, Status::Idle);
    assert_eq!(media.count("new/index.m3u8"), 0, "a late reply must not start any audio");
}

#[test]
fn a_url_resolved_before_a_network_change_is_never_marked_fresh() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let (old, new) = (media.url("old"), media.url("new"));
    let count = Arc::new(AtomicUsize::new(0));
    let round = count.clone();
    let vk = FakeApi::start(move |_, _, _| {
        let first = round.fetch_add(1, Ordering::SeqCst) == 0;
        if first { std::thread::sleep(Duration::from_millis(150)); }
        let url = if first { &old } else { &new };
        (200, format!(r#"{{"response":[{{"id":1,"owner_id":7,"artist":"A","title":"Fresh","duration":12,"url":"{url}"}}]}}"#))
    });
    let rig = rig(&vk.base, 1.0);
    rig.controller.play_tracks(vec![track(1, None)], 0);
    wait_for("the first lookup", || count.load(Ordering::SeqCst) == 1);
    rig.controller.on_network(NetState::Changed);
    rig.wait_state("fresh audio", |s| s.status == Status::Playing);
    assert_eq!(media.count("old/index.m3u8"), 0);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    rig.controller.pause();
    rig.wait_state("paused", |s| s.status == Status::Paused);
    rig.controller.on_network(NetState::Ok);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rig.controller.state().status, Status::Paused, "network recovery respects an explicit pause");
}

#[test]
fn sleep_timer_pauses_audio_and_can_be_cancelled() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let vk = fake_vk(media.url("new"), Arc::new(AtomicBool::new(true)));
    let rig = rig(&vk.base, 1.0);
    rig.controller.play_tracks(vec![track(1, Some(media.url("new")))], 0);
    rig.wait_state("playing", |s| s.status == Status::Playing);
    assert!(rig.controller.set_sleep_timer(Some(0), false).is_err());
    rig.controller.set_sleep_timer(Some(1), false).unwrap();
    rig.wait_state("sleep pause", |s| s.status == Status::Paused);
    let at = rig.controller.state().position;
    rig.controller.on_network(NetState::Changed);
    rig.controller.on_network(NetState::Ok);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rig.controller.state().position, at);
    rig.controller.resume();
    rig.wait_state("resumed", |s| s.status == Status::Playing);
    rig.controller.set_sleep_timer(Some(1), false).unwrap();
    rig.controller.set_sleep_timer(None, false).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(rig.controller.state().status, Status::Playing);
}

#[test]
fn end_of_track_timer_does_not_advance_the_queue() {
    if !require_ffmpeg() { return; }
    let dir = hls_dir();
    let media = Media::start(dir.path().to_path_buf(), usize::MAX);
    let vk = fake_vk(media.url("new"), Arc::new(AtomicBool::new(true)));
    let rig = rig(&vk.base, 10.0);
    rig.controller.play_tracks(vec![track(1, Some(media.url("new"))), track(2, Some(media.url("new")))], 0);
    rig.controller.set_sleep_timer(None, true).unwrap();
    rig.wait_state("playing", |s| s.status == Status::Playing);
    rig.wait_state("end of the first track", |s| s.status == Status::Idle);
    assert_eq!(rig.controller.state().track.unwrap().track_id, 1);
    assert_eq!(rig.controller.state().queue_len, 2);
}
