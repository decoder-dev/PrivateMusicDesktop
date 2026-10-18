//! Загружает поток VK в [`SharedBuffer`] как один элементарный поток.
//!
//! * копируется простой аудиофайл;
//! * список воспроизведения HLS обрабатывается сегмент за сегментом: AES-128 расшифровывается с помощью
//!   `pm_native`, MPEG-TS демультиплексируется с удаленными тегами `pm_native`, ID3.
//!
//! Каждый URL, который выходит из списка воспроизведения, представляет собой данные, находящиеся под
//! влиянием злоумышленника, поэтому каждый из них должен быть https (loopback http только для тестов),
//! не содержать учетных данных и не превышать ограничения по размеру, указанные ниже.

use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::Read;
use std::thread::JoinHandle;
use std::time::Duration;

use pm_native::hls::{self, pm_hls_slice, MediaPlaylist, PM_HLS_CHOOSE_FOUND, PM_HLS_CHOOSE_MEDIA_PLAYLIST, PM_HLS_OK};
use pm_native::media::{
    pm_aes128_cbc_decrypt, pm_mpegts_extract_audio, PM_AES128_OK, PM_MPEGTS_KIND_ADTS, PM_MPEGTS_KIND_MP3,
};
use reqwest::blocking::{Client, Response};
use reqwest::header::{RANGE, USER_AGENT};
use url::Url;

use super::buffer::{ElementaryKind, SharedBuffer};
use super::cmaf::Cmaf;
use crate::net;
use crate::{Error, Result};

const MAX_PLAYLIST_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEGMENT_BYTES: usize = 48 * 1024 * 1024;
const MAX_SEGMENTS: i32 = 4000;
const MAX_PLAYLIST_DEPTH: usize = 3;
/// Попытки для каждого списка воспроизведения, ключа или запроса сегмента (первая плюс еще две, каждая
/// при новом соединении, если при транспортировке последнего произошел сбой).
const REQUEST_ATTEMPTS: u32 = 3;

pub const MAX_STREAM_BYTES: usize = 150 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct FetchConfig {
    pub user_agent: String,
    /// Тесты выполняются с адреса 127.0.0.1 по простому http. Никогда не правда в приложении.
    pub allow_loopback_http: bool,
    pub max_stream_bytes: usize,
    /// Где живут скачанные треки; Адреса `offline:<file>` разрешаются здесь и больше нигде.
    pub offline_dir: Option<std::path::PathBuf>,
}

impl FetchConfig {
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self { user_agent: user_agent.into(), allow_loopback_http: false, max_stream_bytes: MAX_STREAM_BYTES, offline_dir: None }
    }
}

/// Схема адреса скачанного трека: `offline:<file name>`.
pub const OFFLINE_SCHEME: &str = "offline:";

/// Имя загруженного файла: буквы, цифры, `_`, `-` и одно расширение. Ничего, что могло бы выйти за
/// пределы папки загрузок.
pub(crate) fn safe_offline_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !name.starts_with('.')
        && !name.contains("..")
}

pub(crate) fn kind_for_name(name: &str) -> Option<ElementaryKind> {
    match name.rsplit_once('.')?.1 {
        "aac" => Some(ElementaryKind::AdtsAac),
        "mp3" => Some(ElementaryKind::Mp3),
        _ => None,
    }
}

/// Воспроизводит загруженный файл: тот же буфер, нет сети.
fn run_offline(name: &str, config: &FetchConfig, buffer: &SharedBuffer) -> Result<()> {
    let dir = config.offline_dir.as_ref().ok_or(Error::InvalidRequest)?;
    if !safe_offline_name(name) {
        return Err(Error::InvalidRequest);
    }
    let kind = kind_for_name(name).ok_or(Error::InvalidRequest)?;
    let mut file = std::fs::File::open(dir.join(name)).map_err(|e| Error::Stream(format!("downloaded file: {e}")))?;
    buffer.set_kind(kind);
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        if buffer.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let n = std::io::Read::read(&mut file, &mut chunk).map_err(|e| Error::Stream(format!("downloaded file: {e}")))?;
        if n == 0 {
            return Ok(());
        }
        if !buffer.push(&chunk[..n]) {
            return if buffer.is_cancelled() { Err(Error::Cancelled) } else { Err(buffer.failure().unwrap_or(Error::Cancelled)) };
        }
    }
}

