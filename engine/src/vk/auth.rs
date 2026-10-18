//! Превращает зарегистрированный веб-сеанс vk.ru (его файлы cookie) в токен API — тот же обмен, который
//! выполняет приложение iOS (`login.vk.ru/?act=web_token`).

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT};
use serde_json::Value;

use super::client::{retry_delay, Session};
use super::model::int;
use crate::{Error, Result};

/// Идентификатор общедоступного веб-клиента VK, который использует сам веб-клиент vk.ru.
const WEB_CLIENT_ID: &str = "6287487";
const TOKEN_URL: &str = "https://login.vk.ru/?act=web_token";
/// Аудио-методы отвечают по-разному в зависимости от пользовательского агента мобильного приложения.
pub const API_USER_AGENT: &str = "KateMobileAndroid/56 lite-460 (Android 4.4.2; SDK 19; x86; unknown Android SDK built for x86; ru)";

pub fn exchange_web_session(cookie_header: &str, web_user_agent: &str) -> Result<Session> {
    exchange_at(TOKEN_URL, cookie_header, web_user_agent, now_unix())
}

pub(crate) fn exchange_at(url: &str, cookie_header: &str, web_user_agent: &str, now: i64) -> Result<Session> {
    let cookies = cookie_header.trim();
    let agent = web_user_agent.trim();
    if cookies.is_empty() || agent.is_empty() {
        return Err(Error::Unauthorized);
    }
    let http = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    const ATTEMPTS: u32 = 3;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let last = attempt >= ATTEMPTS;
        let sent = http
            .post(url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded; charset=utf-8")
            .header(ACCEPT, "application/json")
            .header(ORIGIN, "https://vk.ru")
            .header(REFERER, "https://vk.ru/")
            .header(COOKIE, cookies)
            .header(USER_AGENT, agent)
            .body(format!("version=1&app_id={WEB_CLIENT_ID}"))
            .send();
        let response = match sent {
            Ok(r) => r,
            Err(err) => {
                let mapped = Error::from(err);
                if !last && matches!(mapped, Error::Timeout | Error::Offline | Error::Transport(_)) {
                    std::thread::sleep(Duration::from_secs_f64(retry_delay(attempt - 1, None, 1.0)));
                    continue;
                }
                return Err(mapped);
            }
        };
        let status = response.status().as_u16();
        if matches!(status, 429 | 500..=599) && !last {
            std::thread::sleep(Duration::from_secs_f64(retry_delay(attempt - 1, None, 1.0)));
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(Error::Format(format!("web_token status {status}")));
        }
        let set_cookies: Vec<String> = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        let body: Value = response.json().map_err(|_| Error::Format("web_token body".into()))?;
        return decode(&body, merge_cookies(cookies, &set_cookies), agent, now);
    }
}

fn decode(body: &Value, cookies: String, web_user_agent: &str, now: i64) -> Result<Session> {
    if body.get("type").and_then(Value::as_str) != Some("okay") {
        // На странице не выполнен вход (или VK отказался): пользователю необходимо войти снова.
        return Err(Error::Unauthorized);
    }
    let data = body.get("data").ok_or_else(|| Error::Format("web_token data".into()))?;
    let token = data
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| t.len() >= 16)
        .ok_or_else(|| Error::Format("web_token access_token".into()))?;
    let expires_at = data.get("expires").and_then(int).filter(|raw| *raw > 0).map(|raw| {
        // Текущий web_token возвращает секунды Unix; сохраняйте форму продолжительности работающей.
        if raw > 1_000_000_000 { raw } else { now + raw }
    });
    Ok(Session {
        access_token: token.to_string(),
        user_id: data.get("user_id").and_then(int),
        expires_at,
        api_user_agent: API_USER_AGENT.to_string(),
        refresh_cookie: cookies,
        web_user_agent: web_user_agent.to_string(),
    })
}

/// Исходные файлы cookie, наложенные на имена VK, повторно заданные в ответе.
pub(crate) fn merge_cookies(original: &str, set_cookie_headers: &[String]) -> String {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    for part in original.split(';') {
        if let Some((name, value)) = part.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                values.insert(name.to_string(), value.to_string());
            }
        }
    }
    for header in set_cookie_headers {
        let pair = header.split(';').next().unwrap_or("");
        if let Some((name, value)) = pair.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                values.insert(name.to_string(), value.trim().to_string());
            }
        }
    }
    values.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
}

pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Session {
    /// Обновите немного, прежде чем токен умрет.
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.expires_at.map(|at| at - now < 300).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_unix_and_duration_expiry() {
        let ok = |expires: i64| {
            json!({"type": "okay", "data": {"access_token": "0123456789abcdef0123", "user_id": 77, "expires": expires}})
        };
        let s = decode(&ok(1_900_000_000), "a=b".into(), "ua", 1_000).unwrap();
        assert_eq!(s.expires_at, Some(1_900_000_000));
        assert_eq!(s.user_id, Some(77));
        let s = decode(&ok(3600), "a=b".into(), "ua", 1_000).unwrap();
        assert_eq!(s.expires_at, Some(4_600));
        let s = decode(&ok(0), "a=b".into(), "ua", 1_000).unwrap();
        assert_eq!(s.expires_at, None);
    }

    #[test]
    fn refusal_and_short_tokens_are_errors() {
        assert_eq!(decode(&json!({"type": "error"}), String::new(), "ua", 0).unwrap_err(), Error::Unauthorized);
        let short = json!({"type": "okay", "data": {"access_token": "short"}});
        assert!(matches!(decode(&short, String::new(), "ua", 0), Err(Error::Format(_))));
    }

    #[test]
    fn cookie_merge_overlays_response_cookies() {
        let merged = merge_cookies(
            "remixsid=old; remixlang=0",
            &["remixsid=new; Path=/; HttpOnly".to_string(), "remixnsid=fresh; Secure".to_string()],
        );
        assert_eq!(merged, "remixlang=0; remixnsid=fresh; remixsid=new");
    }

    #[test]
    fn empty_inputs_mean_not_signed_in() {
        assert_eq!(exchange_web_session("", "ua").unwrap_err(), Error::Unauthorized);
        assert_eq!(exchange_web_session("a=b", "  ").unwrap_err(), Error::Unauthorized);
    }

    #[test]
    fn refresh_window() {
        let mut s = decode(
            &json!({"type":"okay","data":{"access_token":"0123456789abcdef0123","expires":2_000_000_000i64}}),
            String::new(), "ua", 0,
        ).unwrap();
        assert!(!s.needs_refresh(1_000));
        assert!(s.needs_refresh(2_000_000_000 - 100));
        s.expires_at = None;
        assert!(!s.needs_refresh(5));
    }
}
