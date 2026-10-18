//! Слой приложения: состояние входа, данные VK, обновление токена
//! и контроллер воспроизведения. Предоставляет окну единый интерфейс.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::audio::Sink;
use crate::controller::{Controller, Status, UiEvent};
use crate::history::{History, HistoryEntry, StatsView};
use crate::lyrics::{from_plain, GeniusLyrics, LrcLib, Lyrics};
use crate::net::{self, AddressSource, MonitorConfig, NetState, NetworkMonitor, SystemAddresses};
use crate::offline::{OfflineStore, OfflineTrack};
use crate::radio::{self, RadioFetch, RadioKind, RadioRequest};
use crate::stream::fetch::fetch_to_end;
use crate::store::SecretStore;
use crate::stream::FetchConfig;
use crate::vk::auth::{exchange_at, now_unix};
use crate::vk::{model, Album, AlbumTracks, Artist, ArtistAlbums, MusicMix, Page, Playlist, Profile, Session, Track, VkClient};
use crate::{Error, Result};

const TOKEN_URL: &str = "https://login.vk.ru/?act=web_token";

/// Самое дальнее смещение, которое может запросить окно в списке исполнителей или альбомов; Собственные
/// списки VK заканчиваются задолго до этого.
const MAX_OFFSET: u32 = 100_000;

/// Верхняя граница поискового запроса из окна.
const MAX_QUERY: usize = 500;

fn checked_offset(offset: u32) -> Result<u32> {
    (offset <= MAX_OFFSET).then_some(offset).ok_or(Error::InvalidRequest)
}

fn checked_person(owner_id: i64) -> Result<i64> {
    (owner_id > 0).then_some(owner_id).ok_or(Error::InvalidRequest)
}

fn checked_artist_id(id: &str) -> Result<&str> {
    let id = id.trim();
    model::valid_artist_id(id).then_some(id).ok_or(Error::InvalidRequest)
}

fn checked_mix_id(id: &str) -> Result<&str> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .then_some(id).ok_or(Error::InvalidRequest)
}

fn checked_query(query: &str) -> Result<Option<&str>> {
    if query.len() > MAX_QUERY {
        return Err(Error::InvalidRequest);
    }
    let query = query.trim();
    Ok((!query.is_empty()).then_some(query))
}

fn checked_mix_options(options: Option<&str>) -> Result<Option<String>> {
    let Some(options) = options else { return Ok(None) };
    if options.len() > 8_192 {
        return Err(Error::InvalidRequest);
    }
    let value: Value = serde_json::from_str(options).map_err(|_| Error::InvalidRequest)?;
    if !value.is_object() {
        return Err(Error::InvalidRequest);
    }
    Ok(Some(value.to_string()))
}

