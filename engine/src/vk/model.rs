use serde_json::Value;

use crate::unmask;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub track_id: i64,
    pub owner_id: i64,
    pub title: String,
    pub artist: String,
    pub album_title: Option<String>,
    pub duration: u32,
    pub stream_url: Option<String>,
    pub artwork_url: Option<String>,
    pub access_key: Option<String>,
    pub lyrics_id: Option<i64>,
    pub is_hq: bool,
    pub track_code: Option<String>,
    /// The artists VK names on the track (`main_artists`, then
    /// `featured_artists`), so the window can open an artist page. Missing in
    /// queues and downloads saved before this existed.
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    /// The release the track belongs to, when VK gave a complete locator.
    #[serde(default)]
    pub album: Option<AlbumRef>,
}

/// An artist as a track or an album names it. `id` is VK's artist id (an
/// opaque string); a name VK gave without one has `None`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ArtistRef {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
}

/// Where a track's album lives: enough to open it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlbumRef {
    pub album_id: i64,
    pub owner_id: i64,
    #[serde(default)]
    pub access_key: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

/// An artist from `audio.searchArtists` or VK's artist catalogue page.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub photo_url: Option<String>,
}

/// A release (VK keeps albums as playlists, so the locator is the same:
/// owner, id and, for community-owned ones, an access key).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Album {
    pub album_id: i64,
    pub owner_id: i64,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub access_key: Option<String>,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub is_followed: bool,
    /// What `audio.followPlaylist` wants as `hash`, when VK sends one.
    #[serde(default)]
    pub follow_hash: Option<String>,
}

impl Album {
    pub fn id(&self) -> String {
        format!("{}_{}", self.owner_id, self.album_id)
    }

    /// The key, when there is one that is not just blank.
    pub fn usable_access_key(&self) -> Option<&str> {
        self.access_key.as_deref().map(str::trim).filter(|k| !k.is_empty())
    }

    /// Community and official albums have a negative owner; VK wants an
    /// `access_key` for their tracks.
    pub fn needs_access_key(&self) -> bool {
        self.owner_id < 0 && self.usable_access_key().is_none()
    }

    pub fn artist_names(&self) -> impl Iterator<Item = &str> {
        self.artists.iter().map(|a| a.name.as_str())
    }

    /// Prefers what `other` knows about access. An `access_key` is only
    /// valid for its own `owner_id` + `id`, so a different album that has a
    /// key replaces the locator entirely instead of having its key grafted
    /// onto a stale id.
    pub fn merging_access_metadata(&self, other: &Album) -> Album {
        let use_other = other.id() != self.id() && other.usable_access_key().is_some();
        Album {
            album_id: if use_other { other.album_id } else { self.album_id },
            owner_id: if use_other { other.owner_id } else { self.owner_id },
            title: if is_usable_title(&self.title) { self.title.clone() } else { other.title.clone() },
            description: self.description.clone().or_else(|| other.description.clone()),
            count: if other.count > 0 { other.count } else { self.count },
            artwork_url: self.artwork_url.clone().or_else(|| other.artwork_url.clone()),
            access_key: other.usable_access_key().or(self.usable_access_key()).map(str::to_string),
            artists: if self.artists.is_empty() { other.artists.clone() } else { self.artists.clone() },
            year: self.year.or(other.year),
            is_followed: self.is_followed || other.is_followed,
            follow_hash: other.follow_hash.clone().or_else(|| self.follow_hash.clone()),
        }
    }
}

/// A title worth searching for: not blank and not VK's "unnamed".
pub fn is_usable_title(title: &str) -> bool {
    let trimmed = title.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unnamed")
}

impl Track {
    /// `owner_audio`, the identity VK and the iOS app use.
    pub fn id(&self) -> String {
        format!("{}_{}", self.owner_id, self.track_id)
    }

