//! Test fixtures: real HLS streams made by ffmpeg, a tiny range-capable HTTP
//! server, and helpers to decode and measure what comes out.

#![allow(dead_code)]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use pm_engine::stream::{spawn_fetch, Decoder, FetchConfig, SharedBuffer};

pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// CI sets `PM_REQUIRE_FFMPEG=1` so a missing ffmpeg fails instead of skipping.
pub fn require_ffmpeg() -> bool {
    if ffmpeg_available() {
        return true;
    }
    assert!(std::env::var("PM_REQUIRE_FFMPEG").is_err(), "ffmpeg is required here but not installed");
    eprintln!("skipping: ffmpeg is not installed");
    false
}

pub fn run_ffmpeg(dir: &Path, args: &[&str]) {
    let output = Command::new("ffmpeg")
        .current_dir(dir)
        .args(["-y", "-loglevel", "error"])
        .args(args)
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
}

/// 6 s of 440 Hz then 6 s of 880 Hz, stereo, 44.1 kHz.
pub const TONE_FILTER: &str = "[0][1]concat=n=2:v=0:a=1[a]";
pub const TONE_INPUTS: [&str; 8] = ["-f", "lavfi", "-i", "sine=frequency=440:duration=6", "-f", "lavfi", "-i", "sine=frequency=880:duration=6"];

pub fn tone_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args: Vec<&str> = TONE_INPUTS.to_vec();
    args.extend(["-filter_complex", TONE_FILTER, "-map", "[a]", "-ar", "44100", "-ac", "2"]);
    args.extend_from_slice(extra);
    args
}

pub struct Server {
    pub base: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Server {
    /// Serves `root` on 127.0.0.1 with Range support.
    pub fn start(root: PathBuf) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let port = server.server_addr().to_ip().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let Ok(Some(request)) = server.recv_timeout(std::time::Duration::from_millis(50)) else { continue };
                let path = request.url().split('?').next().unwrap_or("").trim_start_matches('/').to_string();
                let file = root.join(&path);
                if path.contains("..") || !file.is_file() {
                    let _ = request.respond(tiny_http::Response::empty(404));
                    continue;
                }
                let data = fs::read(&file).unwrap();
                let range = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Range"))
                    .and_then(|h| h.value.as_str().strip_prefix("bytes=").map(str::to_string));
                match range.and_then(|r| {
                    let (a, b) = r.split_once('-')?;
                    Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                }) {
                    Some((start, end)) if start < data.len() => {
                        let end = end.min(data.len() - 1);
                        let mut response = tiny_http::Response::from_data(data[start..=end].to_vec()).with_status_code(206);
                        response.add_header(
                            tiny_http::Header::from_bytes(&b"Content-Range"[..], format!("bytes {start}-{end}/{}", data.len()).as_bytes())
                                .unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    _ => {
                        let _ = request.respond(tiny_http::Response::from_data(data));
                    }
                }
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), stop, handle: Some(handle) }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn lab_config() -> FetchConfig {
    let mut config = FetchConfig::new("pm-test");
    config.allow_loopback_http = true;
    config
}

pub struct Pcm {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / self.channels as f64 / self.rate as f64
    }

    /// Dominant frequency of channel 0 in `[from, to)` seconds, by zero crossings.
    pub fn frequency(&self, from: f64, to: f64) -> f64 {
        let start = (from * self.rate as f64) as usize;
        let end = ((to * self.rate as f64) as usize).min(self.samples.len() / self.channels);
        let mut crossings = 0usize;
        let mut previous = self.samples[start * self.channels];
        for frame in start + 1..end {
            let sample = self.samples[frame * self.channels];
            if (previous < 0.0) != (sample < 0.0) {
                crossings += 1;
            }
            previous = sample;
        }
        crossings as f64 / 2.0 / ((end - start) as f64 / self.rate as f64)
    }

    pub fn rms(&self) -> f64 {
        let sum: f64 = self.samples.iter().map(|s| (*s as f64).powi(2)).sum();
        (sum / self.samples.len().max(1) as f64).sqrt()
    }
}

pub fn decode_all(url: &str) -> Result<Pcm, pm_engine::Error> {
    let buffer = SharedBuffer::new(64 * 1024 * 1024);
    let _fetch = spawn_fetch(url.to_string(), lab_config(), buffer.clone());
    let kind = buffer.wait_for_kind()?;
    let mut decoder = Decoder::open(buffer.reader(), kind)?;
    let mut samples = Vec::new();
    while decoder.next_block(&mut samples)? {}
    Ok(Pcm { rate: decoder.sample_rate, channels: decoder.channels, samples })
}

pub fn read_file(path: &Path) -> Vec<u8> {
    let mut data = Vec::new();
    fs::File::open(path).unwrap().read_to_end(&mut data).unwrap();
    data
}

/// A scriptable fake HTTP API. The handler gets (path+query, body, headers as
/// "name: value" lines) and returns (status, json body).
pub struct FakeApi {
    pub base: String,
    pub calls: Arc<std::sync::Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FakeApi {
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(&str, &str, &str) -> (u16, String) + Send + 'static,
    {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, flag) = (calls.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let Ok(Some(mut request)) = server.recv_timeout(std::time::Duration::from_millis(30)) else { continue };
                let mut body = String::new();
                let _ = Read::read_to_string(request.as_reader(), &mut body);
                let headers: String =
                    request.headers().iter().map(|h| format!("{}: {}\n", h.field, h.value)).collect();
                let url = request.url().to_string();
                log.lock().unwrap().push((url.clone(), body.clone()));
                let (status, json) = handler(&url, &body, &headers);
                let mut response = tiny_http::Response::from_string(json).with_status_code(status);
                response.add_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), calls, stop, handle: Some(handle) }
    }

    pub fn call_count(&self, needle: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|(u, _)| u.contains(needle)).count()
    }
}

impl Drop for FakeApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Deterministic transport failure: accept and close instead of depending on
/// reserved closed ports, which firewalls can silently drop for a full timeout.
pub struct CutServer {
    pub base: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CutServer {
    pub fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                if let Ok((stream, _)) = listener.accept() { drop(stream); }
                else { std::thread::sleep(std::time::Duration::from_millis(5)); }
            }
        });
        Self { base, stop, handle: Some(handle) }
    }
}

impl Drop for CutServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() { let _ = handle.join(); }
    }
}
