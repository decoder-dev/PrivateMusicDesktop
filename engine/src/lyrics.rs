//! Тексты песен синхронизируются по времени, если они есть у источника.
//!
//! Источники по порядку: LRCLIB (публичный, соответствует исполнителю, названию и продолжительности,
//! имеет метки времени), затем собственный текст VK для `lyrics_id` трека (обычный), затем любой
//! простой текст, который был у LRCLIB. Синхронизированному результату можно доверять только в том
//! случае, если длина записи совпадает: при редактировании или живом дублировании используются слова,
//! но не временная шкала.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::net::{self, Pool};
use crate::vk::Track;
use crate::{Error, Result};

pub const LRCLIB_BASE: &str = "https://lrclib.net/";
pub const GENIUS_BASE: &str = "https://genius.com/";
/// Насколько продолжительность записи может отличаться от временной шкалы текста.
const SYNC_TOLERANCE_SECONDS: f64 = 3.0;
const MAX_BODY: u64 = 2 * 1024 * 1024;
const GENIUS_BODY_LIMIT: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    pub time: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lyrics {
    pub source: String,
    pub text: String,
    /// Пусто, если тексты песен не синхронизированы.
    pub lines: Vec<Line>,
}

/// Разбирает LRC: `[mm:ss.xx] words`, допускается несколько штампов в строке, теги стиля `[ar:…]`
/// игнорируются. Результат сортируется по времени; пустые лирические строки сохранены (это
/// инструментальные пробелы), но основная их часть опущена.
pub fn parse_lrc(raw: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for source in raw.lines() {
        let mut rest = source.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            match parse_stamp(&stripped[..end]) {
                Some(time) => stamps.push(time),
                None if stamps.is_empty() => break, // тег, например [ar:Name]
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

/// Слова заголовка в нижнем регистре, без частей "(…)"/"[…]", суффикса стиля " - Remastered" или хвоста
/// "feat.". Пунктуация внутри заголовка сохраняется в виде разрыва слов: "Mr. Brightside" — это "mr
/// brightside", а не "mr".
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

/// Насколько хорошо кандидат соответствует треку: `None`, когда это другая песня.
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
    // Лучше всего подходит в первую очередь; среди равных тот, длина которого ближе всего.
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
    http: Arc<Pool>,
    base: Url,
}

/// Запасной вариант Public Genius. Результаты поиска принимаются только в том случае, если название и
/// ведущий исполнитель соответствуют запрошенному треку; URLs ограничены `genius.com`, а
/// перенаправления отключены.
pub struct GeniusLyrics {
    http: Arc<Pool>,
    base: Url,
}

impl GeniusLyrics {
    pub fn new() -> Result<Self> {
        Self::with_base(GENIUS_BASE)
    }

    /// только https; простой http для хоста обратной связи, чтобы тесты могли использовать локальный
    /// сервер.
    pub fn with_base(base: &str) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| Error::InvalidRequest)?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "localhost"));
        if base.scheme() != "https" && !(loopback && base.scheme() == "http") {
            return Err(Error::InvalidRequest);
        }
        let http = net::shared_pool(|| {
            reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(6))
                .timeout(Duration::from_secs(12))
                .pool_idle_timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/128 Safari/537.36")
                .build()
                .map_err(Error::from)
        })?;
        Ok(Self { http, base })
    }

    pub fn lookup(&self, track: &Track) -> Result<Option<Lyrics>> {
        let query = format!("{} {}", track.artist, display_title(&track.title));
        let mut search = self.base.join("api/search/song").map_err(|_| Error::InvalidRequest)?;
        search.query_pairs_mut().append_pair("per_page", "8").append_pair("q", &query);
        let value = self.get_json(search)?;
        let candidates = genius_hits(&value);
        let candidate = candidates.into_iter().find(|(title, artist, url)| {
            same_song(&display_title(&track.title), title) && same_artist(&track.artist, artist) && genius_url(url)
        });
        let Some((_, _, url)) = candidate else { return Ok(None) };
        let page = self.get_text(&url)?;
        let text = extract_genius_lyrics(&page);
        Ok(text.filter(|text| !text.trim().is_empty()).map(|text| Lyrics {
            source: "Genius".into(),
            text,
            lines: Vec::new(),
        }))
    }

    fn get_json(&self, url: Url) -> Result<serde_json::Value> {
        let body = soft_get(&self.http, "genius", |client| {
            let response = client.get(url.clone()).header("Accept", "application/json").header("Referer", "https://genius.com/").send()?;
            if !response.status().is_success() { return Err(Error::Http(response.status().as_u16())); }
            read_capped(response, GENIUS_BODY_LIMIT)
        })?;
        serde_json::from_slice(&body).map_err(|_| Error::Format("Genius search response".into()))
    }

    fn get_text(&self, url: &str) -> Result<String> {
        let parsed = Url::parse(url).map_err(|_| Error::InvalidRequest)?;
        if !genius_url(url) { return Err(Error::InvalidRequest); }
        let body = soft_get(&self.http, "genius", |client| {
            let response = client.get(parsed.clone()).header("Accept", "text/html").header("Referer", "https://genius.com/").send()?;
            if !response.status().is_success() { return Err(Error::Http(response.status().as_u16())); }
            read_capped(response, GENIUS_BODY_LIMIT)
        })?;
        String::from_utf8(body).map_err(|_| Error::Format("Genius page encoding".into()))
    }
}

