//! Downloads a VK stream into a [`SharedBuffer`] as one elementary stream.
//!
//! * a plain audio file is copied through;
//! * an HLS playlist is walked segment by segment: AES-128 decrypted with
//!   `pm_native`, MPEG-TS demuxed with `pm_native`, ID3 tags stripped.
//!
//! Every URL that comes out of a playlist is attacker-influenced data, so each
//! one must be https (loopback http only for tests), carry no credentials and
//! stay under the size limits below.

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
use crate::{Error, Result};

const MAX_PLAYLIST_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEGMENT_BYTES: usize = 48 * 1024 * 1024;
const MAX_SEGMENTS: i32 = 4000;
const MAX_PLAYLIST_DEPTH: usize = 3;
pub const MAX_STREAM_BYTES: usize = 150 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct FetchConfig {
    pub user_agent: String,
    /// Tests serve from 127.0.0.1 over plain http. Never true in the app.
    pub allow_loopback_http: bool,
    pub max_stream_bytes: usize,
}

impl FetchConfig {
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self { user_agent: user_agent.into(), allow_loopback_http: false, max_stream_bytes: MAX_STREAM_BYTES }
    }
}

/// Starts the download on its own thread. The buffer is finished, failed or
/// cancelled when the thread ends; callers never need to join it.
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
    http: Client,
    config: &'a FetchConfig,
    buffer: &'a SharedBuffer,
}

fn run(url: &str, config: &FetchConfig, buffer: &SharedBuffer) -> Result<()> {
    let start = parse_url(url, config)?;
    let allow_loopback = config.allow_loopback_http;
    let http = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            let url = attempt.url();
            let ok = (url.scheme() == "https" && !is_local_host(url)) || (allow_loopback && is_loopback(url));
            if attempt.previous().len() >= 3 || !ok {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let fetcher = Fetcher { http, config, buffer };
    fetcher.run(start)
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]" | "::1"))
}

/// Literal local addresses (loopback, private, link-local, unspecified) and
/// `localhost`: a stream address handed over by the window must not be able to
/// make the engine knock on the user's own machine or network.
pub(crate) fn is_local_host(url: &Url) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr};
    fn v4(a: Ipv4Addr) -> bool {
        a.is_loopback() || a.is_private() || a.is_link_local() || a.is_unspecified() || a.is_broadcast()
    }
    fn v6(a: Ipv6Addr) -> bool {
        let first = a.segments()[0];
        a.is_loopback()
            || a.is_unspecified()
            || first & 0xfe00 == 0xfc00 // unique local
            || first & 0xffc0 == 0xfe80 // link local
            || a.to_ipv4_mapped().is_some_and(v4)
    }
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(a)) => v4(a),
        Some(url::Host::Ipv6(a)) => v6(a),
        None => true,
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

    fn send(&self, url: &Url, range: Option<(u64, u64)>) -> Result<Response> {
        let mut last = Error::Offline;
        for attempt in 0..2 {
            self.check_cancel()?;
            let mut request = self.http.get(url.clone()).header(USER_AGENT, self.config.user_agent.as_str());
            if let Some((start, length)) = range {
                request = request.header(RANGE, format!("bytes={}-{}", start, start + length - 1));
            }
            match request.send() {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => last = Error::Http(response.status().as_u16()),
                Err(err) => last = Error::from(err),
            }
            let retryable = matches!(last, Error::Timeout | Error::Offline | Error::Transport(_))
                || matches!(last, Error::Http(408 | 429 | 500..=599));
            if attempt == 0 && retryable {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            break;
        }
        Err(last)
    }

    /// Whole body, capped.
    fn read_all(&self, mut response: Response, cap: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            self.check_cancel()?;
            let n = response.read(&mut chunk).map_err(|e| Error::Transport(e.to_string()))?;
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
        let response = self.send(url, range)?;
        let status = response.status().as_u16();
        let body = self.read_all(response, cap)?;
        match range {
            None => Ok(body),
            Some((start, length)) => {
                if status == 206 {
                    if body.len() as u64 == length { Ok(body) } else { Err(Error::Stream("short range response".into())) }
                } else {
                    // The server ignored Range and sent the whole file.
                    let start = start as usize;
                    let end = start.checked_add(length as usize).ok_or_else(|| Error::Stream("bad byte range".into()))?;
                    body.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| Error::Stream("range outside response".into()))
                }
            }
        }
    }

    fn run(&self, start: Url) -> Result<()> {
        let mut response = self.send(&start, None)?;
        // Sniff: a playlist starts with #EXTM3U, a plain stream with audio.
        let mut head = vec![0u8; 4096];
        let mut filled = 0;
        while filled < 16 {
            self.check_cancel()?;
            let n = response.read(&mut head[filled..]).map_err(|e| Error::Transport(e.to_string()))?;
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
        let kind = sniff_elementary(&head).ok_or_else(|| Error::Stream("unsupported audio format".into()))?;
        let mut first = head;
        strip_id3_prefix(&mut first);
        self.buffer.set_kind(kind);
        if !first.is_empty() && !self.buffer.push(&first) {
            return self.stopped();
        }
        let mut chunk = [0u8; 32 * 1024];
        loop {
            self.check_cancel()?;
            let n = response.read(&mut chunk).map_err(|e| Error::Transport(e.to_string()))?;
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

    /// One segment's bytes: ranged, downloaded and decrypted.
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

    /// Downloads, decrypts and parses one `EXT-X-MAP` initialization section.
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
        // A map range without an offset starts at byte 0, never at the
        // media-segment cursor.
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
            // RFC 8216: an encrypted init section needs an explicit IV.
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

/// RFC 8216 §5.2: the default IV is the media sequence number as a 128-bit
/// big-endian integer.
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
    // SAFETY: every pointer is valid for the length passed with it; key and
    // iv are 16 bytes as the native function requires.
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
    // MPEG-TS: 0x47 sync byte repeating every 188 bytes.
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

/// ADTS AAC or MP3, after an optional ID3v2 tag.
fn sniff_elementary(data: &[u8]) -> Option<ElementaryKind> {
    let start = id3_length(data).unwrap_or(0);
    let head = data.get(start..)?;
    if head.len() < 2 || head[0] != 0xFF {
        return None;
    }
    // ADTS: syncword 0xFFF, layer bits 00. MPEG audio: syncword 0xFFE, layer != 00.
    match (head[1] & 0xF6, head[1] & 0xE0) {
        (0xF0, _) => Some(ElementaryKind::AdtsAac),
        (_, 0xE0) if head[1] & 0x06 != 0 => Some(ElementaryKind::Mp3),
        _ => None,
    }
}

/// Total size of a leading ID3v2 tag (header included), if there is one.
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
    // SAFETY: all buffers are valid for the capacities passed.
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
        // A tag that claims more than the data holds leaves nothing.
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
        // Inside the two-byte é: not a char boundary.
        assert_eq!(slice_text(text, pm_hls_slice { offset: 2, length: 1 }), None);
        assert_eq!(slice_text(text, pm_hls_slice { offset: 0, length: 99 }), None);
        assert_eq!(slice_text(text, pm_hls_slice { offset: 0, length: -1 }), None);
    }
}