    /// The form `audio.getById` wants: `owner_audio[_accessKey]`.
    pub fn audio_id_with_key(&self) -> String {
        match self.access_key.as_deref().filter(|k| !k.is_empty()) {
            Some(key) => format!("{}_{}", self.id(), key),
            None => self.id(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub playlist_id: i64,
    pub owner_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub count: u32,
    pub artwork_url: Option<String>,
    pub access_key: Option<String>,
}

impl Playlist {
    pub fn id(&self) -> String {
        format!("{}_{}", self.owner_id, self.playlist_id)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub photo_url: Option<String>,
}

/// A VK catalogue stream mix, including optional tuning already attached to
/// a mood or activity tile.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MusicMix {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork_url: Option<String>,
    pub section_title: Option<String>,
    pub options: Option<String>,
}

impl Profile {
    pub fn display_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name).trim().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u32,
    /// Offset to request next, `None` when the list is exhausted.
    pub next_offset: Option<u32>,
}

/// VK sends ids as numbers, numeric strings or floats depending on the
/// method, so every integer read goes through here.
pub(crate) fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn best_photo(object: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .and_then(unmask::secure_remote_url)
}

/// Cover: `album.thumb`, then `album.photo`, then a top-level `thumb`,
/// largest first. Stream-mix tracks often carry only the last.
fn track_artwork(object: &Value) -> Option<String> {
    const KEYS: [&str; 3] = ["photo_600", "photo_300", "photo_270"];
    if let Some(album) = object.get("album") {
        for field in ["thumb", "photo"] {
            if let Some(found) = album.get(field).and_then(|t| best_photo(t, &KEYS)) {
                return Some(found);
            }
        }
    }
    object.get("thumb").and_then(|t| best_photo(t, &KEYS))
}

/// Lossy decode of one `audio` object. One ad or placeholder entry must not
/// fail a whole page, so anything that is not a real track is skipped.
pub fn parse_track(object: &Value, user_id: Option<i64>) -> Option<Track> {
    let track_id = int(object.get("id")?)?;
    let owner_id = int(object.get("owner_id")?)?;
    if track_id <= 0 || owner_id == 0 {
        return None;
    }
    let title = text(object.get("title"))?;
    let artist = text(object.get("artist"))?;
    let is_hq = match object.get("is_hq") {
        Some(Value::Bool(flag)) => *flag,
        Some(other) => int(other).map(|n| n != 0).unwrap_or(false),
        None => false,
    };
    let stream_url = object
        .get("url")
        .and_then(Value::as_str)
        .filter(|u| !u.is_empty())
        .and_then(|raw| unmask::resolve(raw, user_id))
        .and_then(|resolved| unmask::secure_remote_url(&resolved));
    Some(Track {
        track_id,
        owner_id,
        title,
        artist,
        album_title: text(object.get("album_title"))
            .or_else(|| text(object.get("album").and_then(|a| a.get("title")))),
        duration: object.get("duration").and_then(int).unwrap_or(0).max(0) as u32,
        stream_url,
        artwork_url: track_artwork(object),
        access_key: text(object.get("access_key")),
        lyrics_id: object.get("lyrics_id").and_then(int),
        is_hq,
        track_code: text(object.get("track_code")),
        artists: track_artists(object),
        album: album_ref(object),
    })
}

/// `main_artists` then `featured_artists`, each `{name, id, domain}`.
/// Duplicates (the same id twice) are dropped.
fn track_artists(object: &Value) -> Vec<ArtistRef> {
    let mut found: Vec<ArtistRef> = Vec::new();
    for key in ["main_artists", "featured_artists"] {
        for entry in object.get(key).and_then(Value::as_array).into_iter().flatten() {
            if let Some(artist) = artist_ref(entry) {
                if !found.iter().any(|a| a.id.is_some() && a.id == artist.id) {
                    found.push(artist);
                }
            }
        }
    }
    found
}

fn artist_ref(entry: &Value) -> Option<ArtistRef> {
    let name = text(entry.get("name"))?.trim().to_string();
    if !is_usable_artist(&name) {
        return None;
    }
    let id = entry.get("id").and_then(string_id).filter(|id| valid_artist_id(id));
    Some(ArtistRef { id, name })
}

/// A nested `album` with both its id and its owner. A top-level `album_id`
/// or an id without an owner is not a locator: the album's owner can differ
/// from the track's.
fn album_ref(object: &Value) -> Option<AlbumRef> {
    let album = object.get("album")?;
    let album_id = int(album.get("id")?)?;
    let owner_id = int(album.get("owner_id")?)?;
    if album_id <= 0 || owner_id == 0 {
        return None;
    }
    Some(AlbumRef {
        album_id,
        owner_id,
        access_key: text(album.get("access_key")).filter(|k| !k.trim().is_empty()),
        title: text(album.get("title")).filter(|t| is_usable_title(t)),
    })
}

/// VK artist ids are opaque strings (`"5730061962467626447"`, sometimes a
/// domain). Anything else is not sent back to VK.
pub fn valid_artist_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// A number or a string, as text: VK sends artist ids both ways.
fn string_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(_) => int(value).map(|n| n.to_string()),
        _ => None,
    }
}

