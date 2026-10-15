//! HLS playlist parsing: audio rendition / variant selection from a master
//! playlist and segment, key and map extraction from a media playlist.
//!
//! The behaviour (including its quirks) follows the Swift parser it replaces
//! and is compared with it on generated playlists. Strings never cross the
//! boundary: results point into the caller's UTF-8 text as byte offsets, and
//! URL resolution (Foundation, RFC 3986) stays on the Swift side.

use core::slice;
use std::collections::HashMap;

pub const PM_HLS_OK: i32 = 0;
pub const PM_HLS_INVALID_ENCRYPTION_KEY: i32 = 1;
pub const PM_HLS_UNSUPPORTED_ENCRYPTION_METHOD: i32 = 2;
pub const PM_HLS_INVALID_INITIALIZATION_SECTION: i32 = 3;
pub const PM_HLS_INVALID_BYTE_RANGE: i32 = 4;
pub const PM_HLS_ENCRYPTED_INIT_NEEDS_IV: i32 = 5;
pub const PM_HLS_TOO_MANY_SEGMENTS: i32 = 6;
pub const PM_HLS_INVALID_TEXT: i32 = -1;

pub const PM_HLS_CHOOSE_MEDIA_PLAYLIST: i32 = 0;
pub const PM_HLS_CHOOSE_FOUND: i32 = 1;
pub const PM_HLS_CHOOSE_NONE: i32 = 2;

/// Byte range inside the parsed text. `length < 0` means absent.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct pm_hls_slice {
    pub offset: i32,
    pub length: i32,
}

