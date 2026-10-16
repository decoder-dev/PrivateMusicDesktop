//! Song lyrics, time-synced when a source has them.
//!
//! Sources, in order: LRCLIB (public, matches artist, title and duration, and
//! has timestamps), then VK's own text for the track's `lyrics_id` (plain),
//! then whatever plain text LRCLIB had. A synced result is trusted only when
//! the recording's length agrees: an edit or a live take shares the words but
//! not the timeline.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::vk::Track;
use crate::{Error, Result};

pub const LRCLIB_BASE: &str = "https://lrclib.net/";
/// How far the recording's length may differ from the lyrics' timeline.
const SYNC_TOLERANCE_SECONDS: f64 = 3.0;
const MAX_BODY: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    pub time: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lyrics {
    pub source: String,
    pub text: String,
    /// Empty when the lyrics are not synced.
    pub lines: Vec<Line>,
}

/// Parses LRC: `[mm:ss.xx] words`, several stamps per line allowed, `[ar:…]`
/// style tags ignored. Result is sorted by time; empty lyric lines are kept
/// (they are the instrumental gaps) but a leading run of them is dropped.
pub fn parse_lrc(raw: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for source in raw.lines() {
        let mut rest = source.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            match parse_stamp(&stripped[..end]) {
                Some(time) => stamps.push(time),
                None if stamps.is_empty() => break, // a tag such as [ar:Name]
                None => {}
            }
            rest = stripped[end + 1..].trim_start();
        }
        let text = rest.trim();
        for time in stamps {
            lines.push(Line { time, text: text.to_string() });
        }
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    while lines.first().is_some_and(|l| l.text.is_empty()) {
        lines.remove(0);
    }
    lines
}

fn parse_stamp(text: &str) -> Option<f64> {
    let (minutes, seconds) = text.split_once(':')?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().parse().ok()?;
    if !seconds.is_finite() || !(0.0..60.0).contains(&seconds) {
        return None;
    }
    Some(f64::from(minutes) * 60.0 + seconds)
}

/// Lower-case words of a title, without "(…)"/"[…]" parts, a " - Remastered"
/// style suffix or a "feat." tail. Punctuation inside the title is kept as a
/// word break: "Mr. Brightside" is "mr brightside", not "mr".
fn normalize(text: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0u32;
    for ch in text.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            c => plain.extend(c.to_lowercase()),
        }
    }
    let mut cut = plain.len();
    for marker in [" - ", " – ", " — ", " feat.", " feat ", " ft. ", " ft "] {
        if let Some(i) = plain.find(marker) {
            cut = cut.min(i);
        }
    }
    plain[..cut]
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn artists(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    lowered
        .split([',', '&', '/', ';'])
        .flat_map(|part| part.split(" feat.").flat_map(|p| p.split(" feat ")).flat_map(|p| p.split(" ft. ")).flat_map(|p| p.split(" x ")).map(str::to_string).collect::<Vec<_>>())
        .map(|a| normalize(&a))
        .filter(|a| !a.is_empty())
        .collect()
}