/// Загружает весь поток (HLS или прогрессивный) и возвращает аудио, готовое к сохранению:
/// расшифрованное и демультиплексированное точно так же, как и при воспроизведении.
pub fn fetch_to_end(url: &str, config: &FetchConfig) -> Result<(ElementaryKind, Vec<u8>)> {
    if url.starts_with(OFFLINE_SCHEME) {
        return Err(Error::InvalidRequest); // уже загружен
    }
    let buffer = SharedBuffer::new(config.max_stream_bytes);
    run(url, config, &buffer)?;
    buffer.finish();
    buffer.completed_bytes().ok_or_else(|| Error::Stream("the stream held no audio".into()))
}

/// Начинает загрузку в отдельном потоке. Буфер завершается, завершается с ошибкой или отменяется, когда
/// поток завершается; вызывающим абонентам никогда не нужно присоединяться к нему.
pub fn spawn_fetch(url: String, config: FetchConfig, buffer: SharedBuffer) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("pm-fetch".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&url, &config, &buffer)));
            match result {
                Ok(Ok(())) => buffer.finish(),
                Ok(Err(Error::Cancelled)) => buffer.cancel(),
                Ok(Err(error)) => buffer.fail(error),
                Err(_) => buffer.fail(Error::Stream("internal error while downloading".into())),
            }
        })
        .expect("spawn fetch thread")
}

struct Fetcher<'a> {
    /// Клиент и генерация соединения, в рамках которой он был построен.
    http: RefCell<(u64, Client)>,
    config: &'a FetchConfig,
    buffer: &'a SharedBuffer,
}

fn run(url: &str, config: &FetchConfig, buffer: &SharedBuffer) -> Result<()> {
    if let Some(name) = url.strip_prefix(OFFLINE_SCHEME) {
        return run_offline(name, config, buffer);
    }
    let start = parse_url(url, config)?;
    let fetcher = Fetcher { http: RefCell::new((net::epoch(), build_client(config.allow_loopback_http))), config, buffer };
    fetcher.run(start)
}

fn build_client(allow_loopback: bool) -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // Это предел одной блокирующей операции чтения, а не длины трека.
        // Мёртвое соединение не должно держать отменённую загрузку целую минуту.
        .timeout(Duration::from_secs(15))
        .dns_resolver(std::sync::Arc::new(PublicResolver { allow_loopback }))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            let url = attempt.url();
            let ok = (url.scheme() == "https" && !is_local_host(url)) || (allow_loopback && is_loopback(url));
            if attempt.previous().len() >= 3 || !ok {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .expect("the stream client configuration is valid")
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]" | "::1"))
}

/// Шлейфовые, частные, локальные, уникальные локальные, неуказанные и широковещательные адреса:
/// собственная машина и сеть пользователя.
pub(crate) fn is_local_ip(ip: std::net::IpAddr) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    fn v4(a: Ipv4Addr) -> bool {
        a.is_loopback() || a.is_private() || a.is_link_local() || a.is_unspecified() || a.is_broadcast()
    }
    fn v6(a: Ipv6Addr) -> bool {
        let first = a.segments()[0];
        a.is_loopback()
            || a.is_unspecified()
            || first & 0xfe00 == 0xfc00 // уникальный местный
            || first & 0xffc0 == 0xfe80 // ссылка локальная
            || a.to_ipv4_mapped().is_some_and(v4)
    }
    match ip {
        IpAddr::V4(a) => v4(a),
        IpAddr::V6(a) => v6(a),
    }
}

/// Буквальные локальные адреса и `localhost`: адрес потока, передаваемый окном, не должен позволять
/// движку стучать на собственной машине или в сети пользователя. Имена проверяются еще раз после DNS
/// ([`PublicResolver`]).
pub(crate) fn is_local_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(a)) => is_local_ip(a.into()),
        Some(url::Host::Ipv6(a)) => is_local_ip(a.into()),
        None => true,
    }
}

/// DNS, который отказывается от разрешения имен в локальные адреса (перепривязка `127.0.0.1.nip.io`,
/// DNS), поэтому проверку буквального адреса невозможно обойти.
pub(crate) struct PublicResolver {
    pub allow_loopback: bool,
}

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let allow_loopback = self.allow_loopback;
        Box::pin(async move {
            use std::net::ToSocketAddrs;
            let found: Vec<std::net::SocketAddr> = (host.as_str(), 0).to_socket_addrs()?.collect();
            let public: Vec<_> = found
                .into_iter()
                .filter(|a| !is_local_ip(a.ip()) || (allow_loopback && a.ip().is_loopback()))
                .collect();
            if public.is_empty() {
                return Err(format!("{host} does not resolve to a public address").into());
            }
            Ok(Box::new(public.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

fn parse_url(raw: &str, config: &FetchConfig) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| Error::Stream("invalid stream address".into()))?;
    check_url(url, config)
}

fn check_url(url: Url, config: &FetchConfig) -> Result<Url> {
    let allowed = match url.scheme() {
        "https" => !is_local_host(&url),
        "http" => config.allow_loopback_http && is_loopback(&url),
        _ => false,
    };
    if !allowed || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Stream("stream address is not allowed".into()));
    }
    Ok(url)
}