/// Альбом, переданный через окно: настоящий локатор и ключ доступа или хэш отслеживания, который
/// является простым токеном, поскольку оба отправляются обратно в VK.
fn checked_album(album: &Album) -> Result<()> {
    let token = |value: &Option<String>| {
        value.as_deref().is_none_or(|v| v.len() <= 256 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    };
    let artists_ok = album.artists.len() <= 50 && album.artists.iter().all(|a| a.name.len() <= 1024);
    if album.album_id <= 0
        || album.owner_id == 0
        || album.title.len() > 1024
        || !token(&album.access_key)
        || !token(&album.follow_hash)
        || !artists_ok
    {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}

/// Сообщает сетевому монитору, идёт ли загрузка или воспроизведение аудио.
type ActivityHook = Arc<dyn Fn(bool) + Send + Sync>;

/// Ошибки недействительной сессии VK: код 5 соответствует [`Error::Unauthorized`],
/// коды 17 и 28 могут появиться при смене IP-адреса и требуют обновления токена.
fn needs_new_session(error: &Error) -> bool {
    matches!(error, Error::Unauthorized | Error::Vk { code: 17 | 28, .. })
}

fn describe_vk(error: &Error) -> String {
    match error {
        Error::Vk { code, .. } => format!("error {code}"),
        other => other.kind().to_string(),
    }
}

/// Откуда пополняется бесконечная очередь, как называет ее окно.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::large_enum_variant)] // передается один раз на радио
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RadioSource {
    /// Персональный микс потока с примененной настройкой окна.
    Personal { options: Option<String> },
    /// Микс из каталога.
    Catalog { mix_id: String, options: Option<String> },
    /// Радио VK засеяно треком («Похожие треки»).
    Similar { seed: Track },
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionState {
    pub signed_in: bool,
    pub profile: Option<Profile>,
    /// Значение false, если в секретном хранилище OS произошел сбой, поэтому вход в систему не
    /// сохранится после перезапуска.
    pub persistent: bool,
}

/// Объект приложения, который хранится в оболочке. Делиться дешево: все находится в [`Core`], к
/// которому контроллер также может обратиться, чтобы обновить токен с истекшим сроком действия.
pub struct App {
    core: Arc<Core>,
}

impl std::ops::Deref for App {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

pub struct Core {
    controller: Controller,
    store: Box<dyn SecretStore>,
    emit: Arc<dyn Fn(UiEvent) + Send + Sync>,
    profile: Mutex<Option<Profile>>,
    persistent: Mutex<bool>,
    /// Одно обновление за раз: два экрана, одновременно выдающие 401, не должны повторно выдавать
    /// токен.
    refresh_lock: Mutex<()>,
    /// Сталкивался с каждым выходом из дома под этим замком. Обновление, начавшееся до выхода из
    /// системы, не должно возвращать пользователя в систему после завершения.
    generation: Mutex<u64>,
    token_url: String,
    lrclib: Option<LrcLib>,
    genius: Option<GeniusLyrics>,
    offline: Option<Arc<OfflineStore>>,
    history: Arc<History>,
    /// Следит за маршрутом в интернет; `None` при выключении.
    network: Option<NetworkMonitor>,
}

pub struct AppOptions {
    /// Где хранятся загрузки; `None` отключает их.
    pub offline_dir: Option<std::path::PathBuf>,
    /// Где хранится история игр и статистика; `None` хранит их только в памяти.
    pub history_dir: Option<std::path::PathBuf>,
    pub lyrics_base: String,
    /// Гениальный поисковый хост; пустое отключает резервный вариант Genius.
    pub genius_base: String,
    pub fetch: FetchConfig,
    pub token_url: String,
    pub volume: f32,
    /// Обратите внимание на изменения в сети (включение или выключение VPN) и исправьте
    /// воспроизведение. Тесты, которые не должны зависеть от сети машины, отключают ее.
    pub watch_network: bool,
    /// Где монитор считывает локальные адреса; `None` спрашивает OS.
    pub address_source: Option<Arc<dyn AddressSource>>,
    pub network_config: MonitorConfig,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            fetch: FetchConfig::new("PrivateMusicDesktop"),
            token_url: TOKEN_URL.to_string(),
            volume: 1.0,
            watch_network: true,
            address_source: None,
            network_config: MonitorConfig::default(),
            lyrics_base: crate::lyrics::LRCLIB_BASE.to_string(),
            genius_base: crate::lyrics::GENIUS_BASE.to_string(),
            offline_dir: None,
            history_dir: None,
        }
    }
}

impl App {
    pub fn new<F>(
        client: VkClient,
        make_sink: F,
        store: Box<dyn SecretStore>,
        emit: impl Fn(UiEvent) + Send + Sync + 'static,
        options: AppOptions,
    ) -> Self
    where
        F: FnMut() -> Result<Box<dyn Sink>> + Send + 'static,
    {
        let emit: Arc<dyn Fn(UiEvent) + Send + Sync> = Arc::new(emit);
        let controller_emit = emit.clone();
        // Монитору нужно смотреть только во время передачи звука.
        let activity: Arc<std::sync::OnceLock<ActivityHook>> = Arc::default();
        let activity_for_events = activity.clone();
        let controller = Controller::with_fetch(
            client,
            make_sink,
            move |event| {
                if let UiEvent::State(state) = &event {
                    if let Some(set_playing) = activity_for_events.get() {
                        set_playing(matches!(state.status, Status::Playing | Status::Buffering | Status::Loading));
                    }
                }
                controller_emit(event)
            },
            options.volume,
            options.fetch,
        );
        let durable = store.is_durable();
        let app = Core {
            controller,
            store,
            emit,
            profile: Mutex::new(None),
            persistent: Mutex::new(durable),
            refresh_lock: Mutex::new(()),
            generation: Mutex::new(0),
            token_url: options.token_url,
            lrclib: LrcLib::with_base(&options.lyrics_base).ok(),
            genius: GeniusLyrics::with_base(&options.genius_base).ok(),
            offline: options.offline_dir.and_then(|dir| {
                OfflineStore::open(dir).map_err(|e| log::warn!("downloads unavailable: {e}")).ok().map(Arc::new)
            }),
            history: History::open(options.history_dir),
            network: None,
        };
        app.controller.set_history(app.history.clone());
        if let Some(store) = &app.offline {
            app.controller.set_offline(store.clone());
        }
        match app.store.load() {
            Ok(Some(session)) => {
                if session.user_id.is_some() {
                    app.history.set_account(session.user_id);
                }
                app.controller.set_session(Some(session));
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!("secret store unavailable: {error}");
                *app.persistent.lock().unwrap_or_else(|p| p.into_inner()) = false;
            }
        }
        let mut app = app;
        if options.watch_network {
            let source = options.address_source.unwrap_or_else(|| Arc::new(SystemAddresses));
            let (to_ui, to_controller) = (app.emit.clone(), app.controller.network_listener());
            app.network = Some(NetworkMonitor::spawn(source, options.network_config, move |state| {
                match state {
                    NetState::Changed => {
                        log::info!("network changed: dropping connections");
                        net::drop_connections();
                    }
                    NetState::Unreachable => log::warn!("network unreachable: no route to the internet"),
                    NetState::Ok => log::info!("network settled"),
                }
                to_controller(state);
                to_ui(UiEvent::Network { state });
            }));
            if let Some(monitor) = &app.network {
                let handle = monitor.handle();
                let _ = activity.set(Arc::new(move |playing| handle.set_playing(playing)));
            }
        }
        let core = Arc::new(app);
        let weak = Arc::downgrade(&core);
        core.controller.set_refresher(Arc::new(move |stale: &Session| {
            weak.upgrade().ok_or(Error::Unauthorized)?.refresh(stale)
        }));
        App { core }
    }
}

impl App {
    /// Воспроизводит `tracks` из `index` и сохраняет заполнение очереди из `source` во время
    /// воспроизведения (см. [`radio`]). Треки — это то, что окно уже загрузило из того же источника.
    pub fn play_radio(&self, source: RadioSource, tracks: Vec<Track>, index: usize) -> Result<()> {
        self.play_radio_from(source, tracks, index, None)
    }

    pub fn play_radio_from(&self, source: RadioSource, tracks: Vec<Track>, index: usize, next_offset: Option<u32>) -> Result<()> {
        let next_offset = next_offset.unwrap_or(radio::FIRST_REFILL_OFFSET);
        if next_offset > 1_000_000 { return Err(Error::InvalidRequest); }
        let weak = Arc::downgrade(&self.core);
        let ask = move |call: &dyn Fn(&VkClient, &Session) -> Result<Vec<Track>>| -> Result<Vec<Track>> {
            weak.upgrade().ok_or(Error::Unauthorized)?.api(call)
        };
        let (kind, offset, seeds, fetch): (RadioKind, u32, Vec<Track>, RadioFetch) = match source {
            RadioSource::Personal { options } => {
                let options = checked_mix_options(options.as_deref())?;
                let fetch: RadioFetch = Arc::new(move |request| {
                    let RadioRequest::Page { offset } = request else { return Ok(Vec::new()) };
                    let (offset, options) = (*offset, options.clone());
                    ask(&|client, session| client.stream_mix(session, "common", offset, radio::PAGE_SIZE, options.as_deref()))
                });
                (RadioKind::Personal, next_offset, Vec::new(), fetch)
            }
            RadioSource::Catalog { mix_id, options } => {
                let mix_id = checked_mix_id(&mix_id)?.to_string();
                let options = checked_mix_options(options.as_deref())?;
                let fetch: RadioFetch = Arc::new(move |request| {
                    let RadioRequest::Page { offset } = request else { return Ok(Vec::new()) };
                    let (offset, options, mix_id) = (*offset, options.clone(), mix_id.clone());
                    ask(&|client, session| client.stream_mix(session, &mix_id, offset, radio::PAGE_SIZE, options.as_deref()))
                });
                (RadioKind::Catalog, next_offset, Vec::new(), fetch)
            }
            RadioSource::Similar { seed } => {
                if seed.track_id <= 0 || seed.owner_id == 0 {
                    return Err(Error::InvalidRequest);
                }
                let fetch: RadioFetch = Arc::new(move |request| {
                    let RadioRequest::Seed(seed) = request else { return Ok(Vec::new()) };
                    let seed = seed.clone();
                    ask(&|client, session| client.shuffled_recommendations(session, &seed))
                });
                // Сначала семя, а затем то, что окно уже получило за него.
                let seeds = std::iter::once(seed).chain(tracks.iter().cloned()).collect();
                (RadioKind::Similar, 0, seeds, fetch)
            }
        };
        if tracks.is_empty() {
            return Err(Error::InvalidRequest);
        }
        self.controller.play_radio(tracks, index, kind, fetch, offset, seeds);
        Ok(())
    }
}

impl Core {
    pub fn controller(&self) -> &Controller {
        &self.controller
    }

    fn lock<'a, T>(mutex: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
        mutex.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Принимает новый сеанс (только что вошедший в систему или только что обновленный).
    pub fn sign_in(&self, session: Session) {
        let mut generation = Self::lock(&self.generation);
        *generation += 1;
        if let Err(error) = self.store.begin_login() { log::warn!("could not guard account replacement: {error}"); }
        self.adopt(session);
    }

    fn adopt(&self, session: Session) {
        let saved = match self.store.save(&session) {
            Ok(()) => self.store.is_durable(),
            Err(error) => {
                log::warn!("could not save the session: {error}");
                false
            }
        };
        *Self::lock(&self.persistent) = saved;
        *Self::lock(&self.profile) = None;
        // Обновленный токен может не называть своего пользователя: сохраните ту учетную запись, которую
        // мы знаем.
        if session.user_id.is_some() {
            self.history.set_account(session.user_id);
        }
        self.controller.set_session(Some(session));
    }

    pub fn sign_out(&self) {
        let mut generation = Self::lock(&self.generation);
        *generation += 1;
        self.clear_session();
    }

    fn clear_session(&self) {
        if let Err(error) = self.store.clear() {
            log::warn!("could not clear the session: {error}");
        }
        *Self::lock(&self.profile) = None;
        self.history.set_account(None);
        self.controller.set_session(None);
    }

    /// Повторите временную ошибку учетных данных OS без замены поставщика.
    pub fn retry_session_storage(&self) {
        let _generation = Self::lock(&self.generation);
        if !self.store.is_durable() { return; }
        if let Some(session) = self.controller.session() {
            *Self::lock(&self.persistent) = self.store.save(&session).is_ok();
        } else if !*Self::lock(&self.persistent) {
            if let Ok(session) = self.store.load() {
                if let Some(session) = session {
                    self.history.set_account(session.user_id);
                    self.controller.set_session(Some(session));
                }
                *Self::lock(&self.persistent) = true;
            }
        }
    }

    pub fn session_state(&self) -> SessionState {
        let retry = !*Self::lock(&self.persistent);
        if retry { self.retry_session_storage(); }
        SessionState {
            signed_in: self.controller.session().is_some(),
            profile: Self::lock(&self.profile).clone(),
            persistent: *Self::lock(&self.persistent),
        }
    }

    /// Повторно выдает токен из сохраненного веб-файла cookie.
    fn refresh(&self, stale: &Session) -> Result<Session> {
        let _one_at_a_time = Self::lock(&self.refresh_lock);
        let started = *Self::lock(&self.generation);
        // Возможно, кто-то еще обновился, пока мы ждали блокировки.
        let current = self.controller.session().ok_or(Error::Unauthorized)?;
        if current.access_token != stale.access_token {
            // Обновление можно использовать повторно, но вызов API не может пересекать учетные записи.
            if current.user_id != stale.user_id { return Err(Error::Unauthorized); }
            return Ok(current);
        }
        match exchange_at(&self.token_url, &stale.refresh_cookie, &stale.web_user_agent, now_unix()) {
            Ok(mut fresh) => {
                fresh.user_id = fresh.user_id.or(stale.user_id);
                let generation = Self::lock(&self.generation);
                if *generation != started || self.controller.session().is_none() {
                    // Вышел из системы, пока VK отвечал: оставайтесь вне системы.
                    return Err(Error::Unauthorized);
                }
                self.adopt(fresh.clone());
                drop(generation);
                Ok(fresh)
            }
            Err(Error::Unauthorized) => {
                let mut generation = Self::lock(&self.generation);
                if *generation != started {
                    return Err(Error::Unauthorized);
                }
                *generation += 1;
                self.clear_session();
                drop(generation);
                (self.emit)(UiEvent::SessionExpired);
                Err(Error::Unauthorized)
            }
            Err(other) => Err(other),
        }
    }

    /// Выполняет вызов VK с действительным токеном: обновляется до истечения срока действия и еще раз,
    /// если VK все равно отвечает на 401.
    pub fn api<T>(&self, call: impl Fn(&VkClient, &Session) -> Result<T>) -> Result<T> {
        let retry = !*Self::lock(&self.persistent);
        if retry { self.retry_session_storage(); }
        let (started, mut session) = {
            let generation = Self::lock(&self.generation);
            (*generation, self.controller.session().ok_or(Error::Unauthorized)?)
        };
        if session.needs_refresh(now_unix()) {
            session = match self.refresh(&session) {
                Ok(fresh) => fresh,
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(_) => session, // временный сбой сети должен сохранить файл cookie
            };
        }
        if *Self::lock(&self.generation) != started { return Err(Error::Unauthorized); }
        let answer = call(self.controller.client(), &session);
        if *Self::lock(&self.generation) != started { return Err(Error::Unauthorized); }
        match answer {
            // Сеанс VK перестал принимать, что также показывает измененный IP (токен был отчеканен в
            // другой сети): один раз обновите его из cookie и один раз спросите еще раз. Никогда не
            // петля.
            Err(error) if needs_new_session(&error) => {
                log::info!("VK refused the session ({}), renewing it once", describe_vk(&error));
                let fresh = match self.refresh(&session) {
                    Ok(fresh) => fresh,
                    // Собственный сбой продления объясняет отказ во входе; для других ошибок полезен
                    // исходный ответ.
                    Err(renewal) if error == Error::Unauthorized => return Err(renewal),
                    Err(_) => return Err(error),
                };
                let answer = call(self.controller.client(), &fresh);
                if *Self::lock(&self.generation) != started { return Err(Error::Unauthorized); }
                answer
            }
            other => other,
        }
    }

    /// Сообщает движку, привлекает ли окно внимание пользователя; сеть просматривается только во время
    /// ее работы или во время воспроизведения звука.
    pub fn set_window_focused(&self, focused: bool) {
        if let Some(network) = &self.network {
            network.set_focused(focused);
        }
    }

    /// Пользователь нажал «Повторить» после сбоя сети: забудьте обо всех соединениях пула и посмотрите на
    /// сеть сейчас.
    pub fn check_network(&self) {
        net::drop_connections();
        if let Some(network) = &self.network {
            network.check_now();
        }
    }

    /// «Повторить» для воспроизведения, остановленного из-за проблемы с подключением.
    pub fn retry_playback(&self) {
        self.controller.retry();
    }

    pub fn profile(&self) -> Result<Profile> {
        if let Some(profile) = Self::lock(&self.profile).clone() {
            return Ok(profile);
        }
        let started = *Self::lock(&self.generation);
        let profile = self.api(|client, session| client.profile(session))?;
        let generation = Self::lock(&self.generation);
        if *generation != started { return Err(Error::Unauthorized); }
        if let Some(mut session) = self.controller.session() {
            if session.user_id.is_none() {
                session.user_id = Some(profile.id);
                *Self::lock(&self.persistent) = self.store.save(&session).is_ok() && self.store.is_durable();
                self.controller.set_session(Some(session));
            }
        }
        // Сеанс, в котором не указано имя пользователя: история начинается сейчас.
        if self.history.account().is_none() && self.controller.session().is_some() {
            self.history.set_account(Some(profile.id));
        }
        *Self::lock(&self.profile) = Some(profile.clone());
        Ok(profile)
    }

    pub fn library(&self, offset: u32, count: u32) -> Result<Page<Track>> {
        self.api(|client, session| client.library(session, offset, count.clamp(1, 300)))
    }

    pub fn search(&self, query: &str, offset: u32, count: u32) -> Result<Page<Track>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Page { items: Vec::new(), total: 0, next_offset: None });
        }
        self.api(|client, session| client.search(session, query, offset, count.clamp(1, 300)))
    }

    fn offline(&self) -> Result<&OfflineStore> {
        self.offline.as_deref().ok_or_else(|| Error::Stream("downloads are not available".into()))
    }

    pub fn downloads(&self) -> Vec<OfflineTrack> {
        self.offline.as_ref().map(|o| o.list()).unwrap_or_default()
    }

    pub fn downloads_size(&self) -> u64 {
        self.offline.as_ref().map(|o| o.total_bytes()).unwrap_or(0)
    }

    /// Скачивает трек для прослушивания без подключения. Сначала запрашивается новый адрес потока: срок
    /// действия адреса, хранящегося в окне, возможно, истек.
    pub fn download(&self, track: &Track) -> Result<OfflineTrack> {
        let store = self.offline()?;
        if let Some(existing) = store.list().into_iter().find(|e| e.track.id() == track.id()) {
            if std::fs::metadata(store.dir().join(&existing.file)).is_ok_and(|file| file.is_file() && file.len() == existing.bytes && file.len() > 0) {
                return Ok(existing);
            }
            // Запись в кэше может пережить файл, удаленный или усеченный за пределами приложения.
            // Загрузка и экспорт должен восстановить его вместо возврата успешного результата,
            // указывающего на отсутствие звука.
            store.remove(&track.id())?;
        }
        for attempt in 0..3 {
            let result = (|| {
                let fresh = self.api(|client, session| client.refreshed_track(session, track))?;
                let url = fresh.stream_url.as_deref().ok_or(Error::Unavailable)?;
                fetch_to_end(url, &self.controller.fetch_config())
            })();
            match result {
                Ok((kind, audio)) => return store.save(&Track { stream_url: None, ..track.clone() }, kind, &audio),
                Err(error) if attempt < 2 && (net::is_connectivity(&error) || matches!(error, Error::Http(401 | 403 | 404 | 410 | 408 | 429 | 500..=599))) => {
                    log::info!("download {}: refreshing the stream address (attempt {}/3)", error.kind(), attempt + 2);
                    net::drop_connections();
                    std::thread::sleep(std::time::Duration::from_millis(300 * (attempt + 1)));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("every download attempt returns or retries within the bound")
    }

    pub fn remove_download(&self, track_id: &str) -> Result<bool> {
        self.offline()?.remove(track_id)
    }

    /// Персональный микс: сразу несколько страниц (см. `VkClient::mix_window`).
    pub fn mix(&self) -> Result<Vec<Track>> {
        self.mix_with_options(None)
    }

    pub fn mix_with_options(&self, options: Option<&str>) -> Result<Vec<Track>> {
        self.stream_mix_at("common", options, 0)
    }

    pub fn mix_settings(&self) -> Result<Value> {
        self.api(|client, session| client.mix_settings(session))
    }

    pub fn catalog_mixes(&self) -> Result<Vec<MusicMix>> {
        self.api(|client, session| client.catalog_mixes(session))
    }

    pub fn stream_mix(&self, mix_id: &str, options: Option<&str>) -> Result<Vec<Track>> {
        self.stream_mix_at(mix_id, options, 0)
    }

    pub fn stream_mix_at(&self, mix_id: &str, options: Option<&str>, offset: u32) -> Result<Vec<Track>> {
        if offset > 1_000_000 { return Err(Error::InvalidRequest); }
        let mix_id = checked_mix_id(mix_id)?.to_string();
        let options = checked_mix_options(options)?;
        self.api(|client, session| client.mix_window_at(session, &mix_id, options.as_deref(), offset))
    }

    /// Полка «Новые релизы» из каталога VK.
    pub fn new_releases(&self) -> Result<Vec<Album>> {
        self.api(|client, session| client.new_releases(session))
    }

    pub fn recommendations(&self) -> Result<Vec<Track>> {
        self.api(|client, session| client.recommendations(session, None))
    }

    /// "More like this": Радио VK засеяно одним треком.
    pub fn similar(&self, track: &Track) -> Result<Vec<Track>> {
        self.api(|client, session| client.recommendations(session, Some(track)))
    }

    pub fn create_playlist(&self, title: &str, description: &str) -> Result<Playlist> {
        self.api(|client, session| client.create_playlist(session, title, description))
    }

    pub fn edit_playlist(&self, playlist: &Playlist, title: &str, description: &str) -> Result<Playlist> {
        self.api(|client, session| client.edit_playlist(session, playlist, title, description))
    }

    pub fn delete_playlist(&self, playlist: &Playlist) -> Result<()> {
        self.api(|client, session| client.delete_playlist(session, playlist))
    }

    pub fn add_to_playlist(&self, playlist: &Playlist, track: &Track) -> Result<()> {
        self.api(|client, session| client.change_playlist(session, playlist, track, true))
    }

    pub fn remove_from_playlist(&self, playlist: &Playlist, track: &Track) -> Result<()> {
        self.api(|client, session| client.change_playlist(session, playlist, track, false))
    }

    pub fn add_to_library(&self, track: &Track) -> Result<Track> {
        self.api(|client, session| client.add_track(session, track))
    }

    pub fn remove_from_library(&self, track: &Track) -> Result<()> {
        self.api(|client, session| client.delete_track(session, track))
    }

    /// Текст для трека: сначала приурочен LRCLIB, затем точный идентификатор текста VK, Genius, когда
    /// его результат поиска совпадает, затем простой текст LRCLIB.
    ///
    /// Источник, который терпит неудачу, не скрывает ответ другого источника. `Err` возвращается только
    /// тогда, когда ни один источник не дал однозначного ответа ("found" или "none"), поэтому
    /// пользователю сообщают "could not load" только тогда, когда мы действительно не можем это
    /// выяснить; в любом случае ошибка записывается в журнал.
    pub fn lyrics(&self, track: &Track) -> Result<Option<Lyrics>> {
        let mut failure: Option<Error> = None;
        let mut answered = false;
        let lrc = match self.lrclib.as_ref().map(|l| l.lookup(track)) {
            Some(Ok(found)) => {
                answered = true;
                found
            }
            Some(Err(error)) => {
                log::warn!("lyrics source failed: {}", crate::lyrics::describe("lrclib", &error));
                failure = Some(error);
                None
            }
            None => None,
        };
        if lrc.as_ref().is_some_and(|l| !l.lines.is_empty()) {
            return Ok(lrc);
        }
        if let Some(id) = track.lyrics_id {
            match self.api(|client, session| client.lyrics_text(session, id)) {
                Ok(Some(text)) => {
                    answered = true;
                    if let Some(found) = from_plain("VK", &text) {
                        return Ok(Some(found));
                    }
                }
                Ok(None) => answered = true,
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    log::warn!("lyrics source failed: {}", crate::lyrics::describe("vk lyrics", &error));
                    failure = failure.or(Some(error));
                }
            }
        }
        if let Some(genius) = &self.genius {
            match genius.lookup(track) {
                Ok(Some(found)) => return Ok(Some(found)),
                Ok(None) => answered = true,
                Err(error) => {
                    // Genius — это дополнительный источник последней надежды. Его общедоступные
                    // конечные точки могут отклонять автоматические запросы; это не должно превращать
                    // "no lyrics found" в ошибку воспроизведения.
                    log::warn!("lyrics source failed: {}", crate::lyrics::describe("genius", &error));
                    failure = failure.or(Some(error));
                }
            }
        }
        match (lrc, failure) {
            (Some(found), _) => Ok(Some(found)),
            (None, Some(error)) if !answered => Err(error),
            (None, _) => Ok(None),
        }
    }

    // История игр и статистика хранятся только на этом компьютере.

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.history.entries()
    }

    pub fn history_remove(&self, track_id: &str) -> Result<bool> {
        if track_id.is_empty() || track_id.len() > 64 || !track_id.chars().all(|c| c.is_ascii_digit() || c == '_' || c == '-') {
            return Err(Error::InvalidRequest);
        }
        Ok(self.history.remove(track_id))
    }

    pub fn history_clear(&self) {
        self.history.clear();
    }

    pub fn listening_stats(&self, scope: &str, choice: Option<&str>) -> Result<StatsView> {
        self.history.stats(scope, choice)
    }

    pub fn reset_listening_stats(&self) {
        self.history.reset_stats();
    }

    /// Минуты к востоку от UTC, указанные в окне, для часов и дней недели.
    pub fn set_utc_offset(&self, minutes: i32) -> Result<()> {
        self.history.set_utc_offset(minutes)
    }

    /// Сейчас записывает историю на диск (приложение закрывается).
    pub fn flush_history(&self) {
        self.history.flush();
    }

    pub fn playlists(&self, offset: u32, count: u32) -> Result<Page<Playlist>> {
        self.api(|client, session| client.playlists(session, offset, count.clamp(1, 200)))
    }

    /// Друзья вошедшего пользователя.
    pub fn friends(&self, offset: u32, count: u32) -> Result<Page<Profile>> {
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.friends(session, offset, count.clamp(1, 200)))
    }

    /// Следы друга. `owner_id` должен быть человеком (идентификаторы людей VK положительны), а не
    /// сообществом и никогда "0".
    pub fn friend_tracks(&self, owner_id: i64, offset: u32, count: u32) -> Result<Page<Track>> {
        let owner_id = checked_person(owner_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.friend_tracks(session, owner_id, offset, count.clamp(1, 300)))
    }

    pub fn friend_playlists(&self, owner_id: i64, offset: u32, count: u32) -> Result<Page<Playlist>> {
        let owner_id = checked_person(owner_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.friend_playlists(session, owner_id, offset, count.clamp(1, 200)))
    }

    pub fn playlist_tracks(&self, playlist: &Playlist, offset: u32, count: u32) -> Result<Page<Track>> {
        self.api(|client, session| client.playlist_tracks(session, playlist, offset, count.clamp(1, 300)))
    }

    pub fn search_artists(&self, query: &str, offset: u32, count: u32) -> Result<Vec<Artist>> {
        let offset = checked_offset(offset)?;
        let Some(query) = checked_query(query)? else { return Ok(Vec::new()) };
        self.api(|client, session| client.search_artists(session, query, offset, count.clamp(1, 100)))
    }

    /// Под названием исполнитель имел в виду треки, которые не связаны между собой.
    pub fn find_artist(&self, name: &str) -> Result<Option<Artist>> {
        let Some(name) = checked_query(name)? else { return Ok(None) };
        self.api(|client, session| client.find_artist(session, name))
    }

    pub fn artist_tracks(&self, artist_id: &str, offset: u32, count: u32) -> Result<Page<Track>> {
        let artist_id = checked_artist_id(artist_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.artist_tracks(session, artist_id, offset, count.clamp(1, 100)))
    }

    pub fn artist_albums(&self, artist_id: &str, offset: u32, count: u32) -> Result<ArtistAlbums> {
        let artist_id = checked_artist_id(artist_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.artist_albums(session, artist_id, offset, count.clamp(1, 100)))
    }

    pub fn search_albums(&self, query: &str, offset: u32, count: u32) -> Result<Page<Album>> {
        let offset = checked_offset(offset)?;
        let Some(query) = checked_query(query)? else {
            return Ok(Page { items: Vec::new(), total: 0, next_offset: None });
        };
        self.api(|client, session| client.search_albums(session, query, offset, count.clamp(1, 100)))
    }

    pub fn followed_albums(&self, offset: u32, count: u32) -> Result<Page<Album>> {
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.followed_albums(session, offset, count.clamp(1, 100)))
    }

    pub fn playlist_by_id(&self, album: &Album) -> Result<Album> {
        checked_album(album)?;
        self.api(|client, session| client.playlist_by_id(session, album))
    }

    pub fn album_tracks(&self, album: &Album, offset: u32, count: u32) -> Result<AlbumTracks> {
        checked_album(album)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.album_tracks(session, album, offset, count.clamp(1, 100)))
    }

    pub fn follow_album(&self, album: &Album, follow: bool) -> Result<()> {
        checked_album(album)?;
        self.api(|client, session| client.follow_album(session, album, follow))
    }
}