impl pm_hls_slice {
    const ABSENT: pm_hls_slice = pm_hls_slice { offset: 0, length: -1 };
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct pm_hls_key {
    pub uri: pm_hls_slice,
    pub has_iv: bool,
    pub iv: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct pm_hls_map {
    pub uri: pm_hls_slice,
    pub has_range: bool,
    pub range_length: i64,
    pub has_range_offset: bool,
    pub range_offset: i64,
    /// Index into the key table, or -1.
    pub key_index: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct pm_hls_segment {
    pub uri: pm_hls_slice,
    pub has_range: bool,
    pub range_length: i64,
    pub has_range_offset: bool,
    pub range_offset: i64,
    pub key_index: i32,
    pub map_index: i32,
    pub discontinuity: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct pm_hls_media_info {
    pub code: i32,
    pub media_sequence: i64,
    pub segment_count: i32,
    pub key_count: i32,
    pub map_count: i32,
    /// For `PM_HLS_UNSUPPORTED_ENCRYPTION_METHOD`: the method text.
    pub detail: pm_hls_slice,
}

// ------------------------------------------------------------------ text

/// `CharacterSet.newlines`: U+000A–U+000D, U+0085, U+2028, U+2029.
fn is_newline(c: char) -> bool {
    matches!(c, '\u{A}'..='\u{D}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn offset_in(base: &str, part: &str) -> i32 {
    (part.as_ptr() as usize - base.as_ptr() as usize) as i32
}

fn slice_of(base: &str, part: &str) -> pm_hls_slice {
    pm_hls_slice { offset: offset_in(base, part), length: part.len() as i32 }
}

/// Swift `dropFirst(n)` counts Characters; for the ASCII prefixes used here
/// counting chars is equivalent apart from combining marks right after them.
fn drop_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

fn int(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

/// Quote-aware attribute list; later duplicates win, keys are upper-cased,
/// values trimmed and unquoted.
fn attribute_list(raw: &str) -> HashMap<String, &str> {
    let mut tokens: Vec<&str> = Vec::new();
    let mut inside = false;
    let mut start = 0usize;
    for (i, c) in raw.char_indices() {
        if c == '"' {
            inside = !inside;
        } else if c == ',' && !inside {
            tokens.push(&raw[start..i]);
            start = i + 1;
        }
    }
    if start < raw.len() {
        tokens.push(&raw[start..]);
    }
    let mut result = HashMap::new();
    for token in tokens {
        let Some(eq) = token.find('=') else { continue };
        let key = token[..eq].trim().to_uppercase();
        let mut value = token[eq + 1..].trim();
        if value.starts_with('"') && value.ends_with('"') && value.chars().count() >= 2 {
            value = &value[1..value.len() - 1];
        }
        result.insert(key, value);
    }
    result
}

/// `value.split("@")` with empty parts dropped, then Int() of the first two.
fn byte_range_parts(value: &str) -> (Option<i64>, Option<i64>) {
    let mut parts = value.split('@').filter(|s| !s.is_empty());
    let first = parts.next();
    let length = first.and_then(int);
    let offset = parts.next().and_then(int);
    (length, offset)
}

// ----------------------------------------------------------- master playlist

struct Variant<'a> {
    uri: &'a str,
    bandwidth: i64,
    codecs: Vec<&'a str>,
    audio_group: Option<&'a str>,
}

struct Rendition<'a> {
    group: &'a str,
    uri: &'a str,
    is_default: bool,
    is_autoselect: bool,
}

fn parse_variants(text: &str) -> Vec<Variant<'_>> {
    let mut variants = Vec::new();
    let mut pending: Option<(i64, Vec<&str>, Option<&str>)> = None;
    for raw in text.split(is_newline) {
        let line = raw.trim();
        if line.starts_with("#EXT-X-STREAM-INF") {
            let attrs = attribute_list(drop_chars(line, "#EXT-X-STREAM-INF:".len()));
            let bandwidth = attrs.get("BANDWIDTH").and_then(|v| int(v)).unwrap_or(0);
            let codecs = attrs
                .get("CODECS")
                .map(|v| v.split(',').filter(|s| !s.is_empty()).map(str::trim).collect())
                .unwrap_or_default();
            pending = Some((bandwidth, codecs, attrs.get("AUDIO").copied()));
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((bandwidth, codecs, audio_group)) = pending.take() {
            variants.push(Variant { uri: line, bandwidth, codecs, audio_group });
        }
    }
    variants
}

fn parse_renditions(text: &str) -> Vec<Rendition<'_>> {
    let mut out = Vec::new();
    for raw in text.split(is_newline) {
        let line = raw.trim();
        if !line.starts_with("#EXT-X-MEDIA:") {
            continue;
        }
        let attrs = attribute_list(&line["#EXT-X-MEDIA:".len()..]);
        let is_audio = attrs.get("TYPE").is_some_and(|t| t.to_uppercase() == "AUDIO");
        let (Some(group), Some(uri)) = (attrs.get("GROUP-ID"), attrs.get("URI")) else { continue };
        if !is_audio {
            continue;
        }
        out.push(Rendition {
            group,
            uri,
            is_default: attrs.get("DEFAULT") == Some(&"YES"),
            is_autoselect: attrs.get("AUTOSELECT") == Some(&"YES"),
        });
    }
    out
}

/// Selection order: DEFAULT rendition, AUTOSELECT rendition, first rendition
/// in a group some variant references, then the best audio-only variant.
pub fn choose_audio(text: &str) -> (i32, pm_hls_slice) {
    let renditions = parse_renditions(text);
    let variants = parse_variants(text);
    if variants.is_empty() && renditions.is_empty() {
        return (PM_HLS_CHOOSE_MEDIA_PLAYLIST, pm_hls_slice::ABSENT);
    }
    let found = |uri: &str| (PM_HLS_CHOOSE_FOUND, slice_of(text, uri));
    if let Some(r) = renditions.iter().find(|r| r.is_default) {
        return found(r.uri);
    }
    if let Some(r) = renditions.iter().find(|r| r.is_autoselect) {
        return found(r.uri);
    }
    if let Some(r) = renditions
        .iter()
        .find(|r| variants.iter().any(|v| v.audio_group == Some(r.group)))
    {
        return found(r.uri);
    }
    let audio_prefixes = ["mp4a", "aac", "ac-3", "ec-3", "mp3"];
    let video_prefixes = ["avc1", "hvc1", "hev1", "vp9", "av01"];
    let mut best: Option<&Variant> = None;
    for v in &variants {
        let audio = v.codecs.iter().any(|c| audio_prefixes.iter().any(|p| c.starts_with(p)));
        let video = v.codecs.iter().any(|c| video_prefixes.iter().any(|p| c.starts_with(p)));
        if !(audio && !video) {
            continue;
        }
        // `max(by:)` keeps the first of equal elements.
        if best.map_or(true, |b| b.bandwidth < v.bandwidth) {
            best = Some(v);
        }
    }
    match best {
        Some(v) => found(v.uri),
        None => (PM_HLS_CHOOSE_NONE, pm_hls_slice::ABSENT),
    }
}

// ----------------------------------------------------------- media playlist

pub struct MediaPlaylist {
    pub code: i32,
    pub media_sequence: i64,
    pub detail: pm_hls_slice,
    pub segments: Vec<pm_hls_segment>,
    pub keys: Vec<pm_hls_key>,
    pub maps: Vec<pm_hls_map>,
}

fn parse_iv(raw: &str) -> Option<[u8; 16]> {
    let mut hex: &str = raw;
    if hex.starts_with("0x") || hex.starts_with("0X") {
        hex = &hex[2..];
    }
    let mut chars: Vec<char> = hex.chars().collect();
    if chars.len() % 2 == 1 {
        chars.insert(0, '0');
    }
    let mut bytes: Vec<u8> = Vec::new();
    for pair in chars.chunks(2) {
        let s: String = pair.iter().collect();
        if let Ok(b) = u8::from_str_radix(&s, 16) {
            bytes.push(b);
        }
    }
    <[u8; 16]>::try_from(bytes.as_slice()).ok()
}

pub fn parse_media(text: &str, max_segments: i32) -> MediaPlaylist {
    let mut out = MediaPlaylist {
        code: PM_HLS_OK,
        media_sequence: 0,
        detail: pm_hls_slice::ABSENT,
        segments: Vec::new(),
        keys: Vec::new(),
        maps: Vec::new(),
    };
    let mut current_key: i32 = -1;
    let mut pending_range: Option<(i64, Option<i64>)> = None;
    let mut pending_map: i32 = -1;
    let mut discontinuity = false;

    for raw in text.split(is_newline) {
        let line = raw.trim();
        if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
            out.media_sequence = int(&line.replace("#EXT-X-MEDIA-SEQUENCE:", "")).unwrap_or(0);
        } else if line.starts_with("#EXT-X-KEY:") {
            let attrs = attribute_list(&line["#EXT-X-KEY:".len()..]);
            let Some(method) = attrs.get("METHOD") else {
                out.code = PM_HLS_INVALID_ENCRYPTION_KEY;
                return out;
            };
            match method.to_uppercase().as_str() {
                "NONE" => current_key = -1,
                "AES-128" => {
                    let Some(uri) = attrs.get("URI") else {
                        out.code = PM_HLS_INVALID_ENCRYPTION_KEY;
                        return out;
                    };
                    let iv = attrs.get("IV").and_then(|v| parse_iv(v));
                    out.keys.push(pm_hls_key {
                        uri: slice_of(text, uri),
                        has_iv: iv.is_some(),
                        iv: iv.unwrap_or([0; 16]),
                    });
                    current_key = out.keys.len() as i32 - 1;
                }
                _ => {
                    out.code = PM_HLS_UNSUPPORTED_ENCRYPTION_METHOD;
                    out.detail = slice_of(text, method);
                    return out;
                }
            }
        } else if line.starts_with("#EXT-X-BYTERANGE:") {
            let value = line.replace("#EXT-X-BYTERANGE:", "");
            let (length, offset) = byte_range_parts(&value);
            pending_range = length.map(|l| (l, offset));
        } else if line.starts_with("#EXT-X-MAP:") {
            let attrs = attribute_list(&line["#EXT-X-MAP:".len()..]);
            let Some(uri) = attrs.get("URI") else {
                out.code = PM_HLS_INVALID_INITIALIZATION_SECTION;
                return out;
            };
            let mut range = None;
            if let Some(raw_range) = attrs.get("BYTERANGE") {
                let (length, offset) = byte_range_parts(raw_range);
                let Some(length) = length else {
                    out.code = PM_HLS_INVALID_BYTE_RANGE;
                    return out;
                };
                range = Some((length, offset));
            }
            if current_key >= 0 && !out.keys[current_key as usize].has_iv {
                out.code = PM_HLS_ENCRYPTED_INIT_NEEDS_IV;
                return out;
            }
            out.maps.push(pm_hls_map {
                uri: slice_of(text, uri),
                has_range: range.is_some(),
                range_length: range.map_or(0, |r| r.0),
                has_range_offset: range.is_some_and(|r| r.1.is_some()),
                range_offset: range.and_then(|r| r.1).unwrap_or(0),
                key_index: current_key,
            });
            pending_map = out.maps.len() as i32 - 1;
        } else if line.starts_with("#EXT-X-DISCONTINUITY") {
            discontinuity = true;
        } else if !line.starts_with('#') && !line.is_empty() {
            if out.segments.len() as i64 >= i64::from(max_segments) {
                out.code = PM_HLS_TOO_MANY_SEGMENTS;
                return out;
            }
            out.segments.push(pm_hls_segment {
                uri: slice_of(text, line),
                has_range: pending_range.is_some(),
                range_length: pending_range.map_or(0, |r| r.0),
                has_range_offset: pending_range.is_some_and(|r| r.1.is_some()),
                range_offset: pending_range.and_then(|r| r.1).unwrap_or(0),
                key_index: current_key,
                map_index: pending_map,
                discontinuity,
            });
            pending_range = None;
            discontinuity = false;
        }
    }
    out
}

// ------------------------------------------------------------------ C ABI

unsafe fn text_of<'a>(ptr: *const u8, len: i32) -> Option<&'a str> {
    if ptr.is_null() || len < 0 {
        return None;
    }
    core::str::from_utf8(slice::from_raw_parts(ptr, len as usize)).ok()
}

/// Picks the audio playlist URI out of a master playlist. Returns
/// `PM_HLS_CHOOSE_*`; `uri` points into `text` when found.
#[no_mangle]
pub unsafe extern "C" fn pm_hls_choose_audio(text: *const u8, length: i32, uri: *mut pm_hls_slice) -> i32 {
    let Some(t) = text_of(text, length) else { return PM_HLS_INVALID_TEXT };
    let (code, found) = choose_audio(t);
    if !uri.is_null() {
        *uri = found;
    }
    code
}

/// Parses a media playlist. Tables are filled up to their capacities while
/// `info` always reports the full counts, so a first call with null tables
/// sizes the second. Returns `info.code`.
#[no_mangle]
pub unsafe extern "C" fn pm_hls_parse_media(
    text: *const u8,
    length: i32,
    max_segments: i32,
    info: *mut pm_hls_media_info,
    segments: *mut pm_hls_segment,
    segment_capacity: i32,
    keys: *mut pm_hls_key,
    key_capacity: i32,
    maps: *mut pm_hls_map,
    map_capacity: i32,
) -> i32 {
    let Some(t) = text_of(text, length) else {
        if !info.is_null() {
            *info = pm_hls_media_info {
                code: PM_HLS_INVALID_TEXT,
                media_sequence: 0,
                segment_count: 0,
                key_count: 0,
                map_count: 0,
                detail: pm_hls_slice::ABSENT,
            };
        }
        return PM_HLS_INVALID_TEXT;
    };
    let parsed = parse_media(t, max_segments);
    fn copy<T: Copy>(from: &[T], to: *mut T, capacity: i32) {
        if to.is_null() || capacity <= 0 {
            return;
        }
        let n = from.len().min(capacity as usize);
        unsafe { slice::from_raw_parts_mut(to, n).copy_from_slice(&from[..n]) };
    }
    copy(&parsed.segments, segments, segment_capacity);
    copy(&parsed.keys, keys, key_capacity);
    copy(&parsed.maps, maps, map_capacity);
    if !info.is_null() {
        *info = pm_hls_media_info {
            code: parsed.code,
            media_sequence: parsed.media_sequence,
            segment_count: parsed.segments.len() as i32,
            key_count: parsed.keys.len() as i32,
            map_count: parsed.maps.len() as i32,
            detail: parsed.detail,
        };
    }
    parsed.code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri<'a>(t: &'a str, s: pm_hls_slice) -> &'a str {
        &t[s.offset as usize..(s.offset + s.length) as usize]
    }