/// How well a candidate fits the track: `None` when it is a different song.
fn score(track: &Track, candidate: &Candidate) -> Option<u32> {
    let wanted = normalize(&track.title);
    let found = normalize(candidate.track_name.as_deref().unwrap_or(""));
    if wanted.is_empty() || found.is_empty() {
        return None;
    }
    let title = if wanted == found {
        3
    } else if wanted.contains(&found) || found.contains(&wanted) {
        1
    } else {
        return None;
    };
    let ours = artists(&track.artist);
    let theirs = artists(candidate.artist_name.as_deref().unwrap_or(""));
    let artist = if ours.iter().any(|a| theirs.contains(a)) {
        3
    } else if ours.iter().any(|a| theirs.iter().any(|b| a.contains(b.as_str()) || b.contains(a.as_str()))) {
        1
    } else {
        return None;
    };
    Some(title + artist)
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    track_name: Option<String>,
    artist_name: Option<String>,
    duration: Option<f64>,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

impl Candidate {
    fn has_text(&self) -> bool {
        let filled = |s: &Option<String>| s.as_deref().is_some_and(|s| !s.trim().is_empty());
        !self.instrumental && (filled(&self.plain_lyrics) || filled(&self.synced_lyrics))
    }
}

fn best(track: &Track, candidates: Vec<Candidate>) -> Option<Lyrics> {
    let length = f64::from(track.duration);
    let mut ranked: Vec<(u32, f64, Candidate)> = candidates
        .into_iter()
        .filter(Candidate::has_text)
        .filter_map(|c| {
            let fit = score(track, &c)?;
            let gap = c.duration.filter(|d| d.is_finite()).map_or(f64::MAX, |d| (d - length).abs());
            Some((fit, gap, c))
        })
        .collect();
    // Best fit first; among equals the one whose length is closest.
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
    let (_, gap, chosen) = ranked.into_iter().next()?;
    let synced = chosen.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default();
    let in_step = length > 0.0 && gap <= SYNC_TOLERANCE_SECONDS;
    let lines = if in_step && !synced.is_empty() { synced.clone() } else { Vec::new() };
    let text = if lines.is_empty() {
        chosen
            .plain_lyrics
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().to_string())
            .or_else(|| (!synced.is_empty()).then(|| synced.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")))?
    } else {
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    };
    Some(Lyrics { source: "LRCLIB".into(), text, lines })
}

pub fn from_plain(source: &str, text: &str) -> Option<Lyrics> {
    let text = text.trim();
    (!text.is_empty()).then(|| Lyrics { source: source.into(), text: text.into(), lines: Vec::new() })
}

pub struct LrcLib {
    http: Client,
    base: Url,
}

impl LrcLib {
    pub fn new() -> Result<Self> {
        Self::with_base(LRCLIB_BASE)
    }

    /// https only; plain http for a loopback host so tests can use a local server.
    pub fn with_base(base: &str) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| Error::InvalidRequest)?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "localhost"));
        if base.scheme() != "https" && !(loopback && base.scheme() == "http") {
            return Err(Error::InvalidRequest);
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(10))
            // A fixed host: nothing it says may send us elsewhere.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("PrivateMusicDesktop (https://github.com/decoder-dev/PrivateMusicDesktop)")
            .build()?;
        Ok(Self { http, base })
    }

    fn fetch(&self, path: &str, query: &[(&str, String)]) -> Result<Option<Vec<Candidate>>> {
        let mut url = self.base.join(path).map_err(|_| Error::InvalidRequest)?;
        url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        let response = self.http.get(url).header("Accept", "application/json").send()?;
        match response.status().as_u16() {
            404 => return Ok(None),
            200..=299 => {}
            other => return Err(Error::Http(other)),
        }
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(response, MAX_BODY + 1), &mut body)
            .map_err(|e| Error::Transport(e.to_string()))?;
        if body.len() as u64 > MAX_BODY {
            return Err(Error::Format("lyrics response is too large".into()));
        }
        if let Ok(list) = serde_json::from_slice::<Vec<Candidate>>(&body) {
            return Ok(Some(list));
        }
        serde_json::from_slice::<Candidate>(&body).map(|c| Some(vec![c])).map_err(|_| Error::Format("lyrics response".into()))
    }

    /// `Ok(None)` when LRCLIB has nothing that fits this track.
    pub fn lookup(&self, track: &Track) -> Result<Option<Lyrics>> {
        if track.title.trim().is_empty() {
            return Ok(None);
        }
        // Exact lookup by artist, title and length first: it finds the right recording.
        if track.duration > 0 {
            let mut query = vec![
                ("track_name", track.title.clone()),
                ("artist_name", track.artist.clone()),
                ("duration", track.duration.to_string()),
            ];
            if let Some(album) = track.album_title.as_deref().filter(|a| !a.is_empty()) {
                query.push(("album_name", album.to_string()));
            }
            if let Some(found) = self.fetch("api/get", &query)? {
                if let Some(lyrics) = best(track, found) {
                    if !lyrics.lines.is_empty() {
                        return Ok(Some(lyrics));
                    }
                }
            }
        }
        let query = [("track_name", track.title.clone()), ("artist_name", track.artist.clone())];
        Ok(self.fetch("api/search", &query)?.and_then(|found| best(track, found)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: &str, title: &str, duration: u32) -> Track {
        Track {
            track_id: 1, owner_id: 1, title: title.into(), artist: artist.into(), album_title: None, duration,
            stream_url: None, artwork_url: None, access_key: None, lyrics_id: None, is_hq: false, track_code: None,
            artists: Vec::new(), album: None,
        }
    }

    fn candidate(artist: &str, title: &str, duration: f64, plain: Option<&str>, synced: Option<&str>) -> Candidate {
        Candidate {
            track_name: Some(title.into()), artist_name: Some(artist.into()), duration: Some(duration),
            plain_lyrics: plain.map(Into::into), synced_lyrics: synced.map(Into::into), instrumental: false,
        }
    }

    #[test]
    fn punctuation_inside_a_title_is_not_an_end() {
        assert_eq!(normalize("Mr. Brightside"), "mr brightside");
        assert_eq!(normalize("Rock & Roll"), "rock roll");
        assert_eq!(normalize("Song - Remastered 2011"), "song");
        assert_eq!(normalize("Song (feat. X) [Live]"), "song");
        assert_eq!(normalize("Song feat. X"), "song");
        assert_eq!(normalize("Он, она и я"), "он она и я");
        let t = track("The Killers", "Mr. Brightside", 222);
        assert!(score(&t, &candidate("The Killers", "Mr. Brightside", 222.0, Some("x"), None)).is_some());
        assert!(score(&t, &candidate("The Killers", "Mr Something Else", 222.0, Some("x"), None)).is_none());
    }

    #[test]
    fn lrc_stamps_tags_and_repeats() {
        let lines = parse_lrc("[ar:Someone]\n[ti:Song]\n[00:00.00]\n[00:12.50] First\n[01:02.3][00:30]Chorus\n[bad] x\n[00:61.00] no\nplain line");
        assert_eq!(
            lines,
            vec![
                Line { time: 12.5, text: "First".into() },
                Line { time: 30.0, text: "Chorus".into() },
                Line { time: 62.3, text: "Chorus".into() },
            ]
        );
    }

    #[test]
    fn titles_and_artists_are_compared_loosely_but_not_carelessly() {
        let t = track("Скриптонит, Niman", "Это любовь (feat. X)", 200);
        assert!(score(&t, &candidate("Скриптонит", "Это любовь", 200.0, Some("x"), None)).is_some());
        assert!(score(&t, &candidate("Niman & Other", "это ЛЮБОВЬ - Remastered", 200.0, Some("x"), None)).is_some());
        assert!(score(&t, &candidate("Другой", "Это любовь", 200.0, Some("x"), None)).is_none());
        assert!(score(&t, &candidate("Скриптонит", "Совсем другая песня", 200.0, Some("x"), None)).is_none());
        // Exact beats partial.
        let exact = score(&t, &candidate("Скриптонит", "Это любовь", 200.0, Some("x"), None)).unwrap();
        let partial = score(&t, &candidate("Скриптонит", "Это любовь (live)", 200.0, Some("x"), None));
        assert!(partial.is_none_or(|p| p <= exact));
    }

    #[test]
    fn synced_lyrics_are_used_only_when_the_length_agrees() {
        let synced = "[00:01.00] a\n[00:02.00] b";
        let t = track("A", "B", 200);
        let ok = best(&t, vec![candidate("A", "B", 201.0, Some("a\nb"), Some(synced))]).unwrap();
        assert_eq!(ok.lines.len(), 2);
        let off = best(&t, vec![candidate("A", "B", 260.0, Some("a\nb"), Some(synced))]).unwrap();
        assert!(off.lines.is_empty() && off.text == "a\nb", "a different edit keeps the words, not the timeline");
        let unknown_length = best(&track("A", "B", 0), vec![candidate("A", "B", 200.0, None, Some(synced))]).unwrap();
        assert!(unknown_length.lines.is_empty() && unknown_length.text == "a\nb");
    }

    #[test]
    fn the_closest_length_wins_among_equal_matches_and_instrumentals_are_skipped() {
        let t = track("A", "B", 200);
        let mut instrumental = candidate("A", "B", 200.0, None, None);
        instrumental.instrumental = true;
        let far = candidate("A", "B", 240.0, Some("far"), None);
        let near = candidate("A", "B", 199.0, Some("near"), None);
        assert_eq!(best(&t, vec![instrumental, far, near]).unwrap().text, "near");
        assert!(best(&t, vec![]).is_none());
    }

    #[test]
    fn base_must_be_https_or_loopback() {
        assert!(LrcLib::with_base("https://lrclib.net/").is_ok());
        assert!(LrcLib::with_base("http://127.0.0.1:1/").is_ok());
        assert!(LrcLib::with_base("http://lrclib.net/").is_err());
        assert!(LrcLib::with_base("file:///x").is_err());
    }
}
