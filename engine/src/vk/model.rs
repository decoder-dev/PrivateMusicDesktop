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
    })
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