    #[test]
    fn picks_default_rendition_first() {
        let t = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"x\",URI=\"low.m3u8\"\n\
                 #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",DEFAULT=YES,URI=\"main.m3u8\"\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=100,AUDIO=\"a\"\nv.m3u8\n";
        let (code, s) = choose_audio(t);
        assert_eq!(code, PM_HLS_CHOOSE_FOUND);
        assert_eq!(uri(t, s), "main.m3u8");
    }

    #[test]
    fn falls_back_to_the_best_audio_only_variant() {
        let t = "#EXTM3U\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=900,CODECS=\"avc1.4d401f, mp4a.40.2\"\nvideo.m3u8\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=64,CODECS=\"mp4a.40.5\"\nlow.m3u8\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=128,CODECS=\"mp4a.40.2\"\nhigh.m3u8\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=128,CODECS=\"mp4a.40.2\"\ntie.m3u8\n";
        let (code, s) = choose_audio(t);
        assert_eq!((code, uri(t, s)), (PM_HLS_CHOOSE_FOUND, "high.m3u8"));
        assert_eq!(choose_audio("#EXTM3U\nseg.ts\n").0, PM_HLS_CHOOSE_MEDIA_PLAYLIST);
        let video_only = "#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"avc1.1\"\nv.m3u8\n";
        assert_eq!(choose_audio(video_only).0, PM_HLS_CHOOSE_NONE);
    }

