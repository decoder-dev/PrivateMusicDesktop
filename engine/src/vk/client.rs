use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER, USER_AGENT};
use serde_json::Value;
use url::Url;

use super::model::{self, Page, Playlist, Profile, Track};
use crate::{Error, Result};

pub const DEFAULT_BASE_URL: &str = "https://api.vk.ru";
pub const API_VERSION: &str = "5.199";

/// What the API needs from a signed-in user. Held by the app, never logged.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub access_token: String,
    pub user_id: Option<i64>,
    /// Unix seconds, when VK told us.
    pub expires_at: Option<i64>,
    /// Audio methods answer differently depending on the app user agent.
    pub api_user_agent: String,
    /// Cookie header used to re-issue the token without a new sign-in.
    pub refresh_cookie: String,
    pub web_user_agent: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Secrets must not reach a log line through `{:?}`.
        f.debug_struct("Session")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct VkClient {
    http: Client,
    base: Url,
    version: String,
}

/// Seconds to wait before a retry. `Retry-After` is server-controlled text:
/// only a finite, non-negative number is believed, everything else falls
/// back to the exponential schedule. Always within 0.2 – 8 s.
pub fn retry_delay(attempt: u32, retry_after: Option<&str>, jitter: f64) -> f64 {
    let announced = retry_after
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0);
    let exponential = (2f64.powi(attempt.min(10) as i32) * 0.55).min(4.0);
    announced.unwrap_or(exponential * jitter).clamp(0.2, 8.0)
}

impl VkClient {
    pub fn new() -> Result<Self> {
        Self::with_base(DEFAULT_BASE_URL)
    }

