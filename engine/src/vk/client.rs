use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde_json::Value;
use url::Url;

use super::model::{self, Album, Artist, MusicMix, Page, Playlist, Profile, Track};
use crate::net::{self, Pool};
use crate::{Error, Result};

pub const DEFAULT_BASE_URL: &str = "https://api.vk.ru";
pub const API_VERSION: &str = "5.199";
const MAX_API_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INLINE_WAIT: f64 = 8.0;

/// Что нужно API от вошедшего в систему пользователя. Хранится приложением, никогда не регистрируется.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub access_token: String,
    pub user_id: Option<i64>,
    /// Unix-секунды, когда нам сообщил VK.
    pub expires_at: Option<i64>,
    /// Аудио-методы отвечают по-разному в зависимости от пользовательского агента приложения.
    pub api_user_agent: String,
    /// Заголовок файла cookie, используемый для повторного выпуска токена без нового входа в систему.
    pub refresh_cookie: String,
    pub web_user_agent: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Секреты не должны достигать строки журнала через `{:?}`.
        f.debug_struct("Session")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct VkClient {
    /// Восстановлено (все соединения в пуле разорваны) после транспортной ошибки или изменения сети;
    /// клоны разделяют это.
    http: Arc<Pool>,
    base: Url,
    version: String,
    cooldown: Arc<Mutex<Option<(Instant, u16)>>>,
}

/// Секунды ожидания перед повторной попыткой. `Retry-After` — это текст, управляемый сервером:
/// считается только конечное неотрицательное число, все остальное возвращается к экспоненциальному
/// графику. Длительное ожидание сохраняется в общем ограничителе запросов,
/// а вызывающему коду возвращается ошибка без блокировки потока на минуты.
fn announced_delay(raw: &str, now: SystemTime) -> Option<f64> {
    net::retry_after_at(raw, now).map(|delay| delay.as_secs_f64())
}

pub fn retry_delay(attempt: u32, retry_after: Option<&str>, jitter: f64) -> f64 {
    let announced = retry_after.and_then(|raw| announced_delay(raw, SystemTime::now()));
    let jitter = if jitter.is_finite() { jitter.clamp(0.0, 2.0) } else { 1.0 };
    let exponential = (2f64.powi(attempt.min(10) as i32) * 0.55).min(4.0);
    announced.unwrap_or(exponential * jitter).clamp(0.2, 86400.0)
}

fn read_api_body(mut reader: impl Read, limit: u64, budget: Duration) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let deadline = Instant::now() + budget;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        if Instant::now() >= deadline { return Err(Error::Timeout); }
        let n = reader.read(&mut chunk).map_err(|error| net::read_error(&error))?;
        if n == 0 { break; }
        if body.len() as u64 + n as u64 > limit { return Err(Error::Format("API response exceeds size limit".into())); }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

impl VkClient {
    pub fn new() -> Result<Self> {
        Self::with_base(DEFAULT_BASE_URL)
    }