/// Один GET против дополнительного источника (тексты песен, каверы): если соединение плохое, пул
/// перестраивается и запрос делается еще раз после небольшой паузы. Источник, который после этого все
/// еще недоступен, просто пропускается вызывающей стороной; ничто здесь не стоит больше, чем две
/// быстрые попытки.
fn soft_get<T>(pool: &Pool, source: &str, attempt: impl Fn(&reqwest::blocking::Client) -> Result<T>) -> Result<T> {
    match attempt(&pool.get()) {
        Err(error) if net::is_connectivity(&error) => {
            log::info!("{source}: {} → new connections, trying once more", error.kind());
            net::drop_connections();
            std::thread::sleep(Duration::from_millis(300));
            attempt(&pool.get())
        }
        other => other,
    }
}

fn read_capped(response: reqwest::blocking::Response, limit: u64) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(response, limit + 1), &mut body)
        .map_err(|error| net::read_error(&error))?;
    if body.len() as u64 > limit { return Err(Error::Format("Genius response is too large".into())); }
    Ok(body)
}

fn genius_url(raw: &str) -> bool {
    Url::parse(raw).is_ok_and(|url| url.scheme() == "https" && matches!(url.host_str(), Some("genius.com" | "www.genius.com")))
}

fn genius_hits(value: &serde_json::Value) -> Vec<(String, String, String)> {
    fn collect(value: &serde_json::Value, out: &mut Vec<(String, String, String)>, depth: u8) {
        if depth > 8 { return; }
        match value {
            serde_json::Value::Object(object) => {
                if let Some(result) = object.get("result") {
                    let kind = object.get("type").and_then(serde_json::Value::as_str).unwrap_or("song");
                    if ["song", "lyric", "top_hit"].contains(&kind) {
                        if let (Some(title), Some(artist), Some(url)) = (
                            result.get("title").and_then(serde_json::Value::as_str),
                            result.get("primary_artist").and_then(|item| item.get("name")).and_then(serde_json::Value::as_str),
                            result.get("url").and_then(serde_json::Value::as_str),
                        ) {
                            out.push((title.to_string(), artist.to_string(), url.to_string()));
                        }
                    }
                }
                for item in object.values() { collect(item, out, depth + 1); }
            }
            serde_json::Value::Array(items) => for item in items { collect(item, out, depth + 1); },
            _ => {}
        }
    }
    let mut found = Vec::new();
    collect(value, &mut found, 0);
    found
}

fn normalize_lyric_key(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars().flat_map(char::to_lowercase) {
        let latin = match character {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'ё' => "e",
            'ж' => "zh", 'з' => "z", 'и' | 'і' => "i", 'й' => "y", 'к' => "k", 'л' => "l",
            'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s",
            'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch",
            'ш' => "sh", 'щ' => "sch", 'ы' => "y", 'э' | 'є' => "e", 'ю' => "yu", 'я' => "ya",
            'ґ' => "g", 'ї' => "i", 'ъ' | 'ь' => "", value if value.is_alphanumeric() => {
                result.push(value);
                continue;
            }
            _ => "",
        };
        result.push_str(latin);
    }
    result
}