impl Fetcher<'_> {
    fn check_cancel(&self) -> Result<()> {
        if self.buffer.is_cancelled() { Err(Error::Cancelled) } else { Ok(()) }
    }

    /// Клиент для следующего запроса: сначала пересобирается, если сеть менялась с момента его
    /// создания, чтобы ни один запрос на сокет из старого не отправлялся.
    fn http(&self) -> Client {
        let mut slot = self.http.borrow_mut();
        let now = net::epoch();
        if slot.0 != now {
            log::info!("network changed: stream download continues on new connections");
            *slot = (now, build_client(self.config.allow_loopback_http));
        }
        slot.1.clone()
    }

    /// Спит `seconds`, рано просыпается (с `Cancelled`), если воспроизведение продолжается.
    fn nap(&self, seconds: f64) -> Result<()> {
        let end = std::time::Instant::now() + Duration::from_secs_f64(seconds);
        while std::time::Instant::now() < end {
            self.check_cancel()?;
            std::thread::sleep(Duration::from_millis(50));
        }
        self.check_cancel()
    }

    /// Выполняет один шаг сети до [`REQUEST_ATTEMPTS`] раз. Плохое соединение (сброс, тайм-аут, DNS,
    /// TLS) или занятый сервер (408/429/5xx) повторяется с короткими резкими задержками, причем первый
    /// вариант - при совершенно новых соединениях. 401/403/404/410 нет: срок действия адреса истек, и
    /// помогает только свежий от VK (задание контроллера).
    fn retrying<T>(&self, step: impl Fn() -> Result<T>) -> Result<T> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.check_cancel()?;
            let error = match step() {
                Ok(value) => return Ok(value),
                Err(error) => error,
            };
            let connectivity = net::is_connectivity(&error);
            let busy = matches!(error, Error::Http(408 | 429 | 500..=599));
            if attempt >= REQUEST_ATTEMPTS || !(connectivity || busy) {
                return Err(error);
            }
            log::info!("stream request: {} → {}retrying (attempt {}/{REQUEST_ATTEMPTS})", error.kind(), if connectivity { "new connection, " } else { "" }, attempt + 1);
            if connectivity {
                *self.http.borrow_mut() = (net::epoch(), build_client(self.config.allow_loopback_http));
            }
            self.nap(net::backoff(attempt - 1, 0.3, 2.0, 0.3, net::jitter_unit()))?;
        }
    }

    fn send(&self, url: &Url, range: Option<(u64, u64)>) -> Result<Response> {
        self.retrying(|| self.send_once(url, range))
    }

    fn send_once(&self, url: &Url, range: Option<(u64, u64)>) -> Result<Response> {
        let mut request = self.http().get(url.clone()).header(USER_AGENT, self.config.user_agent.as_str());
        if let Some((start, length)) = range {
            request = request.header(RANGE, format!("bytes={}-{}", start, start + length - 1));
        }
        match request.send() {
            Ok(response) if response.status().is_success() => Ok(response),
            Ok(response) => {
                let status = response.status().as_u16();
                let delay = net::retry_after(response.headers());
                if (status == 429 || status == 503) && (delay.is_some() || status == 429) {
                    return Err(Error::RetryAfter { status, seconds:(delay.unwrap_or(Duration::from_secs(1)).as_secs_f64().ceil() as u64).max(1) });
                }
                Err(Error::Http(status))
            },
            Err(err) => Err(Error::from(err)),
        }
    }

    /// Все тело, в шапке.
    fn read_all(&self, mut response: Response, cap: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            self.check_cancel()?;
            let n = response.read(&mut chunk).map_err(|e| net::read_error(&e))?;
            if n == 0 {
                return Ok(out);
            }
            if out.len() + n > cap {
                return Err(Error::Stream("response is larger than the size limit".into()));
            }
            out.extend_from_slice(&chunk[..n]);
        }
    }

    fn get_bytes(&self, url: &Url, range: Option<(u64, u64)>, cap: usize) -> Result<Vec<u8>> {
        // Запрос и чтение его тела составляют один шаг: соединение, разрывающееся в середине сегмента,
        // снова извлекает этот сегмент.
        let body = self.retrying(|| {
            let response = self.send_once(url, range)?;
            let status = response.status().as_u16();
            Ok((status, self.read_all(response, cap)?))
        })?;
        let (status, body) = body;
        match range {
            None => Ok(body),
            Some((start, length)) => {
                if status == 206 {
                    if body.len() as u64 == length { Ok(body) } else { Err(Error::Stream("short range response".into())) }
                } else {
                    // Сервер проигнорировал Range и отправил файл целиком.
                    let start = start as usize;
                    let end = start.checked_add(length as usize).ok_or_else(|| Error::Stream("bad byte range".into()))?;
                    body.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| Error::Stream("range outside response".into()))
                }
            }
        }
    }

    fn run(&self, start: Url) -> Result<()> {
        let mut response = self.send(&start, None)?;
        // Sniff: список воспроизведения начинается с #EXTM3U, обычного потока со звуком.
        let mut head = vec![0u8; 4096];
        let mut filled = 0;
        while filled < 16 {
            self.check_cancel()?;
            let n = response.read(&mut head[filled..]).map_err(|e| net::read_error(&e))?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        head.truncate(filled);
        if head.is_empty() {
            return Err(Error::Stream("empty response".into()));
        }
        if looks_like_playlist(&head) {
            let mut rest = self.read_all(response, MAX_PLAYLIST_BYTES.saturating_sub(head.len()))?;
            head.append(&mut rest);
            let text = String::from_utf8(head).map_err(|_| Error::Stream("playlist is not UTF-8".into()))?;
            self.run_playlist(start, text, 0)
        } else {
            self.run_progressive(head, response)
        }
    }

    fn run_progressive(&self, head: Vec<u8>, mut response: Response) -> Result<()> {
        let mut first = head;
        // Обложки в ID3 бывают больше первого сетевого блока. Пропускаем метаданные
        // порциями, проверяя отмену, и определяем формат по настоящему началу звука.
        let mut metadata_bytes = 0usize;
        let mut scratch = [0u8; 16 * 1024];
        loop {
            while first.len() < 16 {
                self.check_cancel()?;
                let remaining = 16 - first.len();
                let n = response.read(&mut scratch[..remaining]).map_err(|e| net::read_error(&e))?;
                if n == 0 {
                    break;
                }
                first.extend_from_slice(&scratch[..n]);
            }
            if !first.starts_with(b"ID3") {
                break;
            }
            let length = id3_length(&first).ok_or_else(|| Error::Stream("invalid ID3 header".into()))?;
            metadata_bytes = metadata_bytes
                .checked_add(length)
                .filter(|bytes| *bytes <= MAX_SEGMENT_BYTES)
                .ok_or_else(|| Error::Stream("metadata exceeds the size limit".into()))?;
            if length <= first.len() {
                first.drain(..length);
            } else {
                let mut remaining = length - first.len();
                first.clear();
                while remaining > 0 {
                    self.check_cancel()?;
                    let count = remaining.min(scratch.len());
                    let n = response.read(&mut scratch[..count]).map_err(|e| net::read_error(&e))?;
                    if n == 0 {
                        return Err(Error::Stream("truncated ID3 metadata".into()));
                    }
                    remaining -= n;
                }
            }
        }
        let kind = sniff_elementary(&first).ok_or_else(|| Error::Stream("unsupported audio format".into()))?;
        self.buffer.set_kind(kind);
        if !first.is_empty() && !self.buffer.push(&first) {
            return self.stopped();
        }
        let mut chunk = [0u8; 32 * 1024];
        loop {
            self.check_cancel()?;
            let n = response.read(&mut chunk).map_err(|e| net::read_error(&e))?;
            if n == 0 {
                return Ok(());
            }
            if !self.buffer.push(&chunk[..n]) {
                return self.stopped();
            }
        }
    }

    fn stopped(&self) -> Result<()> {
        match self.buffer.failure() {
            Some(error) => Err(error),
            None => Err(Error::Cancelled),
        }
    }

    fn run_playlist(&self, url: Url, text: String, depth: usize) -> Result<()> {
        if depth > MAX_PLAYLIST_DEPTH {
            return Err(Error::Stream("playlist nesting is too deep".into()));
        }
        let (code, slice) = hls::choose_audio(&text);
        if code == PM_HLS_CHOOSE_FOUND {
            let uri = slice_text(&text, slice).ok_or_else(|| Error::Stream("bad variant address".into()))?;
            let next = self.resolve(&url, uri)?;
            let body = self.get_bytes(&next, None, MAX_PLAYLIST_BYTES)?;
            let next_text = String::from_utf8(body).map_err(|_| Error::Stream("playlist is not UTF-8".into()))?;
            return self.run_playlist(next, next_text, depth + 1);
        }
        if code != PM_HLS_CHOOSE_MEDIA_PLAYLIST {
            return Err(Error::Stream("playlist has no audio".into()));
        }
        let playlist = hls::parse_media(&text, MAX_SEGMENTS);
        if playlist.code != PM_HLS_OK {
            return Err(Error::Stream(format!("unsupported playlist (code {})", playlist.code)));
        }
        if playlist.segments.is_empty() {
            return Err(Error::Stream("playlist has no segments".into()));
        }
        self.run_segments(&url, &text, &playlist)
    }

    fn resolve(&self, base: &Url, reference: &str) -> Result<Url> {
        let joined = base.join(reference.trim()).map_err(|_| Error::Stream("bad playlist address".into()))?;
        check_url(joined, self.config)
    }

    /// Байты одного сегмента: ранжированы, загружены и расшифрованы.
    fn segment_bytes(
        &self,
        base: &Url,
        text: &str,
        playlist: &MediaPlaylist,
        index: usize,
        keys: &mut HashMap<i32, [u8; 16]>,
        next_offset: &mut HashMap<String, u64>,
    ) -> Result<Vec<u8>> {
        let segment = &playlist.segments[index];
        let uri = slice_text(text, segment.uri).ok_or_else(|| Error::Stream("bad segment address".into()))?;
        let url = self.resolve(base, uri)?;
        let range = if segment.has_range {
            if segment.range_length <= 0 {
                return Err(Error::Stream("invalid byte range".into()));
            }
            let length = segment.range_length as u64;
            let start = if segment.has_range_offset && segment.range_offset >= 0 {
                segment.range_offset as u64
            } else {
                *next_offset.get(url.as_str()).unwrap_or(&0)
            };
            next_offset.insert(url.as_str().to_string(), start.saturating_add(length));
            Some((start, length))
        } else {
            None
        };
        let mut data = self.get_bytes(&url, range, MAX_SEGMENT_BYTES)?;
        if segment.key_index >= 0 {
            let iv_override = None;
            data = self.decrypt_with_key(base, text, playlist, segment.key_index, index, iv_override, keys, data)?;
        }
        Ok(data)
    }

    #[allow(clippy::too_many_arguments)]
    fn decrypt_with_key(
        &self,
        base: &Url,
        text: &str,
        playlist: &MediaPlaylist,
        key_index: i32,
        segment_index: usize,
        iv_override: Option<[u8; 16]>,
        keys: &mut HashMap<i32, [u8; 16]>,
        data: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let key_info = playlist
            .keys
            .get(key_index as usize)
            .ok_or_else(|| Error::Stream("segment refers to a missing key".into()))?;
        let key = match keys.get(&key_index) {
            Some(key) => *key,
            None => {
                let key_uri = slice_text(text, key_info.uri).ok_or_else(|| Error::Stream("bad key address".into()))?;
                let key_url = self.resolve(base, key_uri)?;
                let bytes = self.get_bytes(&key_url, None, 64)?;
                let key: [u8; 16] = bytes.try_into().map_err(|_| Error::Stream("encryption key is not 16 bytes".into()))?;
                keys.insert(key_index, key);
                key
            }
        };
        let iv = iv_override.unwrap_or_else(|| {
            if key_info.has_iv {
                key_info.iv
            } else {
                sequence_iv(playlist.media_sequence.saturating_add(segment_index as i64))
            }
        });
        decrypt(&data, &key, &iv)
    }

    fn run_segments(&self, base: &Url, text: &str, playlist: &MediaPlaylist) -> Result<()> {
        if !playlist.maps.is_empty() {
            return self.run_cmaf(base, text, playlist);
        }
        let mut keys: HashMap<i32, [u8; 16]> = HashMap::new();
        let mut next_offset: HashMap<String, u64> = HashMap::new();
        let mut kind: Option<ContainerKind> = None;

        for index in 0..playlist.segments.len() {
            self.check_cancel()?;
            let data = self.segment_bytes(base, text, playlist, index, &mut keys, &mut next_offset)?;
            let detected = detect_container(&data).ok_or_else(|| Error::Stream("unsupported segment container".into()))?;
            if let Some(first) = kind {
                if first != detected {
                    return Err(Error::Stream("segment container changed mid-stream".into()));
                }
            } else {
                kind = Some(detected);
            }
            let (elementary_kind, mut bytes) = match detected {
                ContainerKind::MpegTs => demux_ts(&data)?,
                ContainerKind::Adts => (ElementaryKind::AdtsAac, data),
                ContainerKind::Mp3 => (ElementaryKind::Mp3, data),
            };
            strip_id3_prefix(&mut bytes);
            if index == 0 {
                self.buffer.set_kind(elementary_kind);
            }
            if !self.buffer.push(&bytes) {
                return self.stopped();
            }
        }
        Ok(())
    }

    /// Загружает, расшифровывает и анализирует один раздел инициализации `EXT-X-MAP`.
    fn load_init(
        &self,
        base: &Url,
        text: &str,
        playlist: &MediaPlaylist,
        map_index: i32,
        keys: &mut HashMap<i32, [u8; 16]>,
    ) -> Result<Cmaf> {
        let map = usize::try_from(map_index)
            .ok()
            .and_then(|i| playlist.maps.get(i))
            .ok_or_else(|| Error::Stream("segment has no initialization section".into()))?;
        let uri = slice_text(text, map.uri).ok_or_else(|| Error::Stream("bad initialization address".into()))?;
        let url = self.resolve(base, uri)?;
        // Диапазон карты без смещения начинается с байта 0, а не с курсора медиасегмента.
        let range = if map.has_range {
            if map.range_length <= 0 {
                return Err(Error::Stream("invalid byte range".into()));
            }
            let start = if map.has_range_offset && map.range_offset >= 0 { map.range_offset as u64 } else { 0 };
            Some((start, map.range_length as u64))
        } else {
            None
        };
        let mut data = self.get_bytes(&url, range, MAX_SEGMENT_BYTES)?;
        if map.key_index >= 0 {
            let key_info = playlist
                .keys
                .get(map.key_index as usize)
                .ok_or_else(|| Error::Stream("initialization refers to a missing key".into()))?;
            // RFC 8216: для зашифрованного раздела инициализации требуется явный IV.
            if !key_info.has_iv {
                return Err(Error::Stream("encrypted initialization section has no IV".into()));
            }
            data = self.decrypt_with_key(base, text, playlist, map.key_index, 0, Some(key_info.iv), keys, data)?;
        }
        Cmaf::parse_init(&data)
    }

    fn run_cmaf(&self, base: &Url, text: &str, playlist: &MediaPlaylist) -> Result<()> {
        let mut keys: HashMap<i32, [u8; 16]> = HashMap::new();
        let mut next_offset: HashMap<String, u64> = HashMap::new();
        let mut inits: HashMap<i32, Cmaf> = HashMap::new();

        for index in 0..playlist.segments.len() {
            self.check_cancel()?;
            let map_index = playlist.segments[index].map_index;
            let cmaf = match inits.entry(map_index) {
                Entry::Occupied(slot) => slot.into_mut(),
                Entry::Vacant(slot) => slot.insert(self.load_init(base, text, playlist, map_index, &mut keys)?),
            };
            let data = self.segment_bytes(base, text, playlist, index, &mut keys, &mut next_offset)?;
            let adts = cmaf.fragment_to_adts(&data)?;
            if index == 0 {
                self.buffer.set_kind(ElementaryKind::AdtsAac);
            }
            if !self.buffer.push(&adts) {
                return self.stopped();
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    MpegTs,
    Adts,
    Mp3,
}

fn looks_like_playlist(head: &[u8]) -> bool {
    let trimmed = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let start = trimmed.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
    trimmed[start..].starts_with(b"#EXTM3U")
}

fn slice_text(text: &str, slice: pm_hls_slice) -> Option<&str> {
    if slice.length < 0 || slice.offset < 0 {
        return None;
    }
    let start = slice.offset as usize;
    let end = start.checked_add(slice.length as usize)?;
    text.get(start..end).filter(|s| !s.is_empty())
}

/// RFC 8216 §5.2: IV по умолчанию — это порядковый номер мультимедиа в виде 128-битного целого числа с
/// прямым порядком байтов.
fn sequence_iv(sequence: i64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&(sequence.max(0) as u64).to_be_bytes());
    iv
}

fn decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>> {
    if data.is_empty() || data.len() > i32::MAX as usize / 2 {
        return Err(Error::Stream("cannot decrypt this segment".into()));
    }
    let capacity = data.len() + 16;
    let mut out = vec![0u8; capacity];
    let mut out_length: i32 = 0;
    // SAFETY: каждый указатель действителен для переданной с ним длины; key и iv имеют размер 16 байт,
    // как того требует встроенная функция.
    let status = unsafe {
        pm_aes128_cbc_decrypt(
            data.as_ptr(),
            data.len() as i32,
            key.as_ptr(),
            iv.as_ptr(),
            out.as_mut_ptr(),
            capacity as i32,
            &mut out_length,
        )
    };
    if status != PM_AES128_OK || out_length < 0 {
        return Err(Error::Stream("segment decryption failed".into()));
    }
    out.truncate(out_length as usize);
    Ok(out)
}

fn detect_container(data: &[u8]) -> Option<ContainerKind> {
    // MPEG-TS: байт синхронизации 0x47, повторяющийся каждые 188 байт.
    for offset in 0..data.len().min(16) {
        if data[offset] == 0x47 && (data.len() <= 188 || data.get(offset + 188) == Some(&0x47)) {
            return Some(ContainerKind::MpegTs);
        }
    }
    sniff_elementary(data).map(|kind| match kind {
        ElementaryKind::AdtsAac => ContainerKind::Adts,
        ElementaryKind::Mp3 => ContainerKind::Mp3,
    })
}

/// ADTS AAC или MP3 после дополнительного тега ID3v2.
fn sniff_elementary(data: &[u8]) -> Option<ElementaryKind> {
    let start = id3_length(data).unwrap_or(0);
    let head = data.get(start..)?;
    if head.len() < 2 || head[0] != 0xFF {
        return None;
    }
    // ADTS: синхронное слово 0xFFF, биты уровня 00. Звук MPEG: синхрослово 0xFFE, уровень != 00.
    match (head[1] & 0xF6, head[1] & 0xE0) {
        (0xF0, _) => Some(ElementaryKind::AdtsAac),
        (_, 0xE0) if head[1] & 0x06 != 0 => Some(ElementaryKind::Mp3),
        _ => None,
    }
}

/// Общий размер ведущего тега ID3v2 (включая заголовок), если он есть.
fn id3_length(data: &[u8]) -> Option<usize> {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return None;
    }
    let size = data[6..10].iter().try_fold(0usize, |acc, b| (b & 0x80 == 0).then_some((acc << 7) | *b as usize))?;
    let footer = if data[5] & 0x10 != 0 { 10 } else { 0 };
    Some(10 + size + footer)
}

fn strip_id3_prefix(bytes: &mut Vec<u8>) {
    while let Some(length) = id3_length(bytes) {
        if length > bytes.len() {
            bytes.clear();
            return;
        }
        bytes.drain(..length);
    }
}

fn demux_ts(data: &[u8]) -> Result<(ElementaryKind, Vec<u8>)> {
    if data.is_empty() || data.len() > i32::MAX as usize / 2 {
        return Err(Error::Stream("cannot demux this segment".into()));
    }
    let mut out = vec![0u8; data.len()];
    let mut scratch = vec![0u8; data.len()];
    let mut produced: i32 = 0;
    // SAFETY: все буферы действительны для переданных мощностей.
    let kind = unsafe {
        pm_mpegts_extract_audio(
            data.as_ptr(),
            data.len() as i32,
            out.as_mut_ptr(),
            out.len() as i32,
            scratch.as_mut_ptr(),
            scratch.len() as i32,
            &mut produced,
        )
    };
    if produced <= 32 {
        return Err(Error::Stream("transport stream segment has no audio".into()));
    }
    out.truncate(produced as usize);
    match kind {
        PM_MPEGTS_KIND_ADTS => Ok((ElementaryKind::AdtsAac, out)),
        PM_MPEGTS_KIND_MP3 => Ok((ElementaryKind::Mp3, out)),
        _ => Err(Error::Stream("unsupported audio in transport stream".into())),
    }
}

#[cfg(test)]
mod tests {
    /// Будущее преобразователя выполняет свою работу синхронно; опросите его один раз.
    fn resolve_now(resolver: &PublicResolver, host: &str) -> std::result::Result<Vec<std::net::SocketAddr>, String> {
        use reqwest::dns::Resolve;
        use std::str::FromStr;
        use std::task::{Context, Poll, Waker};
        let name = reqwest::dns::Name::from_str(host).unwrap();
        let mut future = resolver.resolve(name);
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(Ok(addrs)) => Ok(addrs.collect()),
            Poll::Ready(Err(e)) => Err(e.to_string()),
            Poll::Pending => Err("pending".into()),
        }
    }

    #[test]
    fn names_that_resolve_to_local_addresses_are_refused() {
        let strict = PublicResolver { allow_loopback: false };
        let error = resolve_now(&strict, "localhost").unwrap_err();
        assert!(error.contains("public address"), "{error}");
        // Тесты (и только тесты) могут взаимодействовать с локальным сервером.
        let lab = PublicResolver { allow_loopback: true };
        assert!(resolve_now(&lab, "localhost").unwrap().iter().all(|a| a.ip().is_loopback()));
    }

    #[test]
    fn local_ip_ranges() {
        for local in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.1.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:192.168.0.1"] {
            assert!(is_local_ip(local.parse().unwrap()), "{local}");
        }
        for public in ["93.184.216.34", "2a00:1450::1", "8.8.8.8"] {
            assert!(!is_local_ip(public.parse().unwrap()), "{public}");
        }
    }

    use super::*;

    #[test]
    fn playlist_sniffing() {
        assert!(looks_like_playlist(b"#EXTM3U\n#EXT-X-VERSION:3"));
        assert!(looks_like_playlist(b"\xEF\xBB\xBF\n#EXTM3U"));
        assert!(!looks_like_playlist(b"ID3\x04\x00"));
        assert!(!looks_like_playlist(b""));
    }

    #[test]
    fn default_iv_is_the_big_endian_sequence() {
        let iv = sequence_iv(0x0102);
        assert_eq!(&iv[..8], &[0; 8]);
        assert_eq!(&iv[8..], &[0, 0, 0, 0, 0, 0, 1, 2]);
        assert_eq!(sequence_iv(-5), [0; 16]);
    }

    #[test]
    fn container_detection() {
        let mut ts = vec![0u8; 188 * 3];
        for packet in 0..3 {
            ts[packet * 188] = 0x47;
        }
        assert_eq!(detect_container(&ts), Some(ContainerKind::MpegTs));
        assert_eq!(detect_container(&[0xFF, 0xF1, 0x50, 0x80]), Some(ContainerKind::Adts));
        assert_eq!(detect_container(&[0xFF, 0xFB, 0x90, 0x00]), Some(ContainerKind::Mp3));
        assert_eq!(detect_container(b"<html>"), None);
    }

    #[test]
    fn id3_tags_are_stripped_even_when_stacked() {
        let mut tag = b"ID3\x04\x00\x00\x00\x00\x00\x03abc".to_vec();
        let mut data = tag.clone();
        data.append(&mut tag);
        data.extend_from_slice(&[0xFF, 0xF1, 0x50]);
        strip_id3_prefix(&mut data);
        assert_eq!(data, vec![0xFF, 0xF1, 0x50]);
        // Тег, который заявляет больше, чем содержит данные, ничего не оставляет.
        let mut lying = b"ID3\x04\x00\x00\x00\x00\x7f\x7fxx".to_vec();
        strip_id3_prefix(&mut lying);
        assert!(lying.is_empty());
    }

    #[test]
    fn hostile_addresses_are_refused() {
        let config = FetchConfig::new("ua");
        for bad in ["http://cdn.example/a", "file:///etc/passwd", "ftp://x/y", "https://user:pw@cdn.example/a", "javascript:alert(1)",
            "https://127.0.0.1/a", "https://192.168.1.1/a", "https://10.0.0.5/a", "https://169.254.169.254/a",
            "https://[::1]/a", "https://[fd00::1]/a", "https://[::ffff:10.0.0.1]/a", "https://localhost/a",
            "https://x.localhost/a"] {
            assert!(parse_url(bad, &config).is_err(), "{bad}");
        }
        assert!(parse_url("https://cdn.example/a/index.m3u8", &config).is_ok());
        assert!(parse_url("http://127.0.0.1:9/a", &config).is_err(), "loopback http is test-only");
        let mut lab = FetchConfig::new("ua");
        lab.allow_loopback_http = true;
        assert!(parse_url("http://127.0.0.1:9/a", &lab).is_ok());
        assert!(parse_url("http://evil.example/a", &lab).is_err());
    }

    #[test]
    fn slices_are_bounds_checked() {
        let text = "héllo";
        assert_eq!(slice_text(text, pm_hls_slice { offset: 0, length: 1 }), Some("h"));
        // Внутри двухбайтового é: не граница символов.
        assert_eq!(slice_text(text, pm_hls_slice { offset: 2, length: 1 }), None);
        assert_eq!(slice_text(text, pm_hls_slice { offset: 0, length: 99 }), None);
        assert_eq!(slice_text(text, pm_hls_slice { offset: 0, length: -1 }), None);
    }
}