fn is_usable_artist(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unknown")
}

/// Every `https` picture under `value`, with the size it claims (`width`,
/// `height`, `size`, or the `_600` in a key), largest wins; on a tie the
/// first one seen.
fn largest_picture(value: &Value) -> Option<String> {
    let mut found: Vec<(i64, String)> = Vec::new();
    collect_pictures(value, None, 0, &mut found);
    let mut best: Option<(i64, String)> = None;
    for (size, url) in found {
        if best.as_ref().is_none_or(|(b, _)| size > *b) {
            best = Some((size, url));
        }
    }
    best.map(|(_, url)| url)
}

const PICTURE_KEYS: [&str; 11] =
    ["photo_1200", "photo_600", "photo_300", "photo_270", "photo_135", "photo", "cover_url", "thumb_url", "image", "src", "url"];

fn collect_pictures(value: &Value, inherited: Option<i64>, depth: u32, out: &mut Vec<(i64, String)>) {
    if depth > 6 {
        return;
    }
    match value {
        Value::String(raw) => {
            if let Some(url) = unmask::secure_remote_url(raw) {
                out.push((inherited.unwrap_or(0), url));
            }
        }
        Value::Array(list) => list.iter().for_each(|v| collect_pictures(v, inherited, depth + 1, out)),
        Value::Object(object) => {
            let own = ["width", "height", "size"].iter().filter_map(|k| object.get(*k).and_then(int)).max().or(inherited);
            for key in PICTURE_KEYS {
                if let Some(raw) = object.get(key).and_then(Value::as_str) {
                    if let Some(url) = unmask::secure_remote_url(raw) {
                        let hinted = key.rsplit('_').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                        out.push((hinted.max(own.unwrap_or(0)), url));
                    }
                }
            }
            for key in ["thumb", "cover", "photo", "thumbs", "photos", "images", "image", "grid", "items", "list"] {
                if let Some(nested) = object.get(key).filter(|v| !v.is_string()) {
                    collect_pictures(nested, own, depth + 1, out);
                }
            }
        }
        _ => {}
    }
}

/// An album's cover: a direct `photo_*` / `cover_url` first, then the
/// largest picture in `thumb`, `cover`, `thumbs`, `photos`, …
fn album_artwork(object: &Value) -> Option<String> {
    for key in ["photo_1200", "photo_600", "photo_300", "photo_270", "photo", "cover_url", "thumb_url", "image"] {
        let Some(value) = object.get(key) else { continue };
        if let Some(url) = value.as_str().and_then(unmask::secure_remote_url) {
            return Some(url);
        }
        if let Some(url) = largest_picture(value) {
            return Some(url);
        }
    }
    ["thumb", "cover", "thumbs", "photos", "images", "grid"].iter().find_map(|key| object.get(*key).and_then(largest_picture))
}

/// Release year from `release_date` (a unix time, or a bare year, as a
/// number or a string, or an ISO date), else from `year`.
fn release_year(object: &Value) -> Option<i32> {
    let plausible = |year: i64| (1900..=2200).contains(&year).then_some(year as i32);
    if let Some(raw) = object.get("release_date") {
        if let Some(n) = int(raw) {
            if n > 10_000 {
                return Some(year_of_unix(n));
            }
            if let Some(year) = plausible(n) {
                return Some(year);
            }
        }
        if let Some(year) = raw.as_str().and_then(|s| s.get(..4)).and_then(|y| y.parse().ok()).and_then(plausible) {
            return Some(year);
        }
    }
    object.get("year").and_then(int).and_then(plausible)
}