fn same_song(wanted: &str, found: &str) -> bool {
    let wanted = normalize_lyric_key(wanted);
    let found = normalize_lyric_key(found);
    !wanted.is_empty() && (wanted == found || (wanted.len() >= 6 && found.len() >= 6 && (wanted.contains(&found) || found.contains(&wanted))))
}

fn same_artist(wanted: &str, found: &str) -> bool {
    let wanted = normalize_lyric_key(wanted);
    let found = normalize_lyric_key(found);
    !wanted.is_empty() && !found.is_empty() && (wanted == found || (wanted.len() >= 4 && found.len() >= 4 && (wanted.contains(&found) || found.contains(&wanted))))
}

fn extract_genius_lyrics(html: &str) -> Option<String> {
    // Устаревшая внешняя оболочка может содержать заголовки contributor/translation, а также настоящие
    // контейнеры. Предпочитайте явные лирические блоки.
    extract_genius_blocks(html, true).or_else(|| extract_genius_blocks(html, false))
}

fn extract_genius_blocks(html: &str, explicit: bool) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    let mut fragments = Vec::new();
    while let Some(relative) = lower[cursor..].find("<div") {
        let start = cursor + relative;
        let open_end = lower[start..].find('>')? + start;
        let opening = &lower[start..=open_end];
        let has_attribute = opening.contains("data-lyrics-container");
        let is_lyrics = if explicit {
            opening.split_once("data-lyrics-container").and_then(|(_, tail)| tail.trim_start().strip_prefix('='))
                .is_some_and(|value| {
                    let value = value.trim_start();
                    value.starts_with("\"true\"") || value.starts_with("'true'") || value.starts_with("true>") || value.starts_with("true ")
                })
        } else {
            !has_attribute && opening.contains("lyrics__container")
        };
        if !is_lyrics {
            cursor = open_end + 1;
            continue;
        }
        let mut depth = 1usize;
        let mut at = open_end + 1;
        let content_start = at;
        while depth > 0 {
            let next_open = lower[at..].find("<div").map(|index| at + index);
            let next_close = lower[at..].find("</div").map(|index| at + index);
            match (next_open, next_close) {
                (_, Some(close)) if next_open.is_none_or(|open| close < open) => {
                    depth -= 1;
                    if depth == 0 {
                        fragments.push(strip_html(&html[content_start..close]));
                        at = close + 6;
                        break;
                    }
                    at = close + 6;
                }
                (Some(open), _) => {
                    depth += 1;
                    at = lower[open..].find('>').map(|end| open + end + 1)?;
                }
                _ => break,
            }
        }
        cursor = at;
    }
    let joined = fragments.into_iter().filter(|item| !item.trim().is_empty()).collect::<Vec<_>>().join("\n\n");
    (!joined.is_empty()).then_some(joined)
}

fn strip_html(fragment: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    let mut excluded: Option<(String, usize)> = None;
    for c in fragment.chars() {
        if c == '<' {
            in_tag = true;
            tag.clear();
        } else if c == '>' && in_tag {
            let normalized = tag.trim_start().to_ascii_lowercase();
            let closing = normalized.starts_with('/');
            let name = normalized.trim_start_matches('/').split_whitespace().next().unwrap_or("").trim_end_matches('/');
            if let Some((skipped, depth)) = &mut excluded {
                if name == skipped {
                    if closing { *depth -= 1; } else { *depth += 1; }
                    if *depth == 0 { excluded = None; }
                }
            } else if !closing && (matches!(name, "script" | "style" | "button")
                || normalized.contains("lyricsheader__") || normalized.contains("lyricsfooter__")
                || normalized.contains("data-exclude-from-selection")) {
                excluded = Some((name.to_string(), 1));
            } else if name == "br" || (closing && name == "p") {
                out.push('\n');
            }
            in_tag = false;
        } else if in_tag {
            tag.push(c);
        } else if excluded.is_none() {
            out.push(c);
        }
    }
    for (entity, decoded) in [("&amp;", "&"), ("&quot;", "\""), ("&#39;", "'"), ("&apos;", "'"), ("&lt;", "<"), ("&gt;", ">"), ("&nbsp;", " ")] {
        out = out.replace(entity, decoded);
    }
    out.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
}