    #[test]
    fn media_playlist_keys_maps_ranges_and_discontinuities() {
        let t = "#EXTM3U\r\n#EXT-X-MEDIA-SEQUENCE:7\r\n\
                 #EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\",IV=0x000102030405060708090A0B0C0D0E0F\r\n\
                 #EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"720@0\"\r\n\
                 #EXT-X-BYTERANGE:100@200\r\na.m4s\r\n\
                 #EXT-X-DISCONTINUITY\r\n#EXT-X-KEY:METHOD=NONE\r\nb.m4s\r\n";
        let m = parse_media(t, 10);
        assert_eq!(m.code, PM_HLS_OK);
        assert_eq!(m.media_sequence, 7);
        assert_eq!((m.segments.len(), m.keys.len(), m.maps.len()), (2, 1, 1));
        assert_eq!(uri(t, m.keys[0].uri), "k.bin");
        assert!(m.keys[0].has_iv);
        assert_eq!(m.keys[0].iv[15], 0x0F);
        let (a, b) = (&m.segments[0], &m.segments[1]);
        assert_eq!((a.has_range, a.range_length, a.has_range_offset, a.range_offset), (true, 100, true, 200));
        assert_eq!((a.key_index, a.map_index, a.discontinuity), (0, 0, false));
        // The map persists, the key was cleared, the discontinuity flag applies once.
        assert_eq!((b.key_index, b.map_index, b.discontinuity, b.has_range), (-1, 0, true, false));
        assert!(m.maps[0].has_range && m.maps[0].has_range_offset);
        assert_eq!(m.maps[0].key_index, 0);
    }