/// Gregorian year of a unix time (UTC), without a date library.
fn year_of_unix(seconds: i64) -> i32 {
    // Howard Hinnant's days-to-civil.
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2)) as i32
}

fn flag(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(flag) => Some(*flag),
        Value::String(s) => Some(matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes")),
        other => int(other).map(|n| n != 0),
    }
}

/// Lossy decode of one album / release object. Artists come from
/// `main_artists`, `artists` (objects or plain names) and `artist`, unique
/// by name.
pub fn parse_album(object: &Value) -> Option<Album> {
    let album_id = int(object.get("id")?)?;
    let owner_id = int(object.get("owner_id")?)?;
    if owner_id == 0 {
        return None;
    }
    let mut artists: Vec<ArtistRef> = Vec::new();
    let mut add = |artist: ArtistRef| {
        let key = artist.name.to_lowercase();
        if let Some(known) = artists.iter_mut().find(|a| a.name.to_lowercase() == key) {
            if known.id.is_none() {
                known.id = artist.id;
            }
        } else {
            artists.push(artist);
        }
    };
    for key in ["main_artists", "artists"] {
        for entry in object.get(key).and_then(Value::as_array).into_iter().flatten() {
            match entry {
                Value::String(name) if is_usable_artist(name) => add(ArtistRef { id: None, name: name.trim().to_string() }),
                _ => {
                    if let Some(artist) = artist_ref(entry) {
                        add(artist);
                    }
                }
            }
        }
    }
    if let Some(name) = text(object.get("artist")).filter(|n| is_usable_artist(n)) {
        add(ArtistRef { id: None, name: name.trim().to_string() });
    }
    Some(Album {
        album_id,
        owner_id,
        title: text(object.get("title")).map(|t| t.trim().to_string()).unwrap_or_default(),
        description: text(object.get("description")).filter(|d| !d.is_empty()),
        count: object.get("count").or_else(|| object.get("size")).and_then(int).unwrap_or(0).max(0) as u32,
        artwork_url: album_artwork(object),
        access_key: text(object.get("access_key")).filter(|k| !k.trim().is_empty()),
        artists,
        year: release_year(object),
        // VK's playlist objects say `is_following`; older payloads
        // `is_followed` / `followed`.
        is_followed: ["is_following", "is_followed", "followed"].iter().any(|k| flag(object.get(*k)) == Some(true)),
        follow_hash: text(object.get("follow_hash")).filter(|h| !h.is_empty()),
    })
}

/// An artist object wherever it sits in a response: a name (or title) and
/// an id (or `artist_id`, or `domain`). Audio rows and albums also have
/// both, so those shapes are refused, as is a typed object whose type is
/// not an artist and that carries no picture.
pub fn parse_artist(object: &Value) -> Option<Artist> {
    let name = text(object.get("name")).or_else(|| text(object.get("title")))?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let id = ["id", "artist_id", "domain"].iter().find_map(|k| object.get(*k).and_then(string_id))?;
    let has = |k: &str| object.get(k).is_some();
    if has("owner_id") && (has("duration") || has("main_artists")) {
        return None;
    }
    let photo = largest_picture(object);
    let kind = text(object.get("type")).or_else(|| text(object.get("data_type"))).unwrap_or_default();
    if !kind.is_empty() && !kind.to_lowercase().contains("artist") && photo.is_none() {
        return None;
    }
    valid_artist_id(&id).then_some(Artist { id, name, photo_url: photo })
}

pub fn parse_playlist(object: &Value) -> Option<Playlist> {
    let playlist_id = int(object.get("id")?)?;
    let owner_id = int(object.get("owner_id")?)?;
    let artwork = ["photo_1200", "photo_600", "photo_300", "photo_270"]
        .iter()
        .find_map(|k| object.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            ["thumb", "photo"].iter().find_map(|field| {
                object.get(*field).and_then(|t| {
                    best_photo(t, &["photo_1200", "photo_600", "photo_300", "photo_270"])
                })
            })
        })
        .or_else(|| {
            // `thumbs` is an array of photo objects.
            object.get("thumbs").and_then(Value::as_array).and_then(|list| {
                list.iter().find_map(|t| {
                    best_photo(t, &["photo_1200", "photo_600", "photo_300", "photo_270"])
                })
            })
        })
        .and_then(|raw| unmask::secure_remote_url(&raw));
    Some(Playlist {
        playlist_id,
        owner_id,
        // An untitled playlist is still a playlist.
        title: text(object.get("title")).unwrap_or_else(|| "Playlist".to_string()),
        description: text(object.get("description")).filter(|d| !d.is_empty()),
        count: object.get("count").and_then(int).unwrap_or(0).max(0) as u32,
        artwork_url: artwork,
        access_key: text(object.get("access_key")),
    })
}