impl LrcLib {
    pub fn new() -> Result<Self> {
        Self::with_base(LRCLIB_BASE)
    }

    /// только https; простой http для хоста обратной связи, чтобы тесты могли использовать локальный
    /// сервер.
    pub fn with_base(base: &str) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| Error::InvalidRequest)?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "localhost"));
        if base.scheme() != "https" && !(loopback && base.scheme() == "http") {
            return Err(Error::InvalidRequest);
        }
        let http = net::shared_pool(|| {
            reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(6))
                .timeout(Duration::from_secs(10))
                .pool_idle_timeout(Duration::from_secs(20))
                // Фиксированный хост: ничто из того, что он говорит, не может отправить нас куда-либо
                // еще.
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("PrivateMusicDesktop (https://github.com/decoder-dev/PrivateMusicDesktop)")
                .build()
                .map_err(Error::from)
        })?;
        Ok(Self { http, base })
    }

    fn fetch(&self, path: &str, query: &[(&str, String)]) -> Result<Option<Vec<Candidate>>> {
        let mut url = self.base.join(path).map_err(|_| Error::InvalidRequest)?;
        url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        let body = soft_get(&self.http, "lrclib", |client| {
            let response = client.get(url.clone()).header("Accept", "application/json").send()?;
            match response.status().as_u16() {
                404 => return Ok(None),
                200..=299 => {}
                other => return Err(Error::Http(other)),
            }
            let mut body = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(response, MAX_BODY + 1), &mut body)
                .map_err(|e| net::read_error(&e))?;
            Ok(Some(body))
        })?;
        let Some(body) = body else { return Ok(None) };
        if body.len() as u64 > MAX_BODY {
            return Err(Error::Format("lyrics response is too large".into()));
        }
        if let Ok(list) = serde_json::from_slice::<Vec<Candidate>>(&body) {
            return Ok(Some(list));
        }
        serde_json::from_slice::<Candidate>(&body).map(|c| Some(vec![c])).map_err(|_| Error::Format("lyrics response".into()))
    }

    /// `Ok(None)`, когда у LRCLIB нет ничего подходящего для этого трека. Ошибка 404 или пустой список
    /// — это "nothing"; любой другой сбой является ошибкой только в том случае, если ни один запрос не
    /// нашел текст. Запрос, который завершился неудачей со статусом HTTP, не останавливает следующий;
    /// сбой транспорта (оффлайн, тайм-аут) касается хоста и немедленно прекращает поиск.
    pub fn versions(&self, track: &Track) -> Result<Vec<Lyrics>> {
        let mut versions = Vec::new();
        for (path, query) in lookup_queries(track).into_iter().take(2) {
            if let Some(found) = self.fetch(path, &query)? {
                for candidate in found.into_iter().take(30) {
                    if let Some(lyrics) = best(track, vec![candidate]) {
                        if !versions.iter().any(|old: &Lyrics| old.text == lyrics.text && old.lines == lyrics.lines) { versions.push(lyrics); }
                    }
                }
            }
            if versions.len() >= 8 { break; }
        }
        versions.truncate(8);
        Ok(versions)
    }

    pub fn lookup(&self, track: &Track) -> Result<Option<Lyrics>> {
        if track.title.trim().is_empty() {
            return Ok(None);
        }
        let mut fallback: Option<Lyrics> = None;
        let mut failure: Option<Error> = None;
        for (path, query) in lookup_queries(track) {
            match self.fetch(path, &query) {
                Ok(None) => {}
                Ok(Some(found)) => {
                    let Some(lyrics) = best(track, found) else { continue };
                    // Синхронизированные тексты завершают поиск; простой результат сохраняется на
                    // случай, если ни один запрос не найдет лучшего.
                    if !lyrics.lines.is_empty() {
                        return Ok(Some(lyrics));
                    }
                    fallback = fallback.or(Some(lyrics));
                }
                Err(error @ (Error::Offline | Error::Timeout | Error::Transport(_))) => {
                    return match fallback {
                        Some(found) => Ok(Some(found)),
                        None => Err(error),
                    };
                }
                Err(error) => {
                    log::debug!("{}", describe("lrclib", &error));
                    failure = failure.or(Some(error));
                }
            }
        }
        match (fallback, failure) {
            (Some(found), _) => Ok(Some(found)),
            (None, Some(error)) => Err(error),
            (None, None) => Ok(None),
        }
    }
}