    #[test]
    fn rejects_what_the_swift_parser_rejected() {
        let code = |t: &str| parse_media(t, 3).code;
        assert_eq!(code("#EXT-X-KEY:URI=\"k\"\n"), PM_HLS_INVALID_ENCRYPTION_KEY);
        assert_eq!(code("#EXT-X-KEY:METHOD=AES-128\n"), PM_HLS_INVALID_ENCRYPTION_KEY);
        assert_eq!(code("#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n"), PM_HLS_UNSUPPORTED_ENCRYPTION_METHOD);
        assert_eq!(code("#EXT-X-MAP:BYTERANGE=\"5\"\n"), PM_HLS_INVALID_INITIALIZATION_SECTION);
        assert_eq!(code("#EXT-X-MAP:URI=\"i\",BYTERANGE=\"x\"\n"), PM_HLS_INVALID_BYTE_RANGE);
        // Encrypted map without an explicit IV.
        let t = "#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXT-X-MAP:URI=\"i\"\n";
        assert_eq!(code(t), PM_HLS_ENCRYPTED_INIT_NEEDS_IV);
        assert_eq!(code("a.ts\nb.ts\nc.ts\nd.ts\n"), PM_HLS_TOO_MANY_SEGMENTS);
        assert_eq!(code("a.ts\nb.ts\nc.ts\n"), PM_HLS_OK);
    }

    #[test]
    fn c_abi_sizes_then_fills() {
        let t = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\ns1.ts\ns2.ts\n";
        let mut info = pm_hls_media_info {
            code: -9,
            media_sequence: 0,
            segment_count: 0,
            key_count: 0,
            map_count: 0,
            detail: pm_hls_slice::ABSENT,
        };
        let sized = unsafe {
            pm_hls_parse_media(
                t.as_ptr(), t.len() as i32, 100, &mut info,
                core::ptr::null_mut(), 0, core::ptr::null_mut(), 0, core::ptr::null_mut(), 0,
            )
        };
        assert_eq!((sized, info.segment_count, info.key_count, info.map_count), (PM_HLS_OK, 2, 1, 0));
        let mut segs = vec![
            pm_hls_segment {
                uri: pm_hls_slice::ABSENT, has_range: false, range_length: 0, has_range_offset: false,
                range_offset: 0, key_index: 0, map_index: 0, discontinuity: false,
            };
            1
        ];
        // A short table is filled as far as it goes; the counts stay complete.
        unsafe {
            pm_hls_parse_media(
                t.as_ptr(), t.len() as i32, 100, &mut info,
                segs.as_mut_ptr(), 1, core::ptr::null_mut(), 0, core::ptr::null_mut(), 0,
            )
        };
        assert_eq!(info.segment_count, 2);
        assert_eq!(uri(t, segs[0].uri), "s1.ts");
        let mut bad = pm_hls_slice::ABSENT;
        assert_eq!(unsafe { pm_hls_choose_audio(core::ptr::null(), 0, &mut bad) }, PM_HLS_INVALID_TEXT);
    }
}