    /// `base` должен быть https. Обычный http принимается только для хоста с обратной связью, поэтому
    /// тесты могут взаимодействовать с локальным сервером.
    pub fn with_base(base: &str) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| Error::InvalidRequest)?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
        if base.scheme() != "https" && !(loopback && base.scheme() == "http") {
            return Err(Error::InvalidRequest);
        }
        let http = net::shared_pool(|| {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(15))
                // Соединение в течение длительного времени простоя - это то, которое отключил
                // коммутатор VPN: не доверяйте ему и позвольте OS заметить мертвые узлы.
                .pool_idle_timeout(Duration::from_secs(20))
                .tcp_keepalive(Duration::from_secs(30))
                // Токен перемещается в теле: 307/308 будет воспроизводить его на любом хосте, на
                // котором указан ответ.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(Error::from)
        })?;
        Ok(Self { http, base, version: API_VERSION.to_string(), cooldown: Arc::new(Mutex::new(None)) })
    }

    pub fn cooldown_remaining(&self) -> Duration {
        self.cooldown.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
            .map(|(until, _)| until.saturating_duration_since(Instant::now())).unwrap_or_default()
    }

    fn active_limit(&self) -> Option<Error> {
        let current = *self.cooldown.lock().unwrap_or_else(|p| p.into_inner());
        current.filter(|(until,_)| *until > Instant::now()).map(|(until,status)| Error::RetryAfter {
            status, seconds:(until.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64).max(1),
        })
    }

    fn defer_requests(&self, seconds: f64, status: u16) {
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        let mut current = self.cooldown.lock().unwrap_or_else(|p| p.into_inner());
        if current.as_ref().is_none_or(|(old, _)| until > *old) { *current = Some((until, status)); }
    }

    /// Забывает каждое объединенное соединение: следующий вызов набирает номер снова.
    pub fn reset_connections(&self) {
        net::drop_connections();
    }

    fn url(&self, method: &str) -> Result<Url> {
        if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
            return Err(Error::InvalidRequest);
        }
        self.base.join(&format!("method/{method}")).map_err(|_| Error::InvalidRequest)
    }

    /// POSTs является методом VK и возвращает значение `response`. Вызовы только для чтения повторяются
    /// в случае временных сбоев.
    pub fn call(&self, session: &Session, method: &str, params: &[(&str, String)]) -> Result<Value> {
        self.call_with(session, method, params, 3)
    }

    /// Для вызовов, которые что-то меняют (добавление, удаление): повторная попытка после тайм-аута или
    /// 5xx может применить изменение дважды, поэтому будет ровно одна попытка.
    pub fn call_once(&self, session: &Session, method: &str, params: &[(&str, String)]) -> Result<Value> {
        // Лимит, известный до отправки, не является неподтверждённой записью.
        if let Some(error) = self.active_limit() { return Err(error); }
        match self.call_with(session, method, params, 1) {
            Err(error) if matches!(error, Error::Timeout | Error::Transport(_) | Error::Format(_) | Error::Http(408 | 500..=599) | Error::RetryAfter { status:500..=599,.. }) => {
                log::warn!("{method}: write result is unconfirmed kind={}", error.kind());
                Err(Error::UnconfirmedWrite)
            }
            result => result,
        }
    }

    /// Чтение, которое может быть выполнено всего `attempts` раз (вызывающая сторона выполняет
    /// собственное ожидание между раундами, как это делает восстановление потока).
    pub fn call_limited(&self, session: &Session, method: &str, params: &[(&str, String)], attempts: u32) -> Result<Value> {
        self.call_with(session, method, params, attempts.max(1))
    }

    /// После сбоя транспорта соединения в пуле вызывают подозрение (коммутатор VPN оставляет
    /// неработающие сокеты): отбросьте их, чтобы следующий запрос набрался снова. Возвращает true,
    /// когда вызывающий абонент должен повторить попытку сейчас, после того как отсрочка была отложена.
    fn transport_failed(&self, method: &str, error: &Error, attempt: u32, attempts: u32) -> bool {
        if !net::is_connectivity(error) {
            return false;
        }
        self.reset_connections();
        if attempt >= attempts {
            log::warn!("{method}: {} (attempt {attempt}/{attempts}), giving up", error.kind());
            return false;
        }
        log::info!("{method}: {} → new connections (attempt {}/{attempts})", error.kind(), attempt + 1);
        sleep(retry_delay(attempt - 1, None, net::jitter_unit() * 0.3 + 0.85));
        true
    }

    fn call_with(&self, session: &Session, method: &str, params: &[(&str, String)], attempts: u32) -> Result<Value> {
        self.call_budgeted(session, method, params, attempts, None)
    }

    /// Проверка доступности VK всегда обращается к серверу и не использует кеш профиля.
    /// Одна попытка с коротким ожиданием не запускает каскад обновлений сессии.
    pub fn probe_network(&self, session: &Session) -> Result<()> {
        if let Some(error) = self.active_limit() { return Err(error); }
        // Проверка новой сети не должна использовать сокет, оставшийся от старой.
        self.reset_connections();
        let value = self.call_budgeted(session, "users.get", &[], 1, Some(Duration::from_secs(8)))?;
        if value.as_array().and_then(|items| items.first()).and_then(model::parse_profile).is_none() {
            return Err(Error::Format("network probe: invalid profile".into()));
        }
        Ok(())
    }

    fn call_budgeted(&self, session: &Session, method: &str, params: &[(&str, String)], attempts: u32, timeout: Option<Duration>) -> Result<Value> {
        let url = self.url(method)?;
        let mut form: Vec<(&str, String)> = vec![
            ("access_token", session.access_token.clone()),
            ("v", self.version.clone()),
            ("https", "1".into()),
            ("lang", "ru".into()),
        ];
        form.extend(params.iter().map(|(k, v)| (*k, v.clone())));

        let mut attempt = 0;
        loop {
            if let Some(error) = self.active_limit() { return Err(error); }
            attempt += 1;
            let last = attempt >= attempts;
            let mut request = self
                .http
                .get()
                .post(url.clone())
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded; charset=utf-8")
                .header(ACCEPT, "application/json")
                .header(USER_AGENT, session.api_user_agent.as_str())
                .form(&form);
            if let Some(timeout) = timeout { request = request.timeout(timeout); }
            let sent = request.send();
            let response = match sent {
                Ok(r) => r,
                Err(err) => {
                    let mapped = Error::from(err);
                    if self.transport_failed(method, &mapped, attempt, attempts) {
                        continue;
                    }
                    return Err(mapped);
                }
            };
            let status = response.status().as_u16();
            let announced = net::retry_after(response.headers()).map(|delay| delay.as_secs_f64());
            if status == 401 { return Err(Error::Unauthorized); }
            if matches!(status, 408 | 429 | 500 | 502 | 503 | 504) {
                let delay = announced.unwrap_or_else(|| retry_delay(attempt - 1, None, net::jitter_unit() * 0.3 + 0.85)).max(0.2);
                if status == 429 || announced.is_some() { self.defer_requests(delay, status); }
                if !last && delay <= MAX_INLINE_WAIT {
                    log::info!("{method}: HTTP {status}, retrying (attempt {}/{attempts}, delay_ms={})", attempt + 1, (delay * 1000.0) as u64);
                    sleep(delay);
                    continue;
                }
                if let Some(delay) = announced {
                    return Err(Error::RetryAfter {status,seconds:(delay.ceil() as u64).max(1)});
                }
                return Err(Error::Http(status));
            }
            let body = match read_api_body(response, MAX_API_BYTES, timeout.unwrap_or(Duration::from_secs(30))) {
                Ok(body) => body,
                Err(err) => {
                    // Соединение прервалось, пока приходил ответ.
                    let mapped = err;
                    if self.transport_failed(method, &mapped, attempt, attempts) {
                        continue;
                    }
                    return Err(mapped);
                }
            };
            log::debug!("POST {method} status={status} bytes={}", body.len());

            let parsed: Option<Value> = serde_json::from_slice(&body).ok();
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
                let delay = announced.unwrap_or_else(|| retry_delay(attempt - 1, None, net::jitter_unit() * 0.3 + 0.85)).max(0.2);
                if matches!(code, 6 | 9) { self.defer_requests(delay, 429); }
                // 6: слишком много запросов в секунду, 9: контроль флуда, 10: внутренний.
                if matches!(code, 6 | 9 | 10) && !last && delay <= MAX_INLINE_WAIT {
                    log::info!("{method}: VK error {code}, retrying (attempt {}/{attempts})", attempt + 1);
                    sleep(delay);
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

    /// Идентификатор пользователя: из сеанса, иначе запросили один раз у VK.
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

    /// Копирует трек в пользовательский «Моя музыка». Возвращает копию, имеющую собственный идентификатор
    /// и пользователя в качестве владельца.
    pub fn add_track(&self, session: &Session, track: &Track) -> Result<Track> {
        if track.track_id <= 0 || track.owner_id == 0 {
            return Err(Error::InvalidRequest);
        }
        let mut params = vec![("audio_id", track.track_id.to_string()), ("owner_id", track.owner_id.to_string())];
        if let Some(key) = track.access_key.as_deref().filter(|k| !k.is_empty()) {
            params.push(("access_key", key.to_string()));
        }
        let value = self.call_once(session, "audio.add", &params)?;
        let id = model::int(&value).filter(|id| *id > 0).ok_or_else(|| Error::Format("audio.add: no id".into()))?;
        let owner = match session.user_id {
            Some(user) => user,
            None => self.user_id(session)?,
        };
        Ok(Track { track_id: id, owner_id: owner, access_key: None, ..track.clone() })
    }

    /// Удаляет трек из пользовательского «Моя музыка» (`track` — пользовательская копия).
    pub fn delete_track(&self, session: &Session, track: &Track) -> Result<()> {
        if track.track_id <= 0 || track.owner_id == 0 {
            return Err(Error::InvalidRequest);
        }
        self.call_once(
            session,
            "audio.delete",
            &[("audio_id", track.track_id.to_string()), ("owner_id", track.owner_id.to_string())],
        )?;
        Ok(())
    }

    /// Рекомендации VK для пользователя или «Похожие треки», если задан исходный трек (собственное
    /// радио-из-трека VK).
    pub fn recommendations(&self, session: &Session, seed: Option<&Track>) -> Result<Vec<Track>> {
        self.recommendations_with(session, seed, false)
    }

    /// Пополнения радио: перетасованы, как просит радиокурсор iOS, чтобы повторно пришедшее семя не
    /// давало одну и ту же страницу дважды.
    pub fn shuffled_recommendations(&self, session: &Session, seed: &Track) -> Result<Vec<Track>> {
        self.recommendations_with(session, Some(seed), true)
    }

    fn recommendations_with(&self, session: &Session, seed: Option<&Track>, shuffle: bool) -> Result<Vec<Track>> {
        let user = match session.user_id {
            Some(user) => Some(user),
            None => self.user_id(session).ok(),
        };
        let mut params = vec![("count", "100".to_string()), ("shuffle", if shuffle { "1" } else { "0" }.to_string())];
        if let Some(user) = user {
            params.push(("user_id", user.to_string()));
        }
        if let Some(seed) = seed {
            params.push(("target_audio", format!("{}_{}", seed.owner_id, seed.track_id)));
        }
        let value = self.call(session, "audio.getRecommendations", &params)?;
        let mut tracks = tracks_page(&value, 0, user).items;
        if let Some(seed) = seed {
            tracks.retain(|t| t.id() != seed.id());
        }
        Ok(dedupe(tracks))
    }

    /// Одна страница личного микса VK пользователя (бесконечный поток).
    pub fn mix(&self, session: &Session, offset: u32, count: u32) -> Result<Vec<Track>> {
        self.mix_with_options(session, offset, count, None)
    }

    /// Одна страница личного потока с примененными параметрами настройки VK.
    pub fn mix_with_options(&self, session: &Session, offset: u32, count: u32, options: Option<&str>) -> Result<Vec<Track>> {
        self.stream_mix(session, "common", offset, count, options)
    }

    /// Одна страница микса каталога. IDs проверяются границей приложения.
    pub fn stream_mix(&self, session: &Session, mix_id: &str, offset: u32, count: u32, options: Option<&str>) -> Result<Vec<Track>> {
        let mut params = vec![
            ("mix_id", mix_id.to_string()),
            ("count", count.to_string()),
            ("offset", offset.to_string()),
        ];
        if let Some(options) = options {
            params.push(("mix_options", options.to_string()));
        }
        let value = self.call(session, "audio.getStreamMixAudios", &params)?;
        let mut found = Vec::new();
        collect_tracks(&value, session.user_id, &mut found);
        Ok(dedupe(found))
    }

    /// С чего начинается экран микса или новая очередь: сразу несколько страниц, как запрашивает
    /// приложение iOS (`MixTrackRequestPolicy`: страницы по 100 со смещениями 0, 100, 200, 300), потому
    /// что микс потока часто отвечает на страницу только с несколькими треками. Страницы, которые
    /// терпят неудачу, пропускаются, в то время как любая другая что-то приносит; сообщается о
    /// неактивном сеансе, поэтому вызывающий абонент возобновляет его.
    pub fn mix_window(&self, session: &Session, mix_id: &str, options: Option<&str>) -> Result<Vec<Track>> {
        self.mix_window_at(session, mix_id, options, 0)
    }

    /// Следующая порция микса; ограничение очереди не обрезает список на экране.
    pub fn mix_window_at(&self, session: &Session, mix_id: &str, options: Option<&str>, offset: u32) -> Result<Vec<Track>> {
        use crate::radio::{PAGE_SIZE, WINDOW_PAGES};
        let pages: Vec<Result<Vec<Track>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..WINDOW_PAGES)
                .map(|page| scope.spawn(move || self.stream_mix(session, mix_id, offset + page * PAGE_SIZE, PAGE_SIZE, options)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or(Err(Error::Cancelled))).collect()
        });
        if pages.iter().any(|page| matches!(page, Err(Error::Unauthorized))) {
            return Err(Error::Unauthorized);
        }
        let mut known = std::collections::HashSet::new();
        let mut tracks = Vec::new();
        let mut first_error = None;
        for page in pages {
            match page {
                Ok(page) => tracks.extend(page.into_iter().filter(|t| known.insert(t.id()))),
                Err(error) => first_error = first_error.or(Some(error)),
            }
        }
        if tracks.is_empty() {
            return match first_error {
                Some(error) => Err(error),
                None => Ok(tracks),
            };
        }
        Ok(tracks)
    }

    /// Настроение, активность и выбор жанра VK для личного микса стрима.
    pub fn mix_settings(&self, session: &Session) -> Result<Value> {
        self.call(session, "audio.getStreamMixSettings", &[("mix_id", "common".into())])
    }

    /// Домашний каталог VK смешивает по мере того, как клиент iOS создает их: миксы, найденные в
    /// корневой полезной нагрузке `catalog.getAudio`, а также миксы из каждого раздела каталога,
    /// который выглядит как полка микса (`catalog.getSection`, не более восьми, параллельно).
    /// Персональный микс `common` всегда стоит на первом месте, поэтому вошедший в систему пользователь
    /// никогда не увидит пустую полку.
    pub fn catalog_mixes(&self, session: &Session) -> Result<Vec<MusicMix>> {
        let snapshot = self.catalog_snapshot(session, true, false)?;
        let mut mixes = snapshot.mixes;
        match mixes.iter().position(|mix| mix.id == COMMON_MIX_ID && mix.options.is_none()) {
            Some(index) => {
                let common = mixes.remove(index);
                mixes.insert(0, common);
            }
            None => mixes.insert(0, common_mix()),
        }
        if mixes.iter().all(|mix| mix.id == COMMON_MIX_ID) {
            log::warn!("catalog mixes: none found beyond the personal one; {}", snapshot.shape);
        }
        Ok(mixes)
    }

    /// Полка «Новые релизы» в том виде, в котором ее строит клиент iOS: альбомы в форме релизов из
    /// корневой полезной нагрузки `catalog.getAudio` и, только когда ее нет, альбомы разделов каталога,
    /// которые выглядят как полки релизов (`catalog.getSection`, максимум восемь, параллельно).
    /// Неисправный раздел удаляется; мертвая сессия или сеть нет.
    pub fn new_releases(&self, session: &Session) -> Result<Vec<Album>> {
        let snapshot = self.catalog_snapshot(session, false, true)?;
        // Альбом, окно которого не удалось открыть снова, не стоит и карты.
        let releases: Vec<Album> = snapshot.releases.into_iter().filter(|album| album.album_id > 0).collect();
        if releases.is_empty() {
            log::warn!("catalog new releases: none found; {}", snapshot.shape);
        }
        Ok(releases)
    }

    /// `catalog.getAudio` и гидратация секции за ним (iOS `catalogSnapshot`). Увлажняется только то,
    /// что хочет вызывающий абонент.
    fn catalog_snapshot(&self, session: &Session, want_mixes: bool, want_releases: bool) -> Result<CatalogSnapshot> {
        let mut mixes = Vec::new();
        let mut releases = Vec::new();
        let mut sections = Vec::new();
        let mut shape = String::from("catalog.getAudio failed");
        match self.call(session, "catalog.getAudio", &[("need_blocks", "1".into())]) {
            Ok(root) => {
                collect_mixes(&root, None, &mut mixes);
                collect_albums(&root, &mut releases);
                collect_catalog_sections(&root, &mut sections);
                unique_sections(&mut sections);
                shape = catalog_shape(&root, mixes.len(), releases.len(), &sections);
            }
            Err(error) if is_fatal(&error) => return Err(error),
            // Гидратация раздела ниже может по-прежнему восстанавливать полезный контент.
            Err(error) => log::warn!("catalog.getAudio failed: {}", error.kind()),
        }
        let mut mixes = dedupe_mixes(mixes);
        let mut releases = dedupe_by(releases, Album::id);

        if want_mixes {
            let wanted: Vec<&CatalogSection> = sections.iter().filter(|s| s.looks_like_mix()).take(HYDRATION_LIMIT).collect();
            let mut hydrated = Vec::new();
            let payloads = self.hydrate(session, &wanted)?;
            shape.push_str(&format!("; mix sections asked={} answered={}", wanted.len(), payloads.len()));
            for (section, payload) in payloads {
                collect_mixes(&payload, Some(section.title.trim()), &mut hydrated);
            }
            // Добавлены новые миксы; тот, который уже был в руте, только получил то, чего не хватало
            // руту (обложка, подзаголовок).
            mixes.extend(hydrated);
            mixes = dedupe_mixes(mixes);
        }
        if want_releases && releases.is_empty() {
            let wanted: Vec<&CatalogSection> = sections.iter().filter(|s| s.looks_like_releases()).take(HYDRATION_LIMIT).collect();
            let mut hydrated = Vec::new();
            let payloads = self.hydrate(session, &wanted)?;
            shape.push_str(&format!("; release sections asked={} answered={}", wanted.len(), payloads.len()));
            for (_, payload) in payloads {
                collect_albums(&payload, &mut hydrated);
            }
            releases = dedupe_by(hydrated, Album::id);
        }
        Ok(CatalogSnapshot { mixes, releases, shape })
    }

    /// `catalog.getSection` для каждой секции параллельно. Раздел, который потерпел неудачу, удаляется
    /// (и регистрируется по типу); мертвая сессия - нет.
    fn hydrate<'a>(&self, session: &Session, sections: &[&'a CatalogSection]) -> Result<Vec<(&'a CatalogSection, Value)>> {
        let results: Vec<Result<Value>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = sections
                .iter()
                .map(|section| scope.spawn(move || self.call(session, "catalog.getSection", &[("section_id", section.id.clone())])))
                .collect();
            jobs.into_iter().map(|job| job.join().unwrap_or(Err(Error::Cancelled))).collect()
        });
        let mut payloads = Vec::new();
        for (section, result) in sections.iter().zip(results) {
            match result {
                Ok(payload) => payloads.push((*section, payload)),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => log::warn!(
                    "catalog.getSection dropped: {} (section \"{}\", id length {})",
                    error.kind(),
                    shown(&section.title),
                    section.id.len()
                ),
            }
        }
        Ok(payloads)
    }

    /// Собственный простой текст VK для `lyrics_id` трека.
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
        self.playlists_of(session, user, offset, count)
    }

    fn playlists_of(&self, session: &Session, owner_id: i64, offset: u32, count: u32) -> Result<Page<Playlist>> {
        let value = self.call(
            session,
            "audio.getPlaylists",
            &[("owner_id", owner_id.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        )?;
        let (entries, raw_count) = collect_entries(&value);
        // Последующие выпуски возвращаются в тот же список; им место на полке «Альбомы»
        // (`followed_albums`), и здесь остаются только те, что VK обозначены как релиз вне всякого
        // сомнения.
        let items: Vec<Playlist> = entries
            .iter()
            .filter_map(library_entry)
            .filter(|entry| !looks_like_followed_album(entry))
            .filter_map(model::parse_playlist)
            .collect();
        let total = value.get("count").and_then(model::int).unwrap_or(0).max(0) as u32;
        let total = total.max(offset + raw_count);
        Ok(Page { items, total, next_offset: model::next_offset(offset, raw_count, total) })
    }

    /// Друзья пользователя (`friends.get`) по имени и с их фотографиями.
    pub fn friends(&self, session: &Session, offset: u32, count: u32) -> Result<Page<Profile>> {
        let value = self.call(
            session,
            "friends.get",
            &[
                ("fields", "photo_200".into()),
                ("order", "name".into()),
                ("offset", offset.to_string()),
                ("count", count.to_string()),
            ],
        )?;
        let (entries, raw_count) = collect_entries(&value);
        // Удаленные или заблокированные учетные записи возвращаются без имени: их нет в списке.
        let items: Vec<Profile> =
            entries.iter().filter(|entry| entry.get("deactivated").is_none()).filter_map(model::parse_profile).collect();
        let total = value.get("count").and_then(model::int).map(|n| n.max(0) as u32).unwrap_or(0).max(offset + raw_count);
        Ok(Page { items, total, next_offset: model::next_offset(offset, raw_count, total) })
    }

    /// «Моя музыка» друга (`audio.get` его идентификатора). VK отказывается, когда они сохраняют
    /// конфиденциальность; это возвращается как обычная ошибка VK.
    pub fn friend_tracks(&self, session: &Session, owner_id: i64, offset: u32, count: u32) -> Result<Page<Track>> {
        let user = self.user_id(session)?;
        let value = self.call(
            session,
            "audio.get",
            &[("owner_id", owner_id.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        )?;
        Ok(tracks_page(&value, offset, Some(user)))
    }

    /// Плейлисты друга (`audio.getPlaylists` его идентификатора).
    pub fn friend_playlists(&self, session: &Session, owner_id: i64, offset: u32, count: u32) -> Result<Page<Playlist>> {
        self.playlists_of(session, owner_id, offset, count)
    }

    pub(super) fn own_playlist(&self, session: &Session, playlist: &Playlist) -> Result<()> {
        let user = self.user_id(session)?;
        if playlist.owner_id != user || playlist.playlist_id <= 0 {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }

    fn clean_title(title: &str) -> Result<String> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 128 {
            return Err(Error::InvalidRequest);
        }
        Ok(title.to_string())
    }

    fn clean_description(description: &str) -> Result<String> {
        let description = description.trim();
        if description.chars().count() > 2000 {
            return Err(Error::InvalidRequest);
        }
        Ok(description.to_string())
    }

    pub fn create_playlist(&self, session: &Session, title: &str, description: &str) -> Result<Playlist> {
        self.create_playlist_with_discovery(session, title, description, None)
    }

    /// `None` preserves VK's default; this controls discovery, not access rights.
    pub fn create_playlist_with_discovery(&self, session: &Session, title: &str, description: &str, hidden: Option<bool>) -> Result<Playlist> {
        let title = Self::clean_title(title)?;
        let description = Self::clean_description(description)?;
        let user = self.user_id(session)?;
        let mut params = vec![("owner_id", user.to_string()), ("title", title.clone()), ("description", description.clone())];
        if let Some(hidden) = hidden { params.push(("no_discover", if hidden { "1" } else { "0" }.into())); }
        let value = self.call_once(session, "audio.createPlaylist", &params)?;
        // VK отвечает новым плейлистом; заполните то, что может быть упущено.
        let mut playlist = model::parse_playlist(&value).ok_or_else(|| Error::Format("audio.createPlaylist".into()))?;
        playlist.owner_id = user;
        if playlist.title.is_empty() || playlist.title == "Playlist" {
            playlist.title = title;
        }
        if playlist.description.is_none() && !description.is_empty() {
            playlist.description = Some(description);
        }
        Ok(playlist)
    }

    pub fn edit_playlist(&self, session: &Session, playlist: &Playlist, title: &str, description: &str) -> Result<Playlist> {
        self.edit_playlist_with_discovery(session, playlist, title, description, None)
    }

    pub fn edit_playlist_with_discovery(&self, session: &Session, playlist: &Playlist, title: &str, description: &str, hidden: Option<bool>) -> Result<Playlist> {
        self.own_playlist(session, playlist)?;
        let title = Self::clean_title(title)?;
        let description = Self::clean_description(description)?;
        let mut params = vec![
            ("owner_id", playlist.owner_id.to_string()),
            ("playlist_id", playlist.playlist_id.to_string()),
            ("title", title.clone()),
            ("description", description.clone()),
        ];
        if let Some(hidden) = hidden { params.push(("no_discover", if hidden { "1" } else { "0" }.into())); }
        self.call_once(session, "audio.editPlaylist", &params)?;
        Ok(Playlist { title, description: (!description.is_empty()).then_some(description), ..playlist.clone() })
    }

    pub fn delete_playlist(&self, session: &Session, playlist: &Playlist) -> Result<()> {
        self.own_playlist(session, playlist)?;
        self.call_once(
            session,
            "audio.deletePlaylist",
            &[("owner_id", playlist.owner_id.to_string()), ("playlist_id", playlist.playlist_id.to_string())],
        )?;
        Ok(())
    }

    /// Добавляет (`add = true`) или удаляет трек в одном из плейлистов пользователя.
    pub fn change_playlist(&self, session: &Session, playlist: &Playlist, track: &Track, add: bool) -> Result<()> {
        self.own_playlist(session, playlist)?;
        if track.track_id <= 0 || track.owner_id == 0 {
            return Err(Error::InvalidRequest);
        }
        let method = if add { "audio.addToPlaylist" } else { "audio.removeFromPlaylist" };
        self.call_once(
            session,
            method,
            &[
                ("owner_id", playlist.owner_id.to_string()),
                ("playlist_id", playlist.playlist_id.to_string()),
                ("audio_ids", format!("{}_{}", track.owner_id, track.track_id)),
            ],
        )?;
        Ok(())
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

    /// Исполнители, соответствующие запросу (`audio.searchArtists`). Ответ — `{count, items}`, но он
    /// также вернулся вложенным, поэтому каждый объект в форме художника собирается там, где он
    /// находится.
    pub fn search_artists(&self, session: &Session, query: &str, offset: u32, count: u32) -> Result<Vec<Artist>> {
        let value = self.call(
            session,
            "audio.searchArtists",
            &[("q", query.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        )?;
        let mut found = Vec::new();
        collect_artists(&value, &mut found);
        Ok(dedupe_by(found, |a| a.id.clone()))
    }

    /// Поиск исполнителя VK приравнивается к запросу `name`, если таковой имеется.
    pub fn find_artist(&self, session: &Session, name: &str) -> Result<Option<Artist>> {
        let candidates = self.search_artists(session, name, 0, 8)?;
        Ok(best_artist_match(candidates, name))
    }

    /// Треки исполнителя: `audio.getAudiosByArtist` и страница исполнителя в каталоге VK, если она
    /// отклонена или пуста.
    pub fn artist_tracks(&self, session: &Session, artist_id: &str, offset: u32, count: u32) -> Result<Page<Track>> {
        let user = session.user_id;
        match self.call(
            session,
            "audio.getAudiosByArtist",
            &[("artist_id", artist_id.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        ) {
            Ok(value) => {
                let page = tracks_page(&value, offset, user);
                if !page.items.is_empty() {
                    return Ok(page);
                }
            }
            Err(error) if is_fatal(&error) => return Err(error),
            Err(error) => log::debug!("audio.getAudiosByArtist: {}, trying the catalogue", error.kind()),
        }
        let value = self.artist_catalog(session, artist_id)?;
        let mut tracks = Vec::new();
        collect_tracks(&value, user, &mut tracks);
        Ok(slice_page(dedupe(tracks), offset, count))
    }

    fn artist_catalog(&self, session: &Session, artist_id: &str) -> Result<Value> {
        self.call(
            session,
            "catalog.getAudioArtist",
            &[("artist_id", artist_id.to_string()), ("need_blocks", "1".into())],
        )
    }

    /// Релизы художника. `audio.getAlbumsByArtist` — список; страница исполнителя каталога заполняет
    /// то, чего не хватает в ее записях (ключи, обложки), и заменяет список, когда каждая запись
    /// представляет собой пустую заглушку. В каталоге также указан художник с фотографией, чего в
    /// списке нет.
    pub fn artist_albums(&self, session: &Session, artist_id: &str, offset: u32, count: u32) -> Result<ArtistAlbums> {
        let mut list: Vec<Album> = Vec::new();
        let mut list_total: Option<u32> = None;
        match self.call(
            session,
            "audio.getAlbumsByArtist",
            &[("artist_id", artist_id.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        ) {
            Ok(value) => {
                list = collect_entries(&value).0.iter().filter_map(model::parse_album).collect();
                list_total = value.get("count").and_then(model::int).map(|n| n.max(0) as u32);
            }
            Err(error) if is_fatal(&error) => return Err(error),
            Err(error) => log::debug!("audio.getAlbumsByArtist: {}, trying the catalogue", error.kind()),
        }
        let (catalog, artist) = match self.artist_catalog(session, artist_id) {
            Ok(value) => {
                let mut albums = Vec::new();
                collect_albums(&value, &mut albums);
                (dedupe_by(albums, Album::id), catalog_artist(&value, artist_id))
            }
            Err(error) if is_fatal(&error) => return Err(error),
            Err(_) => (Vec::new(), None),
        };
        Ok(ArtistAlbums { artist, albums: artist_album_page(list, list_total, catalog, offset, count) })
    }

    pub fn search_albums(&self, session: &Session, query: &str, offset: u32, count: u32) -> Result<Page<Album>> {
        let value = self.call(
            session,
            "audio.searchAlbums",
            &[("q", query.to_string()), ("count", count.to_string()), ("offset", offset.to_string())],
        )?;
        let (entries, raw_count) = collect_entries(&value);
        let items: Vec<Album> = entries.iter().filter_map(model::parse_album).collect();
        let total = value.get("count").and_then(model::int).map(|n| n.max(0) as u32).unwrap_or(offset + raw_count);
        Ok(Page { items, total, next_offset: model::next_offset(offset, raw_count, total) })
    }

    /// Пользовательские версии следующие: `audio.getPlaylists` с `filters=albums`. `filters` — это
    /// объединение категорий, а не цепочка квалификаторов (`followed,albums` будет добавлять каждый
    /// плейлист, сохраненный от кого-то другого), поэтому он называет только `albums`; все, что VK
    /// помечает как рукотворное, не учитывается.
    pub fn followed_albums(&self, session: &Session, offset: u32, count: u32) -> Result<Page<Album>> {
        let mut params = vec![("count", count.to_string()), ("offset", offset.to_string()), ("filters", "albums".to_string())];
        if let Ok(user) = self.user_id(session) {
            params.push(("owner_id", user.to_string()));
        }
        let value = self.call(session, "audio.getPlaylists", &params)?;
        let (entries, raw_count) = collect_entries(&value);
        let items: Vec<Album> = entries
            .iter()
            .filter_map(library_entry)
            .filter(|entry| !has_playlist_marker(entry))
            .filter_map(model::parse_album)
            .collect();
        let total = value.get("count").and_then(model::int).map(|n| n.max(0) as u32).unwrap_or(offset + raw_count);
        let total = total.max(offset + raw_count);
        // Всегда продолжается полная страница: `count` VK в этом списке не всегда описывает тот же
        // набор, что и `items`.
        let next_offset = match raw_count {
            0 => None,
            n if n >= count => Some(offset + n),
            n => model::next_offset(offset, n, total),
        };
        Ok(Page { items, total, next_offset })
    }

    /// `album` с пригодным для использования `access_key`, когда он требуется для выпуска сообщества.
    /// Не изменяется, если он уже есть или им владеет человек.
    pub fn resolved_album(&self, session: &Session, album: &Album) -> Result<Album> {
        if !album.needs_access_key() {
            return Ok(album.clone());
        }
        Ok(self.resolve_album_access_key(session, album, false)?.unwrap_or_else(|| album.clone()))
    }

    /// Одна страница треков альбома и описание альбома как VK.
    ///
    /// `execute.getPlaylist`, затем `audio.get` для сеансов, которые отклоняют первый. Если в доступе
    /// отказано, ключ снова ищется посредством поиска; когда методы плейлиста по-прежнему отказываются
    /// (или отвечают пусто), альбом собирается из принадлежащих ему публичных хитов `audio.search`.
    pub fn album_tracks(&self, session: &Session, album: &Album, offset: u32, count: u32) -> Result<AlbumTracks> {
        let effective = self.resolved_album(session, album)?;
        let mut last_error = None;
        match self.fetch_album_tracks(session, &effective, offset, count) {
            Ok(found) if !found.tracks.items.is_empty() => return Ok(found),
            Ok(_) => {}
            Err(error) if is_fatal(&error) => return Err(error),
            Err(error) => {
                if is_access_denied(&error) {
                    let recovered = self.resolve_album_access_key(session, album, true).ok().flatten();
                    if let Some(recovered) = recovered.filter(|r| r.usable_access_key() != effective.usable_access_key()) {
                        if let Ok(found) = self.fetch_album_tracks(session, &recovered, offset, count) {
                            if !found.tracks.items.is_empty() {
                                return Ok(found);
                            }
                        }
                    }
                }
                last_error = Some(error);
            }
        }
        match self.album_tracks_via_search(session, album, offset, count) {
            Ok(tracks) if !tracks.items.is_empty() => return Ok(AlbumTracks { album: effective, tracks }),
            Err(error) if is_fatal(&error) => return Err(error),
            _ => {}
        }
        Err(match last_error {
            Some(error) if is_access_denied(&error) => {
                let code = match error {
                    Error::Vk { code, .. } if code > 0 => code,
                    _ => 15,
                };
                Error::Vk { code, message: ALBUM_DENIED.into() }
            }
            Some(error) => error,
            None => Error::Format("album: no tracks".into()),
        })
    }

    fn fetch_album_tracks(&self, session: &Session, album: &Album, offset: u32, count: u32) -> Result<AlbumTracks> {
        let user = session.user_id;
        let mut params = vec![
            ("owner_id", album.owner_id.to_string()),
            ("id", album.album_id.to_string()),
            ("audio_offset", offset.to_string()),
            ("audio_count", count.to_string()),
            ("need_playlist", "1".to_string()),
            ("need_owner", "0".to_string()),
        ];
        if let Some(key) = album.usable_access_key() {
            params.push(("access_key", key.to_string()));
        }
        match self.call(session, "execute.getPlaylist", &params) {
            Ok(value) => {
                // VK описывает список воспроизведения рядом с его аудиозаписями; то, что там написано
                // (обложка, год, состояние, размер), превосходит то, что было на окне.
                let described = value
                    .get("playlist")
                    .and_then(model::parse_album)
                    .filter(|p| p.id() == album.id())
                    .map(|p| album.merging_access_metadata(&Album { access_key: None, ..p }))
                    .unwrap_or_else(|| album.clone());
                let audios = match value.get("audios") {
                    Some(Value::Array(list)) => Some(list.clone()),
                    Some(Value::Null) => Some(Vec::new()),
                    _ => None,
                };
                let mut tracks = Vec::new();
                match &audios {
                    Some(list) => list.iter().for_each(|v| collect_tracks(v, user, &mut tracks)),
                    None => collect_tracks(&value, user, &mut tracks),
                }
                let tracks = dedupe(tracks);
                if !tracks.is_empty() {
                    let raw = audios.as_ref().map(Vec::len).unwrap_or(tracks.len()) as u32;
                    let consumed = offset + raw;
                    let known_more = described.count > consumed;
                    let unknown_more = described.count == 0 && raw >= count;
                    let total = if described.count > 0 { described.count } else { offset + tracks.len() as u32 };
                    let next_offset = (known_more || unknown_more).then_some(consumed);
                    return Ok(AlbumTracks { album: described, tracks: Page { items: tracks, total, next_offset } });
                }
            }
            Err(error) if is_fatal(&error) || is_access_denied(&error) => return Err(error),
            // Более старые сессии могут отклонять execute.getPlaylist; простой запрос альбома ниже все
            // еще работает для них.
            Err(error) => log::debug!("execute.getPlaylist: {}, trying audio.get", error.kind()),
        }
        let mut params = vec![
            ("owner_id", album.owner_id.to_string()),
            ("album_id", album.album_id.to_string()),
            ("count", count.to_string()),
            ("offset", offset.to_string()),
        ];
        if let Some(key) = album.usable_access_key() {
            params.push(("access_key", key.to_string()));
        }
        let value = self.call(session, "audio.get", &params)?;
        Ok(AlbumTracks { album: album.clone(), tracks: tracks_page(&value, offset, user) })
    }

    /// Находит `access_key` для `album`: `audio.getPlaylistById`, когда ключ или владелец человека
    /// делают это возможным, в противном случае лучшее совпадение среди результатов
    /// `audio.searchAlbums` для его названия и исполнителя.
    fn resolve_album_access_key(&self, session: &Session, album: &Album, force_search: bool) -> Result<Option<Album>> {
        if !force_search && album.usable_access_key().is_some() {
            return Ok(Some(album.clone()));
        }
        if album.usable_access_key().is_some() || album.owner_id > 0 {
            if let Ok(detailed) = self.playlist_by_id(session, album) {
                return Ok(Some(album.merging_access_metadata(&detailed)));
            }
        }
        let mut candidates: Vec<Album> = Vec::new();
        for query in album_search_queries(album) {
            let page = self.search_albums(session, &query, 0, 30)?;
            for item in page.items {
                if !candidates.iter().any(|c| c.id() == item.id()) {
                    candidates.push(item);
                }
            }
        }
        Ok(preferred_album_match(&candidates, album).map(|found| album.merging_access_metadata(found)))
    }

    pub fn playlist_by_id(&self, session: &Session, album: &Album) -> Result<Album> {
        let mut params = vec![("owner_id", album.owner_id.to_string()), ("playlist_id", album.album_id.to_string())];
        if let Some(key) = album.usable_access_key() {
            params.push(("access_key", key.to_string()));
        }
        let value = self.call(session, "audio.getPlaylistById", &params)?;
        model::parse_album(&value).ok_or_else(|| Error::Format("audio.getPlaylistById".into()))
    }

    /// Когда методы списка воспроизведения запрещают доступ, пригодный для использования список треков
    /// перестраивается из общедоступных результатов `audio.search`, принадлежащих альбому.
    fn album_tracks_via_search(&self, session: &Session, album: &Album, offset: u32, count: u32) -> Result<Page<Track>> {
        let mut collected: Vec<Track> = Vec::new();
        let append = |collected: &mut Vec<Track>, tracks: Vec<Track>, soft_artist_match: bool| {
            for track in tracks {
                let matches = track_belongs(&track, album) || (soft_artist_match && artist_matches(&track, album));
                if matches && !collected.iter().any(|t| t.id() == track.id()) {
                    collected.push(track);
                }
            }
        };
        let wanted = (count + offset).max(40) as usize;
        for query in album_search_queries(album) {
            let page = self.search(session, &query, 0, 100)?;
            // Поиск часто не учитывает `album_title`; для запроса, в котором уже назван альбом,
            // достаточно сопоставления исполнителя.
            let names_title = model::is_usable_title(&album.title) && fold(&query).contains(&fold(&album.title));
            append(&mut collected, page.items, names_title);
            if collected.len() >= wanted {
                break;
            }
        }
        // Ключ, который нашелся при поиске, все-таки может открыть плейлист.
        if collected.len() < 8 && album.usable_access_key().is_some() {
            if let Ok(found) = self.fetch_album_tracks(session, album, 0, (count + offset).max(100)) {
                append(&mut collected, found.tracks.items, false);
            }
        }
        // Собственные треки исполнителя являются вторым общедоступным источником, когда поиск названий
        // скуден.
        if collected.len() < 8 {
            if let Some(name) = album.artist_names().next() {
                let artist = self.search_artists(session, name, 0, 5).ok().and_then(|list| list.into_iter().next());
                if let Some(page) = artist.and_then(|a| self.artist_tracks(session, &a.id, 0, 100).ok()) {
                    append(&mut collected, page.items, false);
                }
            }
        }
        // В крайнем случае: хиты, соответствующие исполнителю, только по заголовку, а в противном
        // случае - лучшие хиты, поэтому известное название не открывается пустым.
        if collected.is_empty() && model::is_usable_title(&album.title) {
            let page = self.search(session, &album.title, 0, 100)?;
            append(&mut collected, page.items.clone(), true);
            if collected.is_empty() {
                collected = dedupe(page.items).into_iter().take(count as usize).collect();
            }
        }
        if collected.is_empty() {
            return Err(Error::Format("album: nothing found".into()));
        }
        Ok(slice_page(collected, offset, count))
    }

    /// Сохраняет релиз в альбомы пользователя или удаляет его. VK не ​​имеет `audio.unfollowPlaylist`:
    /// удаление следующего альбома — это тот же вызов `audio.deletePlaylist`, что и удаление
    /// собственного плейлиста.
    pub fn follow_album(&self, session: &Session, album: &Album, follow: bool) -> Result<()> {
        let mut params = vec![("owner_id", album.owner_id.to_string()), ("playlist_id", album.album_id.to_string())];
        if let Some(hash) = album.follow_hash.as_deref().filter(|h| !h.is_empty()) {
            params.push(("hash", hash.to_string()));
        }
        if let Some(key) = album.usable_access_key() {
            params.push(("access_key", key.to_string()));
        }
        let method = if follow { "audio.followPlaylist" } else { "audio.deletePlaylist" };
        self.call_once(session, method, &params)?;
        Ok(())
    }

    /// Свежий стрим URL на один трек (`audio.getById`). Делается прямо перед игрой, когда срок действия
    /// имеющегося у нас URL может истек.
    pub fn refreshed_track(&self, session: &Session, track: &Track) -> Result<Track> {
        self.refreshed_track_with(session, track, 3)
    }

    /// Как и [`refreshed_track`](Self::refreshed_track) с одним запросом: цикл восстановления потока
    /// распределяет свои собственные попытки, и повторная попытка здесь также умножит один сбой на
    /// множество запросов (приложение iOS делает то же самое для своей политики `playbackRecovery`).
    pub fn refreshed_track_once(&self, session: &Session, track: &Track) -> Result<Track> {
        self.refreshed_track_with(session, track, 1)
    }

    fn refreshed_track_with(&self, session: &Session, track: &Track, attempts: u32) -> Result<Track> {
        let value = self.call_limited(session, "audio.getById", &[("audios", track.audio_id_with_key())], attempts)?;
        let user = self.user_id(session).ok();
        let fresh = value
            .as_array()
            .and_then(|list| list.first())
            .and_then(|o| model::parse_track(o, user))
            .ok_or_else(|| Error::Format("audio.getById: empty".into()))?;
        if fresh.stream_url.is_none() {
            return Err(Error::Unavailable);
        }
        Ok(fresh)
    }
}

/// Релизы художника и, если они указаны в каталоге, художник.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ArtistAlbums {
    pub artist: Option<Artist>,
    pub albums: Page<Album>,
}

/// Страница с треками альбома и описанием альбома как VK (с ключом, открывающим его).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AlbumTracks {
    pub album: Album,
    pub tracks: Page<Track>,
}

/// Что показывает экран альбома вместо собственного текста отказа VK.
const ALBUM_DENIED: &str = "Could not load this album's tracks. Try again or find it in search.";

/// Сбои, при которых никакой резервный вариант не может помочь: мертвый сеанс или отсутствие сети.
fn is_fatal(error: &Error) -> bool {
    matches!(error, Error::Unauthorized | Error::Offline | Error::Timeout | Error::Transport(_) | Error::Cancelled)
}

/// VK отказывается от аудио из чужого плейлиста.
fn is_access_denied(error: &Error) -> bool {
    let Error::Vk { code, message } = error else { return false };
    if matches!(code, 15 | 201 | 203 | 204) {
        return true;
    }
    let lower = message.to_lowercase();
    ["access denied", "access to users audio", "permission to perform this action"].iter().any(|m| lower.contains(m))
}

fn slice_page<T>(all: Vec<T>, offset: u32, count: u32) -> Page<T> {
    let total = all.len() as u32;
    let items: Vec<T> = all.into_iter().skip(offset as usize).take(count as usize).collect();
    let consumed = offset + items.len() as u32;
    Page { items, total, next_offset: (consumed < total).then_some(consumed) }
}

fn dedupe_by<T, K: Eq + std::hash::Hash>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|item| seen.insert(key(item))).collect()
}

/// Нижний регистр, обрезанный, ё как е: для сравнения имен VK пишется свободно.
fn fold(text: &str) -> String {
    text.trim().to_lowercase().replace('ё', "е")
}

fn names_match(a: &str, b: &str) -> bool {
    a == b || a.contains(b) || b.contains(a)
}

/// Исполнитель из поиска для использования имени: точное совпадение, иначе одна из частей коллаборации,
/// иначе близкий префикс (минимум три буквы и большая часть имени).
fn best_artist_match(candidates: Vec<Artist>, name: &str) -> Option<Artist> {
    let target = fold(name);
    let score = |candidate: &str| -> u32 {
        let value = fold(candidate);
        if target.is_empty() || value.is_empty() {
            return 0;
        }
        if value == target {
            return 100;
        }
        if collaboration_parts(&value).contains(&target) {
            return 80;
        }
        if value.starts_with(&target) || target.starts_with(&value) {
            let (a, b) = (value.chars().count(), target.chars().count());
            let (shorter, longer) = (a.min(b), a.max(b));
            if shorter >= 3 && shorter as f64 / longer as f64 >= 0.72 {
                return 55;
            }
        }
        0
    };
    let mut best: Option<(u32, Artist)> = None;
    for candidate in candidates {
        let s = score(&candidate.name);
        if s > 0 && best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, candidate));
        }
    }
    best.map(|(_, artist)| artist)
}

fn collaboration_parts(value: &str) -> Vec<String> {
    let mut parts = vec![value.to_string()];
    for separator in [",", ";", " feat. ", " feat ", " featuring ", " ft. ", " ft "] {
        parts = parts.iter().flat_map(|p| p.split(separator).map(str::to_string).collect::<Vec<_>>()).collect();
    }
    parts.iter().map(|p| fold(p)).filter(|p| !p.is_empty()).collect()
}

/// Страница релизов, на которой отображается экран исполнителя. К списку из `audio.getAlbumsByArtist`
/// уже применено смещение, поэтому он только пополняется из каталога и никогда больше не расширяется и
/// не разрезается. Если список пуст или содержит только заглушки без обложки и размера, каталог
/// заменяет его.
fn artist_album_page(list: Vec<Album>, list_total: Option<u32>, catalog: Vec<Album>, offset: u32, count: u32) -> Page<Album> {
    let incomplete = |a: &Album| a.count == 0 && a.artwork_url.is_none();
    let prefer_catalog = !catalog.is_empty() && list.iter().all(incomplete);
    if !list.is_empty() && !prefer_catalog {
        let received = list.len() as u32;
        let items: Vec<Album> = list
            .into_iter()
            .map(|album| match catalog.iter().find(|c| c.id() == album.id()) {
                Some(found) => album.merging_access_metadata(found),
                None => album,
            })
            .collect();
        let total = list_total.unwrap_or(offset + received + u32::from(received >= count));
        return Page { items, total, next_offset: model::next_offset(offset, received, total) };
    }
    slice_page(catalog, offset, count)
}

/// Большинство разделов каталога сведены под одну полку.
const HYDRATION_LIMIT: usize = 8;

/// Они перечислены в разделе каталога под названием `catalog.getAudio`.
struct CatalogSection {
    id: String,
    title: String,
    url: Option<String>,
}

impl CatalogSection {
    /// Заголовок и URL-адрес, строчные буквы: с чем сопоставляются маркеры.
    fn blob(&self) -> String {
        format!("{} {}", self.title, self.url.as_deref().unwrap_or("")).to_lowercase()
    }

    fn looks_like_mix(&self) -> bool {
        const MARKERS: [&str; 33] = [
            "mix", "микс", "stream", "подбор", "radio", "вкус", "для вас", "for you", "discover", "поток", "mood", "настроен", "активност",
            "activity", "жанр", "genre", "section=mix", "listen together", "друг", "вайб", "vibe", "спокойн", "грустн", "радост", "энерг", "chart",
            "чарт", "открыт", "новинк", "плейлист дня", "хит", "kids", "дет",
        ];
        let blob = self.blob();
        MARKERS.iter().any(|marker| blob.contains(marker))
    }

    fn looks_like_releases(&self) -> bool {
        const MARKERS: [&str; 9] = ["release", "релиз", "нов", "выход", "премьер", "album", "альбом", "new music", "свеж"];
        let blob = self.blob();
        MARKERS.iter().any(|marker| blob.contains(marker))
    }
}

/// Одна запись для каждого идентификатора раздела по порядку; идентификаторы, которые не удалось
/// отправить, удаляются.
fn unique_sections(sections: &mut Vec<CatalogSection>) {
    let mut seen = std::collections::HashSet::new();
    sections.retain(|s| s.id.len() <= 512 && !s.id.chars().any(char::is_control) && seen.insert(s.id.clone()));
}

/// Что дал `catalog.getAudio` и его гидратированные разделы.
struct CatalogSnapshot {
    mixes: Vec<MusicMix>,
    releases: Vec<Album>,
    /// Структура корневого ответа, для журнала, когда полка оказывается пустой.
    shape: String,
}

const COMMON_MIX_ID: &str = "common";

/// Персональный поток, названный так, как его называет приложение. Собственная формулировка VK
/// варьируется ("Мой микс", "VK Микс"), поэтому она всегда заменяется этой.
fn common_mix() -> MusicMix {
    MusicMix {
        id: COMMON_MIX_ID.into(),
        title: "Мой микс".into(),
        subtitle: "Ваш личный микс по истории прослушивания".into(),
        artwork_url: None,
        section_title: None,
        options: None,
    }
}

/// Миксы уникальные по id и настройке; повтор лишь восполняет то, чего не хватало первому.
fn dedupe_mixes(mixes: Vec<MusicMix>) -> Vec<MusicMix> {
    let mut out: Vec<MusicMix> = Vec::new();
    for mix in mixes {
        match out.iter_mut().find(|known| known.key() == mix.key()) {
            Some(known) => {
                if known.title.is_empty() {
                    known.title = mix.title;
                }
                if known.subtitle.is_empty() {
                    known.subtitle = mix.subtitle;
                }
                if known.artwork_url.is_none() {
                    known.artwork_url = mix.artwork_url;
                }
                if known.section_title.is_none() {
                    known.section_title = mix.section_title;
                }
            }
            None => out.push(mix),
        }
    }
    out
}

/// Метка общедоступного каталога, безопасная для журнала: без управляющих символов, без длинных цифр
/// (URL-адрес может содержать идентификатор пользователя), обрезанная.
fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut digits = 0;
    for c in text.chars().filter(|c| !c.is_control()) {
        if c.is_ascii_digit() {
            digits += 1;
            if digits > 3 {
                continue;
            }
        } else {
            digits = 0;
        }
        out.push(c);
        if out.chars().count() >= 48 {
            out.push('…');
            break;
        }
    }
    out
}

/// Структура ответа каталога и ничего больше: имена ключей, счетчики, метки общедоступных разделов и
/// типы данных блоков. Никаких идентификаторов, токенов или необработанного текста.
fn catalog_shape(root: &Value, mixes: usize, albums: usize, sections: &[CatalogSection]) -> String {
    fn walk(value: &Value, depth: u8, blocks: &mut usize, kinds: &mut std::collections::BTreeSet<String>, mix_keys: &mut std::collections::BTreeSet<String>) {
        if depth > 24 {
            return;
        }
        match value {
            Value::Array(list) => list.iter().for_each(|v| walk(v, depth + 1, blocks, kinds, mix_keys)),
            Value::Object(object) => {
                for (key, child) in object {
                    if key == "blocks" {
                        *blocks += child.as_array().map_or(0, Vec::len);
                    }
                    if key.to_lowercase().contains("mix") {
                        mix_keys.insert(shown(key));
                    }
                    if matches!(key.as_str(), "data_type" | "type") {
                        if let Some(kind) = child.as_str().filter(|k| k.len() <= 48) {
                            kinds.insert(shown(kind));
                        }
                    }
                    walk(child, depth + 1, blocks, kinds, mix_keys);
                }
            }
            _ => {}
        }
    }
    let (mut blocks, mut kinds, mut mix_keys) = (0, std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    walk(root, 0, &mut blocks, &mut kinds, &mut mix_keys);
    let keys = match root {
        Value::Object(object) => object.keys().take(24).map(|k| shown(k)).collect::<Vec<_>>().join(","),
        Value::Array(list) => format!("<array of {}>", list.len()),
        _ => "<scalar>".into(),
    };
    let listed: Vec<String> = sections
        .iter()
        .take(6)
        .map(|s| format!("\"{}\" {}", shown(&s.title), shown(s.url.as_deref().unwrap_or("-"))))
        .collect();
    format!(
        "root keys=[{keys}] blocks={blocks} sections={} (mix-like {}, release-like {}) first sections=[{}] mixes_in_root={mixes} albums_in_root={albums} types=[{}] mix-ish keys=[{}]",
        sections.len(),
        sections.iter().filter(|s| s.looks_like_mix()).count(),
        sections.iter().filter(|s| s.looks_like_releases()).count(),
        listed.join("; "),
        kinds.into_iter().take(24).collect::<Vec<_>>().join(","),
        mix_keys.into_iter().take(12).collect::<Vec<_>>().join(","),
    )
}

/// Реальный идентификатор раздела — это не просто число: `-2000123456` — это идентификатор объекта,
/// отрицающий владельца, поэтому сначала игнорируется один ведущий минус.
fn is_section_id(id: &str) -> bool {
    let id = id.trim();
    let digits = id.strip_prefix('-').unwrap_or(id);
    !digits.is_empty() && !digits.chars().all(|c| c.is_numeric())
}

fn catalog_section(value: &Value) -> Option<CatalogSection> {
    let object = value.as_object()?;
    let id = scalar_text(object.get("id")).or_else(|| scalar_text(object.get("section_id")))?;
    if id.is_empty() {
        return None;
    }
    let kind = scalar_text(object.get("type")).or_else(|| scalar_text(object.get("data_type"))).unwrap_or_default();
    let url = scalar_text(object.get("url"));
    let looks_like_section = url.as_deref().is_some_and(|u| u.contains("audios"))
        || kind.to_lowercase().contains("section")
        || object.contains_key("blocks")
        || object.contains_key("next_from");
    // Свободное предложение: заголовок и URL-адрес описывают плейлист или альбом, а также раздел,
    // поэтому сам идентификатор должен выглядеть как идентификатор раздела.
    let titled_link = object.contains_key("title") && url.is_some() && is_section_id(&id);
    if !(looks_like_section || titled_link) {
        return None;
    }
    let title = scalar_text(object.get("title")).or_else(|| scalar_text(object.get("name"))).unwrap_or_default();
    Some(CatalogSection { id, title, url })
}

/// Разделы, перечисленные в любом месте ответа каталога (`catalog.sections`, `sections` или объекты в
/// форме разделов).
fn collect_catalog_sections(value: &Value, out: &mut Vec<CatalogSection>) {
    fn visit(value: &Value, out: &mut Vec<CatalogSection>, depth: u8) {
        if depth > 64 {
            return;
        }
        match value {
            Value::Array(list) => list.iter().for_each(|v| visit(v, out, depth + 1)),
            Value::Object(object) => {
                let listed = object.get("catalog").and_then(|c| c.get("sections")).into_iter().chain(object.get("sections"));
                for list in listed.filter_map(Value::as_array) {
                    out.extend(list.iter().filter_map(catalog_section));
                }
                out.extend(catalog_section(value));
                object.values().for_each(|v| visit(v, out, depth + 1));
            }
            _ => {}
        }
    }
    visit(value, out, 0);
}

/// Объекты в форме выпуска в любом месте ответа каталога. Строки аудио также содержат идентификатор,
/// владельца, заголовок и `main_artists`, поэтому `duration` исключает объект.
fn collect_albums(value: &Value, out: &mut Vec<Album>) {
    match value {
        Value::Array(list) => list.iter().for_each(|v| collect_albums(v, out)),
        Value::Object(object) => {
            let release = ["owner_id", "id", "title"].iter().all(|k| object.contains_key(*k))
                && !object.contains_key("duration")
                && (object.contains_key("main_artists") || object.contains_key("year"));
            if release {
                if let Some(album) = model::parse_album(value) {
                    out.push(album);
                    return;
                }
            }
            object.values().for_each(|v| collect_albums(v, out));
        }
        _ => {}
    }
}

fn collect_artists(value: &Value, out: &mut Vec<Artist>) {
    match value {
        Value::Array(list) => list.iter().for_each(|v| collect_artists(v, out)),
        Value::Object(object) => {
            if let Some(artist) = model::parse_artist(value) {
                out.push(artist);
            }
            object.values().for_each(|v| collect_artists(v, out));
        }
        _ => {}
    }
}

/// Артист, о котором идет страница исполнителя в каталоге: его блок `artists`, сначала запрошенный
/// идентификатор.
fn catalog_artist(value: &Value, artist_id: &str) -> Option<Artist> {
    let mut found = Vec::new();
    collect_artists(value.get("artists").unwrap_or(&Value::Null), &mut found);
    let index = found.iter().position(|a| a.id == artist_id)?;
    Some(found.swap_remove(index))
}

/// Объект списка воспроизведения, которым является запись списка, или его оболочка (`playlist`,
/// `album`, `audio_playlist`). Объекты с `duration` являются звуковыми строками.
fn library_entry(value: &Value) -> Option<&Value> {
    let is_entry = |v: &Value| v.get("id").is_some() && v.get("owner_id").is_some() && v.get("duration").is_none();
    for key in ["playlist", "album", "audio_playlist"] {
        if let Some(nested) = value.get(key).filter(|v| v.is_object() && is_entry(v)) {
            return Some(nested);
        }
    }
    is_entry(value).then_some(value)
}

/// Пометки несет только составленный человеком список: `original` (сохраненный от кого-то другого) или
/// `album_type` из `playlist`/`collection`.
fn has_playlist_marker(entry: &Value) -> bool {
    if entry.get("original").is_some_and(Value::is_object) {
        return true;
    }
    let kind = entry.get("album_type").and_then(Value::as_str).unwrap_or("").to_lowercase();
    matches!(kind.as_str(), "playlist" | "collection")
}

/// Последующий релиз в простом списке `audio.getPlaylists`, вне всякого сомнения: VK напечатал его как
/// релиз, назвал исполнителей и датировал его (или пометил как `type: 1`), без маркера плейлиста. VK
/// также добавляет метаданные, подобные выпуску, в обычные плейлисты, поэтому все, что меньше, остается
/// плейлистом: лишняя карта — это недостаток, а исчезающий плейлист — ошибка.
fn looks_like_followed_album(entry: &&Value) -> bool {
    let kind = entry.get("album_type").and_then(Value::as_str).unwrap_or("").to_lowercase();
    let release = matches!(kind.as_str(), "album" | "single" | "ep" | "compilation" | "main_only");
    let has_artists = entry.get("main_artists").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    if has_playlist_marker(entry) || !release || !has_artists {
        return false;
    }
    let dated = entry.get("year").and_then(model::int).is_some() || entry.get("original_year").and_then(model::int).is_some();
    let typed = entry.get("type").and_then(model::int) == Some(1);
    1 + u32::from(dated) + u32::from(typed) >= 2
}

/// Поисковые запросы, по которым можно найти альбом: "title artist", "artist title", название,
/// исполнитель.
fn album_search_queries(album: &Album) -> Vec<String> {
    let title = album.title.trim();
    let artist = album.artist_names().next().map(str::trim).unwrap_or("");
    let mut queries = Vec::new();
    if model::is_usable_title(title) && !artist.is_empty() {
        queries.push(format!("{title} {artist}"));
        queries.push(format!("{artist} {title}"));
    }
    if model::is_usable_title(title) {
        queries.push(title.to_string());
    }
    if !artist.is_empty() {
        queries.push(artist.to_string());
    }
    dedupe_by(queries, |q| q.to_lowercase())
}

/// Результатом поиска является `album`: тот же идентификатор (с ключом, если он есть), в противном
/// случае то же название одного из его исполнителей, в противном случае то же название, сначала
/// результаты с ключом.
fn preferred_album_match<'a>(candidates: &'a [Album], album: &Album) -> Option<&'a Album> {
    let keyed = |a: &&Album| a.usable_access_key().is_some();
    let same: Vec<&Album> = candidates.iter().filter(|c| c.id() == album.id()).collect();
    if let Some(found) = same.iter().copied().find(keyed).or(same.first().copied()) {
        return Some(found);
    }
    let title = fold(&album.title);
    if title.is_empty() {
        return candidates.iter().find(keyed);
    }
    let hints: Vec<String> = album.artist_names().map(fold).filter(|h| !h.is_empty()).collect();
    let titled: Vec<&Album> = candidates.iter().filter(|c| names_match(&fold(&c.title), &title)).collect();
    if !hints.is_empty() {
        let by_artist: Vec<&Album> = titled
            .iter()
            .copied()
            .filter(|c| c.artist_names().any(|name| hints.iter().any(|h| names_match(&fold(name), h))))
            .collect();
        if let Some(found) = by_artist.iter().copied().find(keyed).or(by_artist.first().copied()) {
            return Some(found);
        }
    }
    titled.iter().copied().find(keyed).or(titled.first().copied()).or_else(|| candidates.iter().find(keyed))
}

/// Поисковый результат попадает на `album`, когда VK говорит об этом, или когда название его альбома
/// совпадает, а также совпадает с исполнителем (или в названии альбома нет названия).
fn track_belongs(track: &Track, album: &Album) -> bool {
    if track.album.as_ref().is_some_and(|r| r.album_id == album.album_id && r.owner_id == album.owner_id) {
        return true;
    }
    let title = fold(&album.title);
    let track_album = fold(track.album_title.as_deref().unwrap_or(""));
    if title.is_empty() || track_album.is_empty() || !names_match(&track_album, &title) {
        return false;
    }
    artist_matches(track, album) || album.artists.is_empty()
}

fn artist_matches(track: &Track, album: &Album) -> bool {
    let hints: Vec<String> = album.artist_names().map(fold).filter(|h| !h.is_empty()).collect();
    if hints.is_empty() {
        return true;
    }
    let artist = fold(&track.artist);
    hints.iter().any(|h| artist.contains(h.as_str()) || h.contains(artist.as_str()))
}

fn sleep(seconds: f64) {
    std::thread::sleep(Duration::from_secs_f64(seconds));
}

fn dedupe(tracks: Vec<Track>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    tracks.into_iter().filter(|t| seen.insert(t.id())).collect()
}

fn collect_tracks(value: &Value, user: Option<i64>, out: &mut Vec<Track>) {
    match value {
        Value::Array(list) => list.iter().for_each(|v| collect_tracks(v, user, out)),
        Value::Object(object) => {
            let track_shaped = ["owner_id", "artist", "title"].iter().all(|k| object.contains_key(*k));
            if track_shaped {
                if let Some(track) = model::parse_track(value, user) {
                    out.push(track);
                    return;
                }
            }
            object.values().for_each(|v| collect_tracks(v, user, out));
        }
        _ => {}
    }
}

fn scalar_text(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

/// Каждая плитка микширования потоков в любом месте каталога отвечает (iOS `collectMixes`): объект с
/// `mix_id` или типизированный `…stream_mix…`. Плитка без названия по-прежнему засчитывается. `section`
/// — официальная полка, на которой он был найден.
fn collect_mixes(value: &Value, section: Option<&str>, out: &mut Vec<MusicMix>) {
    fn visit(value: &Value, section: Option<&str>, out: &mut Vec<MusicMix>, depth: u8) {
        if depth > 64 {
            return;
        }
        match value {
            Value::Array(items) => items.iter().for_each(|item| visit(item, section, out, depth + 1)),
            Value::Object(object) => {
                let kind = scalar_text(object.get("data_type")).or_else(|| scalar_text(object.get("type"))).unwrap_or_default();
                let mix_id = scalar_text(object.get("mix_id"));
                let is_mix = mix_id.is_some() || kind.to_lowercase().contains("stream_mix");
                if is_mix {
                    if let Some(id) = mix_id.or_else(|| scalar_text(object.get("id"))) {
                        let options = object.get("mix_options").and_then(mix_options).or_else(|| object.get("options").and_then(mix_options));
                        let personal = id == COMMON_MIX_ID && options.is_none();
                        let title = if personal {
                            common_mix().title
                        } else {
                            scalar_text(object.get("title")).or_else(|| scalar_text(object.get("name"))).unwrap_or_else(|| "Микс".into())
                        };
                        out.push(MusicMix {
                            id,
                            title,
                            subtitle: scalar_text(object.get("subtitle")).or_else(|| scalar_text(object.get("description"))).or_else(|| scalar_text(object.get("caption"))).unwrap_or_default(),
                            artwork_url: mix_cover(object),
                            section_title: section.filter(|s| !s.is_empty()).map(str::to_string),
                            options,
                        });
                    }
                }
                object.values().for_each(|item| visit(item, section, out, depth + 1));
            }
            _ => {}
        }
    }
    visit(value, section, out, 0);
}

/// `mix_options`/`options` настраиваемого тайла: непустая строка или компактно перекодированный объект.
fn mix_options(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) if !raw.is_empty() && raw.len() <= 8_192 => Some(raw.clone()),
        Value::Object(map) if !map.is_empty() && value.to_string().len() <= 8_192 => Some(value.to_string()),
        _ => None,
    }
}