type Query = (&'static str, Vec<(&'static str, String)>);

/// Что спрашивать у LRCLIB, по порядку: точную запись (исполнитель, название, длина), затем поиск по
/// названию как задано, и по названию очищенное от "(speed up)", "[Live]", "feat. X" и т.п., с ведущим
/// исполнителем и без него. Кандидаты сопоставляются локально, поэтому свободный запрос не может
/// вернуть другую песню.
fn lookup_queries(track: &Track) -> Vec<Query> {
    let mut queries: Vec<Query> = Vec::new();
    if track.duration > 0 {
        let mut query = vec![
            ("track_name", track.title.clone()),
            ("artist_name", track.artist.clone()),
            ("duration", track.duration.to_string()),
        ];
        if let Some(album) = track.album_title.as_deref().filter(|a| !a.is_empty()) {
            query.push(("album_name", album.to_string()));
        }
        queries.push(("api/get", query));
    }
    let clean = display_title(&track.title);
    let title = if clean.is_empty() { track.title.trim().to_string() } else { clean };
    let searches = [
        vec![("track_name", track.title.clone()), ("artist_name", track.artist.clone())],
        vec![("track_name", title.clone()), ("artist_name", lead_artist(&track.artist))],
        vec![("track_name", title)],
    ];
    for search in searches {
        if !queries.iter().any(|(path, q)| *path == "api/search" && *q == search) {
            queries.push(("api/search", search));
        }
    }
    queries
}

fn find_chars(haystack: &[char], marker: &str) -> Option<usize> {
    let marker: Vec<char> = marker.chars().collect();
    haystack.windows(marker.len()).position(|w| w == marker.as_slice())
}

fn lower_chars(text: &str) -> Vec<char> {
    // Один символ на символ, поэтому позиции в результате — это позиции в `text`.
    text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

const SUFFIX_MARKERS: [&str; 15] = [
    "remaster", "remix", "live", "version", "edit", "mix", "speed", "sped", "slowed", "nightcore", "acoustic",
    "instrumental", "radio", "ремикс", "версия",
];

/// Название так, как его называют люди: без деталей "(…)"/"[…]", хвост в стиле "- Remastered" или хвост
/// "feat.". Дело и буквы сохранены.
fn display_title(text: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0u32;
    for ch in text.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            c => plain.push(c),
        }
    }
    let lowered = lower_chars(&plain);
    let chars: Vec<char> = plain.chars().collect();
    let mut cut = chars.len();
    for marker in [" feat.", " feat ", " ft. ", " ft ", " featuring ", " при уч."] {
        if let Some(i) = find_chars(&lowered, marker) {
            cut = cut.min(i);
        }
    }
    for marker in [" - ", " – ", " — "] {
        if let Some(i) = find_chars(&lowered, marker) {
            let tail: String = lowered[i..].iter().collect();
            if SUFFIX_MARKERS.iter().any(|m| tail.contains(m)) {
                cut = cut.min(i);
            }
        }
    }
    chars[..cut].iter().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Первый из нескольких признанных исполнителей: "A, B feat. C" — "A".
fn lead_artist(text: &str) -> String {
    let lowered = lower_chars(text);
    let chars: Vec<char> = text.chars().collect();
    let mut cut = chars.iter().position(|c| matches!(c, ',' | ';' | '/' | '&' | '+')).unwrap_or(chars.len());
    for marker in [" feat.", " feat ", " ft. ", " ft ", " featuring ", " при уч.", " x ", " и ", " and ", " vs. ", " vs ", " with "] {
        if let Some(i) = find_chars(&lowered, marker) {
            cut = cut.min(i);
        }
    }
    let lead = chars[..cut].iter().collect::<String>().trim().to_string();
    if lead.is_empty() { text.trim().to_string() } else { lead }
}