    /// `base` must be https. Plain http is accepted only for a loopback host
    /// so tests can talk to a local server.
    pub fn with_base(base: &str) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| Error::InvalidRequest)?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
        if base.scheme() != "https" && !(loopback && base.scheme() == "http") {
            return Err(Error::InvalidRequest);
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            // The token travels in the body: a 307/308 would replay it at
            // whatever host the response names.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { http, base, version: API_VERSION.to_string() })
    }

    fn url(&self, method: &str) -> Result<Url> {
        if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
            return Err(Error::InvalidRequest);
        }
        self.base.join(&format!("method/{method}")).map_err(|_| Error::InvalidRequest)
    }

    /// POSTs a VK method and returns the `response` value.
    pub fn call(&self, session: &Session, method: &str, params: &[(&str, String)]) -> Result<Value> {
        let url = self.url(method)?;
        let mut form: Vec<(&str, String)> = vec![
            ("access_token", session.access_token.clone()),
            ("v", self.version.clone()),
            ("https", "1".into()),
            ("lang", "ru".into()),
        ];
        form.extend(params.iter().map(|(k, v)| (*k, v.clone())));

        const ATTEMPTS: u32 = 3;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let last = attempt >= ATTEMPTS;
            let response = self
                .http
                .post(url.clone())
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded; charset=utf-8")
                .header(ACCEPT, "application/json")
                .header(USER_AGENT, session.api_user_agent.as_str())
                .form(&form)
                .send();
            let response = match response {
                Ok(r) => r,
                Err(err) => {
                    let mapped = Error::from(err);
                    if !last && matches!(mapped, Error::Timeout | Error::Offline | Error::Transport(_)) {
                        sleep(retry_delay(attempt - 1, None, 1.0));
                        continue;
                    }
                    return Err(mapped);
                }
            };
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = response.text().map_err(Error::from)?;
            log::debug!("POST {method} status={status} bytes={}", body.len());

            if matches!(status, 408 | 429 | 500 | 502 | 503 | 504) && !last {
                sleep(retry_delay(attempt - 1, retry_after.as_deref(), 1.0));
                continue;
            }
            let parsed: Option<Value> = serde_json::from_str(&body).ok();
            let vk_error = parsed.as_ref().and_then(|v| v.get("error"));
            let vk_code = vk_error.and_then(|e| e.get("error_code")).and_then(model::int);
            if status == 401 || vk_code == Some(5) {
                return Err(Error::Unauthorized);
            }
            if !(200..300).contains(&status) {
                return Err(Error::Http(status));
            }
            if let Some(error) = vk_error {
                let code = vk_code.unwrap_or(0);
                if matches!(code, 6 | 10) && !last {
                    sleep(retry_delay(attempt - 1, None, 1.0));
                    continue;
                }
                let message = error.get("error_msg").and_then(Value::as_str).unwrap_or("").to_string();
                return Err(Error::Vk { code, message });
            }
            return parsed
                .and_then(|v| v.get("response").cloned())
                .ok_or_else(|| Error::Format(format!("{method}: no response field")));
        }
    }

    pub fn profile(&self, session: &Session) -> Result<Profile> {
        let value = self.call(session, "users.get", &[("fields", "photo_200".into())])?;
        value
            .as_array()
            .and_then(|list| list.first())
            .and_then(model::parse_profile)
            .ok_or_else(|| Error::Format("users.get: empty".into()))
    }

    /// The user's id: from the session, else asked from VK once.
    pub fn user_id(&self, session: &Session) -> Result<i64> {
        match session.user_id {
            Some(id) => Ok(id),
            None => self.profile(session).map(|p| p.id),
        }
    }

    pub fn library(&self, session: &Session, offset: u32, count: u32) -> Result<Page<Track>> {
        let user = self.user_id(session)?;
        let value = self.call(
            session,
            "audio.get",
            &[("owner_id", user.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        )?;
        Ok(tracks_page(&value, offset, Some(user)))
    }

    /// VK's own plain text for a track's `lyrics_id`.
    pub fn lyrics_text(&self, session: &Session, lyrics_id: i64) -> Result<Option<String>> {
        let value = self.call(session, "audio.getLyrics", &[("lyrics_id", lyrics_id.to_string())])?;
        Ok(value.get("text").and_then(Value::as_str).map(str::to_string))
    }

    pub fn search(&self, session: &Session, query: &str, offset: u32, count: u32) -> Result<Page<Track>> {
        let user = session.user_id;
        let value = self.call(
            session,
            "audio.search",
            &[
                ("q", query.to_string()),
                ("count", count.to_string()),
                ("offset", offset.to_string()),
                ("auto_complete", "1".into()),
                ("sort", "2".into()),
            ],
        )?;
        Ok(tracks_page(&value, offset, user))
    }

    pub fn playlists(&self, session: &Session, offset: u32, count: u32) -> Result<Page<Playlist>> {
        let user = self.user_id(session)?;
        let value = self.call(
            session,
            "audio.getPlaylists",
            &[("owner_id", user.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        )?;
        let (entries, raw_count) = collect_entries(&value);
        let items: Vec<Playlist> = entries.iter().filter_map(model::parse_playlist).collect();
        let total = value.get("count").and_then(model::int).unwrap_or(0).max(0) as u32;
        let total = total.max(offset + raw_count);
        Ok(Page { items, total, next_offset: model::next_offset(offset, raw_count, total) })
    }

    pub fn playlist_tracks(
        &self,
        session: &Session,
        playlist: &Playlist,
        offset: u32,
        count: u32,
    ) -> Result<Page<Track>> {
        let mut params = vec![
            ("owner_id", playlist.owner_id.to_string()),
            ("album_id", playlist.playlist_id.to_string()),
            ("count", count.to_string()),
            ("offset", offset.to_string()),
        ];
        if let Some(key) = playlist.access_key.as_deref().filter(|k| !k.is_empty()) {
            params.push(("access_key", key.to_string()));
        }
        let value = self.call(session, "audio.get", &params)?;
        Ok(tracks_page(&value, offset, session.user_id))
    }

    /// Fresh stream URL for one track (`audio.getById`). Done right before
    /// play when the URL we hold may have expired.
    pub fn refreshed_track(&self, session: &Session, track: &Track) -> Result<Track> {
        let value = self.call(session, "audio.getById", &[("audios", track.audio_id_with_key())])?;
        let user = self.user_id(session).ok();
        let fresh = value
            .as_array()
            .and_then(|list| list.first())
            .and_then(|o| model::parse_track(o, user))
            .ok_or_else(|| Error::Format("audio.getById: empty".into()))?;
        if fresh.stream_url.is_none() {
            return Err(Error::Format("audio.getById: no stream url".into()));
        }
        Ok(fresh)
    }
}

fn sleep(seconds: f64) {
    std::thread::sleep(Duration::from_secs_f64(seconds));
}

fn tracks_page(value: &Value, offset: u32, user: Option<i64>) -> Page<Track> {
    let (entries, raw_count) = collect_entries(value);
    let items: Vec<Track> = entries.iter().filter_map(|e| model::parse_track(e, user)).collect();
    let total = value.get("count").and_then(model::int).map(|n| n.max(0) as u32).unwrap_or(offset + raw_count);
    Page { items, total, next_offset: model::next_offset(offset, raw_count, total) }
}

/// The entries of a list response and how many raw entries it held. The
/// response is `{count, items:[…]}` or, for some methods, a bare array.
fn collect_entries(value: &Value) -> (Vec<Value>, u32) {
    let list = match value {
        Value::Array(list) => list.clone(),
        Value::Object(_) => value.get("items").and_then(Value::as_array).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let raw = list.len() as u32;
    (list, raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_retry_after_values_never_escape_the_bounds() {
        for hostile in ["nan", "NaN", "inf", "-inf", "-5", "1e999", "", "soon", "Wed, 21 Oct 2026 07:28:00 GMT"] {
            let d = retry_delay(0, Some(hostile), 1.0);
            assert!(d.is_finite() && (0.2..=8.0).contains(&d), "{hostile} -> {d}");
            assert!((d - 0.55).abs() < 1e-9, "{hostile} falls back to the schedule, got {d}");
        }
        assert_eq!(retry_delay(0, Some("3"), 1.0), 3.0);
        assert_eq!(retry_delay(0, Some(" 2 "), 1.0), 2.0);
        assert_eq!(retry_delay(0, Some("120"), 1.0), 8.0);
        assert_eq!(retry_delay(0, Some("0"), 1.0), 0.2);
        assert_eq!(retry_delay(40, None, 1.0), 4.0);
    }

    #[test]
    fn base_url_must_be_https_or_loopback() {
        assert!(VkClient::with_base("https://api.vk.ru").is_ok());
        assert!(VkClient::with_base("http://api.vk.ru").is_err());
        assert!(VkClient::with_base("http://127.0.0.1:8080").is_ok());
        assert!(VkClient::with_base("file:///tmp").is_err());
    }

    #[test]
    fn method_names_cannot_escape_the_method_path() {
        let c = VkClient::with_base("https://api.vk.ru").unwrap();
        assert!(c.url("audio.get").is_ok());
        for bad in ["", "../x", "audio.get/../../y", "https://evil.example/", "a b", "audio.get?x=1"] {
            assert!(c.url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn session_debug_does_not_leak_secrets() {
        let s = Session {
            access_token: "SECRET-TOKEN".into(),
            user_id: Some(1),
            expires_at: None,
            api_user_agent: "ua".into(),
            refresh_cookie: "remixsid=SECRET-COOKIE".into(),
            web_user_agent: "wua".into(),
        };
        let shown = format!("{s:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }
}