/// Изображение внутри объекта, похожего на крышку: сначала обычные размеры, затем все вложенное (iOS
/// `firstRemoteURL`).
fn first_remote_url(object: &serde_json::Map<String, Value>, depth: u8) -> Option<String> {
    for key in ["photo_1200", "photo_600", "photo_300", "photo_270", "cover_url", "url"] {
        if let Some(url) = object.get(key).and_then(Value::as_str).and_then(crate::unmask::secure_remote_url) {
            return Some(url);
        }
    }
    if depth > 6 {
        return None;
    }
    for value in object.values() {
        match value {
            Value::Object(inner) => {
                if let Some(url) = first_remote_url(inner, depth + 1) {
                    return Some(url);
                }
            }
            Value::Array(list) => {
                for item in list {
                    if let Some(url) = item.as_object().and_then(|inner| first_remote_url(inner, depth + 1)) {
                        return Some(url);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Собственная обложка микс-тайла (iOS `mixCoverURL`): никогда не является аватаром вложенного
/// владельца. Также читается обложка в виде списка картинок.
fn mix_cover(object: &serde_json::Map<String, Value>) -> Option<String> {
    const KEYS: [&str; 11] = ["photo_1200", "photo_600", "photo_300", "photo_270", "cover_url", "thumb_url", "image", "cover", "icon", "background_image_url", "background_url"];
    let from_list = |list: &Vec<Value>| {
        list.iter().find_map(|item| match item {
            Value::Object(inner) => first_remote_url(inner, 0),
            Value::String(raw) => crate::unmask::secure_remote_url(raw),
            _ => None,
        })
    };
    for key in KEYS {
        match object.get(key) {
            Some(Value::String(raw)) => {
                if let Some(url) = crate::unmask::secure_remote_url(raw) {
                    return Some(url);
                }
            }
            Some(Value::Object(inner)) => {
                if let Some(url) = first_remote_url(inner, 0) {
                    return Some(url);
                }
            }
            Some(Value::Array(list)) => {
                if let Some(url) = from_list(list) {
                    return Some(url);
                }
            }
            _ => {}
        }
    }
    ["thumbs", "images", "photos"].iter().find_map(|key| object.get(*key).and_then(Value::as_array)).and_then(from_list)
}

fn tracks_page(value: &Value, offset: u32, user: Option<i64>) -> Page<Track> {
    let (entries, raw_count) = collect_entries(value);
    let items: Vec<Track> = entries.iter().filter_map(|e| model::parse_track(e, user)).collect();
    let total = value.get("count").and_then(model::int).map(|n| n.max(0) as u32).unwrap_or(offset + raw_count);
    Page { items, total, next_offset: model::next_offset(offset, raw_count, total) }
}

/// Записи ответа списка и количество необработанных записей, которые он содержал. Ответом будет
/// `{count, items:[…]}` или, для некоторых методов, пустой массив.
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
    use serde_json::json;

    #[test]
    fn hostile_retry_after_values_never_escape_the_bounds() {
        for hostile in ["nan", "NaN", "inf", "-inf", "-5", "1e999", "", "soon", "1.5", "18446744073709551616"] {
            let d = retry_delay(0, Some(hostile), 1.0);
            assert!(d.is_finite() && (0.2..=8.0).contains(&d), "{hostile} -> {d}");
            assert!((d - 0.55).abs() < 1e-9, "{hostile} falls back to the schedule, got {d}");
        }
        assert_eq!(retry_delay(0, Some("3"), 1.0), 3.0);
        assert_eq!(retry_delay(0, Some(" 2 "), 1.0), 2.0);
        assert_eq!(retry_delay(0, Some("120"), 1.0), 120.0);
        assert_eq!(retry_delay(0, Some("0"), 1.0), 0.2);
        assert_eq!(retry_delay(40, None, 1.0), 4.0);
        assert!(retry_delay(0, None, f64::NAN).is_finite());
        assert!(retry_delay(0, None, f64::INFINITY).is_finite());
    }

    #[test]
    fn api_body_is_bounded_in_size_and_total_read_time() {
        assert_eq!(read_api_body(std::io::Cursor::new(b"abc"), 3, Duration::from_secs(1)).unwrap(), b"abc");
        assert!(matches!(read_api_body(std::io::Cursor::new(b"abcd"), 3, Duration::from_secs(1)), Err(Error::Format(_))));
        struct Drip;
        impl Read for Drip {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                std::thread::sleep(Duration::from_millis(3)); target[0] = b' '; Ok(1)
            }
        }
        assert_eq!(read_api_body(Drip, 1024, Duration::from_millis(1)), Err(Error::Timeout));
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

    fn album(json: serde_json::Value) -> Album {
        model::parse_album(&json).unwrap()
    }

    #[test]
    fn artist_names_match_exactly_by_collaboration_or_by_a_close_prefix() {
        let artist = |id: &str, name: &str| Artist { id: id.into(), name: name.into(), photo_url: None };
        let pick = |list: Vec<Artist>, name: &str| best_artist_match(list, name).map(|a| a.id);
        assert_eq!(pick(vec![artist("1", "Noize MC feat. Монеточка"), artist("2", "Монеточка")], "монеточка"), Some("2".into()));
        assert_eq!(pick(vec![artist("1", "Noize MC feat. Монеточка")], "Монеточка"), Some("1".into()));
        assert_eq!(pick(vec![artist("1", "Земфира Рамазанова")], "Земфира"), None, "too short a share of the name");
        assert_eq!(pick(vec![artist("1", "Кино!")], "Кино"), Some("1".into()));
        assert_eq!(pick(vec![artist("1", "Другой")], "Кино"), None);
    }

    #[test]
    fn album_search_queries_and_the_preferred_match() {
        let wanted = album(json!({"id": 1, "owner_id": -2, "title": "Раскраски", "main_artists": [{"name": "Монеточка"}]}));
        assert_eq!(album_search_queries(&wanted), ["Раскраски Монеточка", "Монеточка Раскраски", "Раскраски", "Монеточка"]);
        let untitled = album(json!({"id": 1, "owner_id": -2, "title": "unnamed"}));
        assert!(album_search_queries(&untitled).is_empty());

        let candidates = vec![
            album(json!({"id": 5, "owner_id": -9, "title": "Раскраски", "main_artists": [{"name": "Кто-то"}], "access_key": "x"})),
            album(json!({"id": 6, "owner_id": -9, "title": "Раскраски (Deluxe)", "main_artists": [{"name": "Монеточка"}]})),
            album(json!({"id": 7, "owner_id": -9, "title": "Раскраски", "main_artists": [{"name": "Монеточка"}], "access_key": "y"})),
        ];
        assert_eq!(preferred_album_match(&candidates, &wanted).unwrap().album_id, 7, "title, artist and a key");
        let same = album(json!({"id": 1, "owner_id": -2, "title": "Другое", "access_key": "z"}));
        assert_eq!(preferred_album_match(&[candidates[0].clone(), same], &wanted).unwrap().album_id, 1, "the same id wins");
        assert!(preferred_album_match(&[], &wanted).is_none());
    }

    #[test]
    fn search_hits_belong_to_an_album_by_locator_or_by_title_and_artist() {
        let wanted = album(json!({"id": 1, "owner_id": -2, "title": "Раскраски", "main_artists": [{"name": "Монеточка"}]}));
        let track = |extra: serde_json::Value| {
            let mut v = json!({"id": 3, "owner_id": 4, "artist": "Монеточка", "title": "t"});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            model::parse_track(&v, None).unwrap()
        };
        assert!(track_belongs(&track(json!({"album": {"id": 1, "owner_id": -2}})), &wanted));
        assert!(track_belongs(&track(json!({"album_title": "раскраски для взрослых"})), &wanted));
        assert!(!track_belongs(&track(json!({"album_title": "Другой"})), &wanted));
        assert!(!track_belongs(&track(json!({"album_title": "Раскраски", "artist": "Кто-то"})), &wanted));
        assert!(!track_belongs(&track(json!({})), &wanted), "no album title, no locator");
        assert!(artist_matches(&track(json!({"artist": "Монеточка, Noize MC"})), &wanted));
    }

    #[test]
    fn denied_and_fatal_errors_are_told_apart() {
        for code in [15, 201, 203, 204] {
            assert!(is_access_denied(&Error::Vk { code, message: String::new() }));
        }
        assert!(is_access_denied(&Error::Vk { code: 7, message: "Permission to perform this action is denied".into() }));
        assert!(!is_access_denied(&Error::Vk { code: 3, message: "Unknown method passed".into() }));
        assert!(is_fatal(&Error::Offline) && is_fatal(&Error::Unauthorized) && !is_fatal(&Error::Http(500)));
    }

    #[test]
    fn the_catalogue_shape_names_structure_only() {
        let root = json!({
            "access_token": "SECRET-TOKEN",
            "catalog": {"sections": [
                {"id": "OPAQUE-SECTION-ID-1234", "title": "Миксы", "url": "https://vk.com/audios123456789?section=mixes", "blocks": []},
            ]},
            "blocks": [{"data_type": "music_audio_stream_mixes", "audio_stream_mixes": [{"mix_id": "daily", "title": "Личное для Иванова", "owner_id": 123456789}]}],
        });
        let mut sections = Vec::new();
        collect_catalog_sections(&root, &mut sections);
        unique_sections(&mut sections);
        let shape = catalog_shape(&root, 1, 0, &sections);
        for secret in ["SECRET-TOKEN", "OPAQUE-SECTION-ID", "123456789", "Иванова", "daily"] {
            assert!(!shape.contains(secret), "{secret} leaked: {shape}");
        }
        for wanted in ["access_token", "catalog", "blocks=1", "sections=1", "\"Миксы\"", "music_audio_stream_mixes", "audio_stream_mixes", "mix-like 1"] {
            assert!(shape.contains(wanted), "{wanted} missing: {shape}");
        }
    }

    #[test]
    fn logged_labels_are_short_and_carry_no_long_numbers() {
        assert_eq!(shown("audios123456789?x=1"), "audios123?x=1");
        assert!(shown(&"я".repeat(200)).chars().count() <= 49);
        assert_eq!(shown("a\nb\u{7}c"), "abc");
    }

    #[test]
    fn mix_markers_read_the_title_and_the_url_not_the_id() {
        let section = |id: &str, title: &str, url: Option<&str>| CatalogSection { id: id.into(), title: title.into(), url: url.map(str::to_string) };
        assert!(section("x", "Миксы для вас", None).looks_like_mix());
        assert!(section("x", "", Some("https://vk.com/audios1?section=mixes")).looks_like_mix());
        assert!(!section("mix_hidden_in_the_id", "Подкасты", None).looks_like_mix());
        assert!(section("x", "Новые релизы", None).looks_like_releases());
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