/// Краткое, не секретное описание сбоя в журнале: источник, тип или статус, но не URL.
pub fn describe(source: &str, error: &Error) -> String {
    match error {
        Error::Timeout => format!("{source}: timeout"),
        Error::Offline => format!("{source}: no connection"),
        Error::Http(status) => format!("{source}: HTTP {status}"),
        Error::Vk { code, .. } => format!("{source}: error code {code}"),
        Error::Transport(what) => format!("{source}: network error ({what})"),
        Error::Format(what) => format!("{source}: unexpected response ({what})"),
        other => format!("{source}: {}", other.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genius_headers_and_controls_are_not_song_lyrics() {
        let html = r#"<div data-lyrics-container="true"><div class="LyricsHeader__Container-x">
            <span>17 Contributors</span><div>Translations<div>English</div></div><h2>Song Lyrics</h2>
            </div>[Куплет]<br/>Первая строка<br>Вторая &amp; третья
            <div class="LyricsFooter__Container-x">Embed</div><button>Read more</button></div>"#;
        assert_eq!(extract_genius_lyrics(html).as_deref(), Some("[Куплет]\nПервая строка\nВторая & третья"));
    }

    #[test]
    fn genius_explicit_blocks_win_over_an_outer_legacy_wrapper() {
        let html = r#"<div class="Lyrics__Container-old">17 Contributors
            <div data-lyrics-container="true">Один<br>Два</div>Translations
            <div data-lyrics-container = 'true'><a href="/annotation">Три</a></div></div>"#;
        assert_eq!(extract_genius_lyrics(html).as_deref(), Some("Один\nДва\n\nТри"));
    }

    #[test]
    fn genius_legacy_blocks_still_work_but_metadata_alone_is_not_a_song() {
        assert_eq!(extract_genius_lyrics(r#"<div class="Lyrics__Container-old">Раз<br>Два</div>"#).as_deref(), Some("Раз\nДва"));
        assert!(extract_genius_lyrics(r#"<div data-lyrics-container="true"><div class="LyricsHeader__Container-x">17 Contributors</div></div>"#).is_none());
        assert!(extract_genius_lyrics(r#"<div data-lyrics-container="false">Not lyrics</div>"#).is_none());
    }

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
        // Точные удары частичные.
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
    fn titles_and_artists_are_cleaned_for_the_search() {
        assert_eq!(display_title("Дорога в ад (speed up)"), "Дорога в ад");
        assert_eq!(display_title("Song [Live] (feat. X)"), "Song");
        assert_eq!(display_title("Song - Remastered 2011"), "Song");
        assert_eq!(display_title("Song feat. Y"), "Song");
        assert_eq!(display_title("Ночь - это день"), "Ночь - это день");
        assert_eq!(display_title("Mr. Brightside"), "Mr. Brightside");
        assert_eq!(lead_artist("Скриптонит, Niman"), "Скриптонит");
        assert_eq!(lead_artist("A feat. B"), "A");
        assert_eq!(lead_artist("Чучело"), "Чучело");
        let t = track("Чучело", "Дорога в ад (speed up)", 150);
        let queries = lookup_queries(&t);
        assert_eq!(queries[0].0, "api/get");
        assert!(queries.iter().any(|(p, q)| *p == "api/search" && q.len() == 1 && q[0].1 == "Дорога в ад"));
        // Никаких повторяющихся запросов для простого заголовка.
        let plain = lookup_queries(&track("A", "B", 0));
        assert_eq!(plain.len(), 2, "{plain:?}");
    }

    #[test]
    fn failures_are_described_without_urls() {
        assert_eq!(describe("lrclib", &Error::Timeout), "lrclib: timeout");
        assert_eq!(describe("lrclib", &Error::Http(503)), "lrclib: HTTP 503");
        assert_eq!(describe("vk lyrics", &Error::Vk { code: 15, message: "x".into() }), "vk lyrics: error code 15");
    }

    #[test]
    fn base_must_be_https_or_loopback() {
        assert!(LrcLib::with_base("https://lrclib.net/").is_ok());
        assert!(LrcLib::with_base("http://127.0.0.1:1/").is_ok());
        assert!(LrcLib::with_base("http://lrclib.net/").is_err());
        assert!(LrcLib::with_base("file:///x").is_err());
    }
}