pub fn parse_profile(object: &Value) -> Option<Profile> {
    Some(Profile {
        id: int(object.get("id")?)?,
        first_name: text(object.get("first_name"))?,
        last_name: text(object.get("last_name")).unwrap_or_default(),
        photo_url: text(object.get("photo_200")).and_then(|u| unmask::secure_remote_url(&u)),
    })
}

/// Page arithmetic shared by every list method: advance by the raw entry
/// count, so skipping an ad object never replays the same window.
pub fn next_offset(offset: u32, received: u32, total: u32) -> Option<u32> {
    let consumed = offset.saturating_add(received);
    (received > 0 && consumed < total).then_some(consumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn track_carries_its_artists_and_album_locator() {
        // Shape of an `audio.get` item from VK.
        let value = json!({
            "artist": "Монеточка, Noize MC", "id": 456239017, "owner_id": -2001859017,
            "title": "Последняя дискотека", "duration": 224, "access_key": "c5d0a0d1d2", "is_explicit": false,
            "url": "https://cs1-77v4.vkuseraudio.net/s/v1/ac/x/index.m3u8",
            "album": {"id": 13859017, "title": "Раскраски для взрослых", "owner_id": -2000859017,
                      "access_key": "a1b2c3d4e5f6", "thumb": {"width": 300, "height": 300,
                      "photo_300": "https://sun9-1.userapi.com/c300.jpg", "photo_600": "https://sun9-1.userapi.com/c600.jpg"}},
            "main_artists": [{"name": "Монеточка", "domain": "monetochka", "id": "1430426429588470911"}],
            "featured_artists": [{"name": "Noize MC", "domain": "noizemc", "id": "5196457418498633612"},
                                 {"name": "Монеточка", "id": "1430426429588470911"}],
            "track_code": "a8d3"
        });
        let t = parse_track(&value, Some(1)).unwrap();
        let names: Vec<_> = t.artists.iter().map(|a| (a.id.as_deref().unwrap(), a.name.as_str())).collect();
        assert_eq!(names, [("1430426429588470911", "Монеточка"), ("5196457418498633612", "Noize MC")]);
        let album = t.album.unwrap();
        assert_eq!((album.album_id, album.owner_id), (13859017, -2000859017));
        assert_eq!(album.access_key.as_deref(), Some("a1b2c3d4e5f6"));
        assert_eq!(album.title.as_deref(), Some("Раскраски для взрослых"));
    }

    #[test]
    fn album_without_owner_is_no_locator_and_hostile_artist_ids_are_dropped() {
        let value = json!({"id": 1, "owner_id": 2, "artist": "a", "title": "t", "album_id": 5,
                           "album": {"id": 5, "title": "x"},
                           "main_artists": [{"name": "A", "id": "../../x"}, {"name": "  "}]});
        let t = parse_track(&value, None).unwrap();
        assert!(t.album.is_none());
        assert_eq!(t.artists, [ArtistRef { id: None, name: "A".into() }]);
    }

    #[test]
    fn tracks_saved_before_artist_links_still_load() {
        let old = r#"{"track_id":1,"owner_id":2,"title":"t","artist":"a","album_title":null,"duration":3,
                     "stream_url":null,"artwork_url":null,"access_key":null,"lyrics_id":null,"is_hq":false,"track_code":null}"#;
        let t: Track = serde_json::from_str(old).unwrap();
        assert!(t.artists.is_empty() && t.album.is_none());
    }

    #[test]
    fn release_from_an_artist_page() {
        // `audio.getAlbumsByArtist` item.
        let value = json!({
            "id": 13859017, "owner_id": -2000859017, "type": 1, "title": " Раскраски для взрослых ",
            "description": "", "count": 13, "is_following": false, "year": 2018, "release_date": 1527800400,
            "access_key": "a1b2c3d4e5f6", "album_type": "main_only",
            "main_artists": [{"name": "Монеточка", "domain": "monetochka", "id": "1430426429588470911"}],
            "photo": {"width": 300, "height": 300, "photo_34": "https://sun9-2.userapi.com/c34.jpg",
                      "photo_300": "https://sun9-2.userapi.com/c300.jpg", "photo_1200": "https://sun9-2.userapi.com/c1200.jpg"},
            "follow_hash": "f0110f"
        });
        let a = parse_album(&value).unwrap();
        assert_eq!(a.id(), "-2000859017_13859017");
        assert_eq!(a.title, "Раскраски для взрослых");
        assert_eq!(a.count, 13);
        assert_eq!(a.year, Some(2018));
        assert_eq!(a.artwork_url.as_deref(), Some("https://sun9-2.userapi.com/c1200.jpg"));
        assert_eq!(a.artists[0].id.as_deref(), Some("1430426429588470911"));
        assert_eq!(a.follow_hash.as_deref(), Some("f0110f"));
        assert!(!a.is_followed && !a.needs_access_key());
    }

    #[test]
    fn album_years_and_follow_flags_in_every_shape() {
        let year = |extra: Value| {
            let mut v = json!({"id": 1, "owner_id": -2});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            parse_album(&v).unwrap()
        };
        assert_eq!(year(json!({"release_date": 0})).year, None);
        assert_eq!(year(json!({"release_date": "1527800400"})).year, Some(2018));
        assert_eq!(year(json!({"release_date": 1999})).year, Some(1999));
        assert_eq!(year(json!({"release_date": "2021-03-04T00:00:00Z"})).year, Some(2021));
        assert_eq!(year(json!({"year": "2004"})).year, Some(2004));
        assert_eq!(year(json!({"release_date": 946_684_799})).year, Some(1999));
        assert_eq!(year(json!({"release_date": 946_684_800})).year, Some(2000));
        assert!(year(json!({"is_followed": 1})).is_followed);
        assert!(year(json!({"followed": "1"})).is_followed);
        assert!(year(json!({"is_following": true})).is_followed);
        let named = year(json!({"artists": ["A", "a", "unknown"], "artist": "B", "thumbs": [
            {"photo_300": "http://cdn.example/3.jpg"}, {"photo_600": "https://cdn.example/6.jpg"}]}));
        assert_eq!(named.artist_names().collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(named.artwork_url.as_deref(), Some("https://cdn.example/6.jpg"));
        assert!(named.needs_access_key());
    }

    #[test]
    fn merging_access_metadata_moves_the_locator_only_with_a_key() {
        let mine = parse_album(&json!({"id": 1, "owner_id": -2, "title": "unnamed", "main_artists": [{"name": "A", "id": "1"}]})).unwrap();
        let keyed = parse_album(&json!({"id": 9, "owner_id": -3, "title": "Real", "access_key": "k", "count": 4})).unwrap();
        let merged = mine.merging_access_metadata(&keyed);
        assert_eq!((merged.id(), merged.title.as_str(), merged.count), ("-3_9".into(), "Real", 4));
        assert_eq!(merged.access_key.as_deref(), Some("k"));
        assert_eq!(merged.artists[0].name, "A");
        let keyless = Album { access_key: None, ..keyed };
        assert_eq!(mine.merging_access_metadata(&keyless).id(), "-2_1");
    }

    #[test]
    fn artists_are_told_apart_from_tracks_and_albums() {
        let artist = json!({"name": "Монеточка", "domain": "monetochka", "id": "1430426429588470911",
                            "photo": [{"url": "https://sun9-3.userapi.com/a135.jpg", "width": 135, "height": 135},
                                      {"url": "https://sun9-3.userapi.com/a600.jpg", "width": 600, "height": 600}]});
        let a = parse_artist(&artist).unwrap();
        assert_eq!((a.id.as_str(), a.name.as_str()), ("1430426429588470911", "Монеточка"));
        assert_eq!(a.photo_url.as_deref(), Some("https://sun9-3.userapi.com/a600.jpg"));
        assert!(parse_artist(&json!({"name": "t", "id": 1, "owner_id": 2, "duration": 3})).is_none());
        assert!(parse_artist(&json!({"title": "t", "id": 1, "owner_id": 2, "main_artists": []})).is_none());
        assert!(parse_artist(&json!({"title": "Section", "id": "x", "type": "playlist"})).is_none());
        assert!(parse_artist(&json!({"name": "Domain only", "domain": "only"})).is_some());
    }

    #[test]
    fn track_with_nested_album_and_string_ids() {
        let value = json!({
            "id": "456239537", "owner_id": 880122316, "artist": "Ksb music",
            "title": "Война", "duration": 187, "is_hq": 1,
            "access_key": "abc123", "lyrics_id": 42,
            "album": {"id": 7, "owner_id": 880122316, "title": "Album",
                      "thumb": {"photo_300": "//cdn.example/c300.jpg"}},
            "unknown_future_field": {"nested": [1, 2, 3]}
        });
        let t = parse_track(&value, Some(1)).unwrap();
        assert_eq!(t.track_id, 456239537);
        assert_eq!(t.id(), "880122316_456239537");
        assert_eq!(t.audio_id_with_key(), "880122316_456239537_abc123");
        assert_eq!(t.album_title.as_deref(), Some("Album"));
        assert_eq!(t.artwork_url.as_deref(), Some("https://cdn.example/c300.jpg"));
        assert!(t.is_hq);
        assert_eq!(t.lyrics_id, Some(42));
    }

    #[test]
    fn is_hq_accepts_number_and_bool() {
        for (raw, expected) in [(json!(1), true), (json!(0), false), (json!(true), true), (json!(false), false)] {
            let v = json!({"id": 1, "owner_id": 2, "artist": "a", "title": "t", "is_hq": raw});
            assert_eq!(parse_track(&v, None).unwrap().is_hq, expected);
        }
    }

    #[test]
    fn invalid_entries_are_skipped_not_fatal() {
        for v in [
            json!({"id": 0, "owner_id": 2, "artist": "a", "title": "t"}),
            json!({"id": -5, "owner_id": 2, "artist": "a", "title": "t"}),
            json!({"id": 1, "owner_id": 0, "artist": "a", "title": "t"}),
            json!({"id": 1, "owner_id": 2, "artist": "a"}),
            json!({"id": 1, "owner_id": 2, "title": "t"}),
            json!("ad"),
        ] {
            assert!(parse_track(&v, None).is_none(), "{v}");
        }
    }

    #[test]
    fn negative_owner_is_a_valid_group() {
        let v = json!({"id": 2, "owner_id": -2001, "artist": "A", "title": "B"});
        assert_eq!(parse_track(&v, None).unwrap().owner_id, -2001);
    }

    #[test]
    fn stream_url_must_be_https_after_normalisation() {
        let v = json!({"id": 1, "owner_id": 2, "artist": "a", "title": "t",
                       "url": "http://cs.example/a/index.m3u8"});
        assert_eq!(parse_track(&v, Some(1)).unwrap().stream_url.as_deref(),
                   Some("https://cs.example/a/index.m3u8"));
        let bad = json!({"id": 1, "owner_id": 2, "artist": "a", "title": "t", "url": "file:///x"});
        assert!(parse_track(&bad, Some(1)).unwrap().stream_url.is_none());
    }

    #[test]
    fn playlist_without_title_and_with_thumbs() {
        let v = json!({"id": "5", "owner_id": 9, "count": "12",
                       "thumbs": [{"photo_600": "https://cdn.example/p.jpg"}]});
        let p = parse_playlist(&v).unwrap();
        assert_eq!(p.id(), "9_5");
        assert_eq!(p.count, 12);
        assert_eq!(p.title, "Playlist");
        assert_eq!(p.artwork_url.as_deref(), Some("https://cdn.example/p.jpg"));
    }

    #[test]
    fn paging_advances_by_raw_count_and_stops() {
        assert_eq!(next_offset(0, 100, 250), Some(100));
        assert_eq!(next_offset(200, 50, 250), None);
        assert_eq!(next_offset(0, 0, 250), None);
    }
}
