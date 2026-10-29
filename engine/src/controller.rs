//! Потокобезопасное управление очередью, плеером и запросами VK.
//!
//! Интерфейс вызывает методы контроллера и получает [`UiEvent`]. Здесь задаются
//! переход к следующему треку, повтор, перемешивание и восстановление ошибок.
//!
//! Истёкший адрес потока заменяется свежим, а воспроизведение продолжается
//! с сохранённой позиции по правилам [`RecoveryPolicy`]. При сетевой ошибке
//! трек не пропускается: после ограниченного числа попыток плеер ждёт повтора
//! или восстановления сети. Недоступные треки пропускаются не больше трёх подряд.
//! Запросы не выполняются в аудиопотоке или под блокировкой состояния.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Serialize;

use crate::history::History;
use crate::net::{self, NetState};
use crate::audio::{PlayerEvent, PlayerHandle, Remote, Sink};
use crate::offline::OfflineStore;
use crate::queue::{Queue, Repeat};
use crate::radio::{self, Radio, RadioFetch, RadioKind, RadioContinuation, RadioSetup};
use crate::stream::FetchConfig;
use crate::vk::{Session, Track, VkClient};
use crate::Error;

const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Нажатие кнопки «Предыдущий» после этого количества секунд вместо этого перезапускает дорожку.
const RESTART_THRESHOLD: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
}

#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub status: Status,
    pub track: Option<Track>,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub queue_index: Option<usize>,
    pub queue_len: usize,
    /// Whether a manual skip has a queued destination or an active radio continuation.
    pub can_next: bool,
    /// Установите, пока очередь пополняется из микса или радиоприемника.
    pub radio: Option<RadioKind>,
    pub radio_context: Option<RadioContinuation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // события кратковременны и переносятся один раз
pub enum UiEvent {
    SleepTimer { remaining: Option<u64>, end_of_track: bool },
    State(State),
    Queue { tracks: Vec<Track>, current: Option<usize> },
    Error { kind: String, message: String },
    /// Токен VK больше не работает: покажите экран входа.
    SessionExpired,
    /// Уровни визуализатора (0…1), около 30 в секунду при включенном режиме.
    Spectrum { bands: Vec<f32> },
    /// Маршрут к Интернету изменился, исчез или прекратился (см. [`crate::net`]).
    Network { state: NetState },
    /// Воспроизведение восстанавливается после сбоя потока.
    Recovery { phase: RecoveryPhase, attempt: u32, max: u32, kind: String, retry_in_secs: Option<u64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryPhase {
    /// Новая попытка запланирована или выполняется.
    Retrying,
    /// Достигнута граница: воспроизведение приостановлено и ожидает повторной попытки.
    Failed,
    /// Трек воспроизводится снова в течение нескольких секунд.
    Recovered,
}

/// Как упорно восстанавливается прерванный ручей. Цифры соответствуют `StreamFailureRetryPolicy`
/// приложения iOS (3 попытки одного и того же трека для мертвого URLs, более широкий бюджет на сбои
/// соединения, их никогда не пропускают) с экспоненциальным замедлением и джиттером на вершине.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryPolicy {
    /// Попытки после сбоя соединения (сброс, таймаут, DNS, TLS, 5xx).
    pub connectivity_attempts: u32,
    /// Попытки после адреса перестали работать (401/403/404/410, неверные данные).
    pub content_attempts: u32,
    /// Аудио, которое остается сухим так долго, считается неудачным потоком.
    pub stall_after: Duration,
    /// Секунды звука после восстановления, прежде чем он будет считаться восстановленным (поток,
    /// который прекращается раньше, продолжает считать попытки с тем же числом).
    pub probation_secs: f64,
    /// Умножает каждое ожидание; тесты уменьшают его.
    pub time_scale: f64,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            connectivity_attempts: 6,
            content_attempts: 3,
            stall_after: Duration::from_secs(20),
            probation_secs: 3.0,
            time_scale: 1.0,
        }
    }
}

impl RecoveryPolicy {
    pub fn attempts(&self, connectivity: bool) -> u32 {
        if connectivity { self.connectivity_attempts } else { self.content_attempts }
    }

    /// Подождите перед попыткой номер `attempt` (от 1). Мертвый адрес переразрешается сразу с первого
    /// раза, как всегда; плохая связь получает преимущество в первую очередь. Последующие попытки
    /// отступают: 1,2, 2,4, 4,8… секунды, до 20 (6 для мертвых адресов), ±15 %.
    pub fn delay(&self, connectivity: bool, attempt: u32, unit: f64) -> Duration {
        let unit = if unit.is_finite() { unit.clamp(0.0, 1.0) } else { 0.5 };
        let seconds = match (connectivity, attempt) {
            (false, 0 | 1) => 0.0,
            (true, 0 | 1) => 0.4 * (0.85 + 0.3 * unit),
            (_, n) => net::backoff(n - 2, 1.2, if connectivity { 20.0 } else { 6.0 }, 0.15, unit),
        };
        self.scaled(seconds)
    }

    fn scaled(&self, seconds: f64) -> Duration {
        let scale = if self.time_scale.is_finite() { self.time_scale.max(0.0) } else { 1.0 };
        Duration::from_secs_f64((seconds * scale).clamp(0.0, 3600.0))
    }

    /// После того, как сеть восстановилась: короткий резкий старт, а не замедление, свойственное
    /// мертвому пути.
    pub fn network_return_delay(&self, unit: f64) -> Duration {
        let unit = if unit.is_finite() { unit.clamp(0.0, 1.0) } else { 0.5 };
        self.scaled(0.1 + 0.4 * unit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Соединение или маршрут плохие: подождите, разорвите соединение, повторите попытку.
    Connectivity,
    /// Адрес недействителен или данные неверны: запросите у VK новый.
    Content,
    /// Восстановление ничего не может исправить (доступ запрещен, требуется вход в систему и т. д.).
    Fatal,
}

fn classify(error: &Error) -> Class {
    match error {
        Error::Timeout | Error::Offline | Error::Transport(_) => Class::Connectivity,
        Error::Http(408 | 429 | 500..=599) => Class::Connectivity,
        Error::RetryAfter { .. } => Class::Connectivity,
        // 9.06.10: слишком много запросов, контроль флуда, внутренняя ошибка VK.
        Error::Vk { code: 6 | 9 | 10, .. } => Class::Connectivity,
        Error::Stream(message) if message.contains("audio output") || message.starts_with("audio device") => Class::Connectivity,
        Error::Http(_) | Error::Stream(_) => Class::Content,
        _ => Class::Fatal,
    }
}

/// Логически безопасное имя для сбоя (никогда не URL).
fn what(error: &Error) -> String {
    match error {
        Error::Http(status) => status.to_string(),
        Error::Stream(message) => {
            let category = if message.contains("audio output") || message.starts_with("audio device") { "audio output unavailable" }
                else if message.starts_with("decoder:") { "audio decoding failed" }
                else if message.contains("size limit") { "stream size limit" }
                else if message.contains("playlist") { "invalid playlist" }
                else if message.contains("initialization") || message.contains("AAC configuration") { "unsupported stream initialization" }
                else if message.contains("empty response") || message.contains("held no audio") || message.contains("ended before any audio") { "empty audio stream" }
                else if message.starts_with("resampler:") || message=="invalid audio format" { "audio conversion failed" }
                else { "stream error" };
            category.into()
        },
        Error::Timeout => "timeout".into(),
        Error::Offline => "connect failed".into(),
        Error::Transport(_) => "connection lost".into(),
        other => other.kind().into(),
    }
}

struct Inner {
    prefetch: bool,
    advance_on_error: bool,
    prefer_high_quality: bool,
    prepared_tag: Option<(String,String,u64)>,
    preparation_generation:u64,
    sleep_end: bool,
    user_paused: bool,
    queue: Queue,
    session: Option<Session>,
    status: Status,
    position: f64,
    duration: f64,
    volume: f32,
    rng: StdRng,
    failures: u32,
    /// Во время этой попытки треки уже были повторены с новым URL.
    refreshed: HashSet<String>,
    /// Где начинается следующий старт текущего трека (восстановленная сессия).
    pending_start: f64,
    /// Бесконечный источник, из которого пополняется очередь, если она пришла из него.
    radio: Option<Radio>,
    radio_generation: u64,
    /// Неисправность исцеляется, если таковая имеется.
    recovery: Option<Recovery>,
    /// Столкнулся с необходимостью отменить попытку, ожидающую отмены.
    recovery_token: u64,
    /// Воспроизведение прекратилось из-за проблемы с подключением и возобновилось само собой, когда
    /// сеть стабилизируется.
    gave_up: bool,
    /// Натолкнулся на отмену сторожевого таймера предыдущей буферизации.
    stall_token: u64,
    /// Сталкивался с каждым изменением сети; URLs, разрешенные под более старым значением, не являются
    /// доверенными (VK связывает их с запрашивавшей сетью).
    network_epoch: u64,
    /// Идентификатор трека → эпоха сети, в которой был разрешен его поток URL.
    fresh: HashMap<String, u64>,
    /// Текущий трек воспроизводится из загруженного файла: здесь ничего не применимо.
    source_offline: bool,
    loading_since: Instant,
}

struct Recovery {
    track_id: String,
    /// На данный момент неудачных попыток, что также является номером попытки, которую они запросили.
    attempts: u32,
    /// Граница вида сбоя, который наблюдался последним, и его имя для окна.
    max: u32,
    kind: &'static str,
    started: Instant,
    /// Установите, когда звук возобновится: позиция, с которой он возобновился.
    probation_from: Option<f64>,
}

type TimerCancel = Arc<(Mutex<bool>, std::sync::Condvar)>;

struct Shared {
    taste: Mutex<crate::taste::Taste>,
    sleep_cancel: Mutex<Option<TimerCancel>>,
    recovery_cancel: Mutex<Option<TimerCancel>>,
    stall_cancel: Mutex<Option<TimerCancel>>,
    sleep_deadline: Mutex<Option<Instant>>,
    inner: Mutex<Inner>,
    client: VkClient,
    remote: OnceLock<Remote>,
    emit: Box<dyn Fn(UiEvent) + Send + Sync>,
    fetch_template: FetchConfig,
    refresher: OnceLock<Refresher>,
    offline: OnceLock<Arc<OfflineStore>>,
    history: OnceLock<Arc<History>>,
    policy: Mutex<RecoveryPolicy>,
}

/// Обновляет истекший сеанс (см. [`Controller::set_refresher`]).
pub type Refresher = Arc<dyn Fn(&Session) -> crate::Result<Session> + Send + Sync>;

pub struct Controller {
    shared: Arc<Shared>,
    _player: PlayerHandle,
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shared.cancel_recovery_wait();
        self.shared.cancel_stall_wait();
        if let Some(cancel) = self.shared.sleep_cancel.lock().unwrap_or_else(|p| p.into_inner()).take() {
            *cancel.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
            cancel.1.notify_all();
        }
    }
}

impl Inner {
    fn can_next(&self) -> bool {
        self.queue.peek_next(false).is_some() ||
            (self.queue.current().is_some() && self.radio.as_ref().is_some_and(|r| !r.done))
    }

    fn state(&self) -> State {
        State {
            status: self.status,
            track: self.queue.current().cloned(),
            position: self.position,
            duration: self.duration,
            volume: self.volume,
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            queue_index: self.queue.current_index(),
            queue_len: self.queue.len(),
            can_next: self.can_next(),
            radio: self.radio.as_ref().map(|r| r.kind),
            radio_context: self.radio.as_ref().and_then(Radio::continuation),
        }
    }

    fn queue_event(&self) -> UiEvent {
        UiEvent::Queue { tracks: self.queue.tracks().to_vec(), current: self.queue.current_index() }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn remote(&self) -> &Remote {
        self.remote.get().expect("controller is fully built")
    }

    /// Запускает `f` под блокировкой, затем выдает то, что вернул после разблокировки.
    fn with<R>(self: &Arc<Self>, f: impl FnOnce(&mut Inner, &mut Vec<UiEvent>) -> R) -> R {
        let mut events = Vec::new();
        let result = {
            let mut inner = self.lock();
            let could_skip = inner.can_next();
            let result=f(&mut inner, &mut events);
            let can_skip = inner.can_next();
            // Appending/removing queue entries can change transport actions
            // without changing the track, position or playback status.
            if could_skip != can_skip && !events.iter().any(|event| matches!(event, UiEvent::State(state) if state.can_next == can_skip)) {
                events.push(UiEvent::State(inner.state()));
            }
            self.prepare_next(&mut inner);
            result
        };
        for event in events {
            (self.emit)(event);
        }
        result
    }

    fn prepare_next(self:&Arc<Self>,inner:&mut Inner){
        // Called on every position report on the audio thread: inspecting the next
        // entry must stay constant-time even for a library-sized queue.
        let track=if inner.prefetch && !inner.sleep_end && !inner.user_paused && inner.status==Status::Playing {inner.queue.peek_next(true)}else{None};
        let tag=track.and_then(|track|inner.queue.current().map(|current|(current.id(),track.id(),inner.network_epoch)));
        if tag==inner.prepared_tag{return;}
        let track=track.cloned();
        inner.prepared_tag=tag.clone();
        inner.preparation_generation=inner.preparation_generation.wrapping_add(1);
        let generation=inner.preparation_generation;
        self.remote().send(crate::audio::Command::ClearPrepared);
        let Some(track)=track else{return};
        let config=self.fetch_config(inner);let session=inner.session.clone();
        let downloaded=self.offline.get().and_then(|offline|offline.address(&track.id()));
        let stale=self.is_stale(inner,&track);
        let weak=Arc::downgrade(self);
        std::thread::spawn(move||{
            let Some(shared)=weak.upgrade()else{return};
            let resolved=if let Some(address)=downloaded{Ok((track.clone(),address))}else if !stale && track.stream_url.is_some(){Ok((track.clone(),track.stream_url.clone().unwrap_or_default()))}else if let Some(session)=session{shared.client.refreshed_track_once(&session,&track).and_then(|fresh|fresh.stream_url.clone().map(|url|(fresh,url)).ok_or(Error::InvalidRequest))}else{Err(Error::Unauthorized)};
            match resolved{
                Ok((fresh,url))=>shared.with(|inner,_|{
                    if inner.prepared_tag!=tag || inner.preparation_generation!=generation{return;}
                    inner.queue.replace_track(&fresh);inner.fresh.insert(fresh.id(),inner.network_epoch);
                    shared.remote().send(crate::audio::Command::Prepare{track_id:fresh.id(),url,duration:f64::from(fresh.duration),fetch:config});
                }),
                Err(error)=>log::debug!("next track preparation deferred: {}",error.kind()),
            }
        });
    }

    fn fetch_config(&self, inner: &Inner) -> FetchConfig {
        let mut config = self.fetch_template.clone();
        if let Some(session) = &inner.session {
            config.user_agent = session.api_user_agent.clone();
        }
        config.prefer_high_quality=inner.prefer_high_quality;
        config.offline_dir = self.offline.get().map(|o| o.dir().to_path_buf());
        config
    }

    /// Запускает текущую дорожку очереди или останавливает ее, если ее нет.
    fn start_current(self: &Arc<Self>, inner: &mut Inner, events: &mut Vec<UiEvent>) {
        inner.user_paused = false;
        // Все, что исцелялось, принадлежит тому, что играло раньше.
        self.cancel_recovery(inner, events);
        if let Some(radio) = inner.radio.as_mut() {
            radio.waiting = false;
        }
        self.maybe_refill(inner);
        let Some(track) = inner.queue.current().cloned() else {
            inner.status = Status::Idle;
            self.remote().stop();
            events.push(UiEvent::State(inner.state()));
            return;
        };
        let start = std::mem::take(&mut inner.pending_start).clamp(0.0, f64::from(track.duration));
        // Новое начало: прослушивание снова имеет значение с нуля.
        if let Some(history) = self.history.get() {
            history.restart();
        }
        inner.status = Status::Loading;
        inner.loading_since = Instant::now();
        inner.position = start;
        inner.duration = f64::from(track.duration);
        events.push(UiEvent::State(inner.state()));
        // Загруженная копия воспроизводится с диска. Если его файл исчез, проигрыватель терпит неудачу
        // и вместо этого при обычной повторной попытке извлекается дорожка из VK.
        let downloaded = if inner.refreshed.contains(&track.id()) {
            None
        } else {
            self.offline.get().and_then(|o| o.address(&track.id()))
        };
        if let Some(address) = downloaded {
            // Никаких учетных данных, никакой сети: ничего о VPNs его не касается.
            inner.source_offline = true;
            let config = self.fetch_config(inner);
            self.remote().play(&track.id(), &address, f64::from(track.duration), start, config);
            return;
        }
        inner.source_offline = false;
        self.watch_stall(inner, track.id());
        match track.stream_url.clone() {
            Some(url) if !self.is_stale(inner, &track) => {
                let config = self.fetch_config(inner);
                self.remote().play(&track.id(), &url, f64::from(track.duration), start, config);
            }
            // Адрес отсутствует или адрес был решен до изменения сети.
            _ => {
                inner.pending_start = start;
                self.refresh_then_play(inner, track)
            }
        }
    }

    /// URL, разрешенный в более раннюю сетевую эпоху, не является доверенным.
    fn is_stale(&self, inner: &Inner, track: &Track) -> bool {
        inner.session.is_some() && inner.network_epoch > 0 && inner.fresh.get(&track.id()) != Some(&inner.network_epoch)
    }

    fn policy(&self) -> RecoveryPolicy {
        *self.policy.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Запрашивает у VK новый поток URL в рабочем потоке, а затем воспроизводит его.
    fn refresh_then_play(self: &Arc<Self>, inner: &mut Inner, track: Track) {
        let Some(session) = inner.session.clone() else {
            // Без сеанса спрашивать не о чем; вести себя как неудачник.
            let shared = self.clone();
            std::thread::spawn(move || shared.fail_track(&track, Error::Unauthorized));
            return;
        };
        inner.refreshed.insert(track.id());
        let token = inner.recovery_token;
        let epoch = inner.network_epoch;
        let shared = self.clone();
        std::thread::spawn(move || {
            let mut result = shared.client.refreshed_track_once(&session, &track);
            // Токен с истекшим сроком действия обновляется из сохраненного файла cookie и не
            // рассматривается как выход из системы, как и собственные вызовы приложения.
            if result == Err(Error::Unauthorized) {
                if let Some(refresh) = shared.refresher.get() {
                    result = refresh(&session).and_then(|fresh| shared.client.refreshed_track_once(&fresh, &track));
                }
            }
            shared.with(|inner, events| {
                // Тот же трек можно перезапустить, приостановить или повторно разрешить, пока VK
                // отвечает. Поздний ответ никогда не должен возобновлять старое воспроизведение или
                // помечать адрес из старой сети как свежий.
                if inner.recovery_token != token || inner.session.is_none()
                    || inner.queue.current().map(Track::id) != Some(track.id()) {
                    return;
                }
                if inner.network_epoch != epoch {
                    shared.refresh_then_play(inner, track);
                    return;
                }
                match result {
                Ok(fresh) => {
                    // Возможно, пользователь ушел дальше, пока VK отвечал.
                    if inner.queue.current().map(Track::id) != Some(track.id()) {
                        return;
                    }
                    inner.queue.replace_track(&fresh);
                    inner.fresh.insert(fresh.id(), inner.network_epoch);
                    let config = shared.fetch_config(inner);
                    let start = std::mem::take(&mut inner.pending_start);
                    if let Some(url) = &fresh.stream_url {
                        shared.remote().play(&fresh.id(), url, f64::from(fresh.duration), start, config);
                    }
                    events.push(UiEvent::State(inner.state()));
                },
                Err(error) => {
                    let next = shared.clone();
                    std::thread::spawn(move || next.stream_failed_at(track, error, Some(token)));
                }
                }
            });
        });
    }

    /// Запускает радио-раунд в рабочем потоке, когда очередь заканчивается.
    fn maybe_refill(self: &Arc<Self>, inner: &mut Inner) {
        let upcoming = inner.queue.upcoming();
        let Some(radio) = inner.radio.as_mut() else { return };
        if !radio.wants_refill(upcoming) {
            return;
        }
        radio.in_flight = true;
        let generation = radio.generation;
        let shared = self.clone();
        std::thread::spawn(move || shared.refill(generation));
    }

    /// Один радиораунд: до нескольких запросов, пока не поступит что-то новое.
    fn refill(self: &Arc<Self>, generation: u64) {
        let mut added_total = 0;
        let mut failure = None;
        for attempt in 0..radio::ATTEMPTS {
            let step = self.with(|inner, _| {
                inner.radio.as_ref().filter(|r| r.generation == generation).map(|r| (r.next_request(), r.fetch.clone()))
            });
            let Some((request, fetch)) = step else { return };
            let Some(request) = request else { break };
            let proposed = match fetch(&request) {
                Ok(tracks) => tracks,
                Err(error) => {
                    log::debug!("radio refill: {}", error.kind());
                    failure = Some(error);
                    break;
                }
            };
            let added = self.with(|inner, events| {
                let Inner { queue, radio, .. } = inner;
                let radio = radio.as_mut().filter(|r| r.generation == generation)?;
                radio.answered(&request);
                let fresh = radio.fresh(proposed);
                let room = radio::UPCOMING_LIMIT.saturating_sub(queue.upcoming());
                let appended = queue.append_unique(self.taste.lock().unwrap_or_else(|p|p.into_inner()).filter(fresh), room);
                radio.record(&appended);
                if !appended.is_empty() {
                    events.push(inner.queue_event());
                    events.push(UiEvent::State(inner.state()));
                }
                Some(appended.len())
            });
            match added {
                None => return,
                Some(0) if attempt + 1 < radio::ATTEMPTS => std::thread::sleep(radio::ATTEMPT_PAUSE),
                Some(0) => {}
                Some(n) => {
                    added_total = n;
                    break;
                }
            }
        }
        self.with(|inner, events| {
            let Some(radio) = inner.radio.as_mut().filter(|r| r.generation == generation) else { return };
            radio.in_flight = false;
            radio.finish_round(added_total, failure.as_ref());
            let waiting = std::mem::take(&mut radio.waiting);
            if waiting {
                // Пока этот раунд был в пути, очередь иссякла.
                if added_total > 0 && inner.queue.advance(false).is_some() {
                    inner.failures = 0;
                    inner.refreshed.clear();
                    events.push(inner.queue_event());
                    self.start_current(inner, events);
                } else if inner.radio.as_ref().is_some_and(|r| !r.done) {
                    // One empty/error round does not exhaust the bounded retry budget.
                    if let Some(radio) = inner.radio.as_mut() { radio.waiting = true; }
                    self.maybe_refill(inner);
                    events.push(UiEvent::State(inner.state()));
                } else {
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    events.push(UiEvent::State(inner.state()));
                }
            } else if added_total > 0 {
                // После этой страницы все еще мало: попросите следующую.
                self.maybe_refill(inner);
            }
            if !waiting { events.push(UiEvent::State(inner.state())); }
        });
    }

    /// Трек не может воспроизводиться: сообщите об этом и двигайтесь дальше, в пределах ограничений.
    fn fail_track(self: &Arc<Self>, track: &Track, error: Error) {
        self.with(|inner, events| self.fail_track_in(inner, events, track, error));
    }

    fn fail_track_in(self: &Arc<Self>, inner: &mut Inner, events: &mut Vec<UiEvent>, track: &Track, error: Error) {
        if inner.queue.current().map(Track::id) != Some(track.id()) {
            return;
        }
        if error == Error::Unauthorized {
            events.push(UiEvent::SessionExpired);
        }
        events.push(UiEvent::Error { kind: error.kind().to_string(), message: error.to_string() });
        let skippable = matches!(error, Error::Stream(_) | Error::Http(_) | Error::Format(_) | Error::Vk { .. } | Error::Unavailable);
        inner.failures += 1;
        if inner.advance_on_error && skippable && inner.failures < MAX_CONSECUTIVE_FAILURES && inner.queue.advance(false).is_some() {
            inner.refreshed.clear();
            events.push(inner.queue_event());
            self.start_current(inner, events);
        } else {
            self.cancel_recovery(inner, events);
            inner.status = Status::Idle;
            self.remote().stop();
            events.push(UiEvent::State(inner.state()));
        }
    }

    /// Забывает об устранении сбоя (начался новый трек, пользователь остановился и т. д.) и о любых
    /// попытках, все еще ожидающих отмены.
    fn cancel_recovery(&self, inner: &mut Inner, events: &mut Vec<UiEvent>) {
        self.cancel_recovery_wait();
        self.cancel_stall_wait();
        inner.recovery_token += 1;
        inner.stall_token += 1;
        inner.gave_up = false;
        if let Some(recovery) = inner.recovery.take() {
            if recovery.probation_from.is_some() {
                Self::announce_recovered(&recovery, events);
            }
        }
    }

    fn announce_recovered(recovery: &Recovery, events: &mut Vec<UiEvent>) {
        log::info!("recovered after {:.1} s ({} attempt{})", recovery.started.elapsed().as_secs_f64(), recovery.attempts, if recovery.attempts == 1 { "" } else { "s" });
        events.push(UiEvent::Recovery {
            phase: RecoveryPhase::Recovered,
            attempt: recovery.attempts,
            max: recovery.max,
            kind: recovery.kind.to_string(),
            retry_in_secs: None,
        });
    }

    /// Не удалось получить, декодировать или разрешить поток. Принимает решение между его исцелением
    /// (свежие URL, новые соединения, откат), пропуском мертвого трека и паузой с повторной попыткой
    /// после достижения границы.
    fn stream_failed(self: &Arc<Self>, track: Track, error: Error) {
        self.stream_failed_at(track, error, None);
    }

    fn stream_failed_at(self: &Arc<Self>, track: Track, error: Error, token: Option<u64>) {
        let policy = self.policy();
        self.with(|inner, events| {
            if token.is_some_and(|token| inner.recovery_token != token)
                || inner.queue.current().map(Track::id) != Some(track.id()) {
                return;
            }
            if inner.user_paused {
                inner.pending_start = inner.position;
                inner.status = Status::Idle;
                self.remote().stop();
                events.push(UiEvent::State(inner.state()));
                return;
            }
            // Удалите застрявший decoder/download, прежде чем запрашивать еще один URL. Поздняя неудача
            // не должна расходовать бюджет повторных попыток новой попытки.
            self.remote().stop();
            // Ожидание, запрошенное сервером, не является зависшим декодером.
            self.cancel_stall_wait();
            inner.stall_token += 1;
            // Загруженная копия, которую невозможно прочитать: получите ее один раз с VK.
            if std::mem::take(&mut inner.source_offline) {
                if inner.session.is_some() && !inner.refreshed.contains(&track.id()) && matches!(error, Error::Stream(_) | Error::Http(_)) {
                    inner.pending_start = inner.position;
                    self.refresh_then_play(inner, track);
                } else {
                    self.fail_track_in(inner, events, &track, error);
                }
                return;
            }
            let class = classify(&error);
            if inner.session.is_none() || class == Class::Fatal {
                self.fail_track_in(inner, events, &track, error);
                return;
            }
            let connectivity = class == Class::Connectivity;
            if net::is_connectivity(&error) {
                // Какие бы сокеты ни были объединены в пул, они принадлежат маршруту, который потерпел
                // неудачу.
                net::drop_connections();
            }
            let max = policy.attempts(connectivity);
            let kind = if net::is_connectivity(&error) { "network" }
                else if matches!(error, Error::Http(408 | 429 | 500..=599) | Error::RetryAfter { .. } | Error::Vk { code:6 | 9 | 10, .. }) { "service" }
                else if connectivity { "device" } else { "stream" };
            let id = track.id();
            let recovery = match inner.recovery.take() {
                Some(r) if r.track_id == id => r,
                _ => Recovery { track_id: id, attempts: 0, max, kind, started: Instant::now(), probation_from: None },
            };
            let recovery = inner.recovery.insert(Recovery { attempts: recovery.attempts + 1, max, kind, probation_from: None, ..recovery });
            let (attempt, started) = (recovery.attempts, recovery.started);
            if attempt > max {
                inner.recovery = None;
                inner.recovery_token += 1;
                if !connectivity {
                    // Этот адрес не будет работать, как бы часто его ни просили: мертвый трек,
                    // пропущенный в обычных пределах.
                    log::warn!("stream {}: still failing after {max} fresh URLs, giving up on this track", what(&error));
                    self.fail_track_in(inner, events, &track, error);
                    return;
                }
                log::warn!("stream {}: still failing after {max} attempts ({:.1} s), pausing", what(&error), started.elapsed().as_secs_f64());
                inner.gave_up = true;
                inner.pending_start = inner.position;
                inner.status = Status::Idle;
                self.remote().stop();
                events.push(UiEvent::Error { kind: error.kind().to_string(), message: error.to_string() });
                // Только реальный сбой соединения указывает на сеть (и VPN); сервис, который продолжает
                // отвечать на 5хх - другое дело.
                events.push(UiEvent::Recovery { phase: RecoveryPhase::Failed, attempt: max, max, kind: kind.into(), retry_in_secs: None });
                events.push(UiEvent::State(inner.state()));
                return;
            }
            let announced = match error { Error::RetryAfter { seconds, .. } => Duration::from_secs(seconds), _ => Duration::ZERO };
            let delay = policy.delay(connectivity, attempt, net::jitter_unit()).max(self.client.cooldown_remaining()).max(announced);
            log::info!("stream {} → refreshing url (attempt {attempt}/{max}, in {:.1} s)", what(&error), delay.as_secs_f64());
            events.push(UiEvent::Recovery { phase: RecoveryPhase::Retrying, attempt, max, kind: kind.into(), retry_in_secs:Some(delay.as_secs_f64().ceil() as u64) });
            inner.status = Status::Loading;
            inner.loading_since = Instant::now();
            events.push(UiEvent::State(inner.state()));
            self.schedule_attempt(inner, track.id(), delay);
        });
    }

    /// Запускает следующую попытку восстановления для `track_id` после `delay`, если она не отменена
    /// (новый трек, остановка, смена сети, заменившая ее).
    fn schedule_attempt(self: &Arc<Self>, inner: &mut Inner, track_id: String, delay: Duration) {
        self.cancel_recovery_wait();
        let wake = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        *self.recovery_cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(wake.clone());
        inner.recovery_token += 1;
        let token = inner.recovery_token;
        let shared = self.clone();
        std::thread::spawn(move || {
            let cancelled = wake.1.wait_timeout_while(wake.0.lock().unwrap_or_else(|p| p.into_inner()), delay, |cancelled| !*cancelled)
                .unwrap_or_else(|p| p.into_inner());
            if *cancelled.0 { return; }
            drop(cancelled);
            shared.with(|inner, events| {
                if inner.recovery_token != token || inner.session.is_none() {
                    return;
                }
                let Some(track) = inner.queue.current().cloned().filter(|t| t.id() == track_id) else { return };
                // Продолжайте там, где сломались, а не с самого начала.
                inner.pending_start = inner.position;
                inner.status = Status::Loading;
                inner.loading_since = Instant::now();
                events.push(UiEvent::State(inner.state()));
                shared.refresh_then_play(inner, track);
                shared.watch_stall(inner, track_id.clone());
            });
        });
    }

    fn cancel_recovery_wait(&self) {
        if let Some(cancel) = self.recovery_cancel.lock().unwrap_or_else(|p| p.into_inner()).take() {
            *cancel.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
            cancel.1.notify_all();
        }
    }

    fn cancel_stall_wait(&self) {
        if let Some(cancel) = self.stall_cancel.lock().unwrap_or_else(|p| p.into_inner()).take() {
            *cancel.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
            cancel.1.notify_all();
        }
    }

    /// Звук иссяк и остался сухим: относитесь к нему как к неудачному стриму.
    fn watch_stall(self: &Arc<Self>, inner: &mut Inner, track_id: String) {
        self.cancel_stall_wait();
        let wake = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        *self.stall_cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(wake.clone());
        inner.stall_token += 1;
        let token = inner.stall_token;
        let policy = self.policy();
        let after = policy.scaled(policy.stall_after.as_secs_f64());
        let shared = self.clone();
        std::thread::spawn(move || {
            let cancelled = wake.1.wait_timeout_while(wake.0.lock().unwrap_or_else(|p| p.into_inner()), after, |cancelled| !*cancelled)
                .unwrap_or_else(|p| p.into_inner());
            if *cancelled.0 { return; }
            drop(cancelled);
            let stalled = shared.with(|inner, _| {
                (inner.stall_token == token && matches!(inner.status, Status::Buffering | Status::Loading) && !inner.source_offline)
                    .then(|| inner.queue.current().cloned().filter(|t| t.id() == track_id))
                    .flatten().map(|track| (track, inner.recovery_token))
            });
            if let Some((track, recovery_token)) = stalled {
                log::warn!("playback stalled for {:.0} s, treating it as a failed stream", after.as_secs_f64());
                shared.stream_failed_at(track, Error::Timeout, Some(recovery_token));
            }
        });
    }

    fn on_network(self: &Arc<Self>, state: NetState) {
        match state {
            NetState::Changed => self.network_changed(),
            NetState::Ok => self.network_settled(),
            NetState::Unreachable => {}
        }
    }

    /// Каждый разрешенный на данный момент поток URL принадлежит старой сети: для каждого удаленного
    /// трека запрашивается новый перед воспроизведением следующего. Все, что воспроизводится из
    /// загруженного файла, остается нетронутым.
    fn network_changed(self:&Arc<Self>) {
        self.with(|inner, _| {
            inner.network_epoch += 1;
            let queued = inner.queue.len();
            log::info!("network changed: stream URLs of {queued} queued track(s) are stale, fresh ones are asked for before they play");
        });
    }

    /// Сеть снова стабильна: поток, который ждал отката, застопорился, и воспроизведение, которое
    /// сдалось, началось сразу же (после небольшой случайной паузы, чтобы ничего не тормозило).
    fn network_settled(self: &Arc<Self>) {
        let shared = self;
        let policy = self.policy();
        self.with(|inner, events| {
            let Some(track) = inner.queue.current().cloned() else { return };
            if inner.source_offline || inner.session.is_none() || inner.user_paused {
                return;
            }
            let waiting = inner.recovery.is_some() || inner.gave_up;
            let stalled = inner.status == Status::Buffering
                || (inner.status == Status::Loading && inner.loading_since.elapsed() > policy.scaled(3.0));
            if !waiting && !stalled {
                return;
            }
            log::info!("network is back: restarting {}", if inner.gave_up { "the paused playback" } else { "the stream" });
            if inner.recovery.is_none() {
                inner.recovery = Some(Recovery {
                    track_id: track.id(),
                    attempts: 0,
                    max: policy.connectivity_attempts,
                    kind: "network",
                    started: Instant::now(),
                    probation_from: None,
                });
            }
            inner.gave_up = false;
            inner.pending_start = inner.position;
            self.remote().stop();
            inner.status = Status::Loading;
            inner.loading_since = Instant::now();
            events.push(UiEvent::State(inner.state()));
            let delay = policy.network_return_delay(net::jitter_unit());
            shared.schedule_attempt(inner, track.id(), delay);
        });
    }

    fn on_player_event(self: &Arc<Self>, event: PlayerEvent) {
        match event {
            PlayerEvent::Transition{from,to}=>self.with(|inner,events|{
                if inner.queue.current().map(Track::id).as_deref()!=Some(from.as_str()){return;}
                if inner.queue.peek_next(true).map(Track::id).as_deref()!=Some(to.as_str()){self.remote().stop();self.start_current(inner,events);return;}
                inner.queue.advance(true);inner.position=0.0;inner.pending_start=0.0;inner.duration=inner.queue.current().map(|track|f64::from(track.duration)).unwrap_or_default();inner.status=Status::Playing;
                inner.prepared_tag=None;
                inner.source_offline=inner.queue.current().is_some_and(|track|self.offline.get().and_then(|offline|offline.address(&track.id())).is_some());
                inner.failures=0;
                if let Some(history)=self.history.get(){history.restart();}
                self.cancel_recovery(inner,events);self.maybe_refill(inner);
                events.push(inner.queue_event());events.push(UiEvent::State(inner.state()));
            }),
            PlayerEvent::Position { track_id, seconds, duration } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.position = seconds;
                    let probation = self.policy().probation_secs;
                    if inner.recovery.as_ref().and_then(|r| r.probation_from).is_some_and(|from| seconds >= from + probation) {
                        if let Some(recovery) = inner.recovery.take() {
                            Self::announce_recovered(&recovery, events);
                        }
                    }
                    if duration > 0.0 {
                        inner.duration = duration;
                    }
                    if let (Some(history), Some(track)) = (self.history.get(), inner.queue.current()) {
                        history.tick(track, seconds, inner.duration, inner.status == Status::Playing);
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Spectrum { bands } => {
                let bands = bands.iter().map(|b| (b * 1000.0).round() / 1000.0).collect();
                (self.emit)(UiEvent::Spectrum { bands });
            }
            PlayerEvent::Loading { .. } => {}
            PlayerEvent::Playing { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.status = Status::Playing;
                    inner.failures = 0;
                    inner.refreshed.remove(&track_id);
                    let position = inner.position;
                    if let Some(recovery) = inner.recovery.as_mut().filter(|r| r.track_id == track_id) {
                        // Звук снова течет; он считается восстановленным через несколько секунд.
                        recovery.probation_from = Some(position);
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Paused { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str()) {
                    inner.status = Status::Paused;
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Buffering { track_id, on } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() == Some(track_id.as_str())
                    && matches!(inner.status, Status::Playing | Status::Buffering)
                {
                    inner.status = if on { Status::Buffering } else { Status::Playing };
                    if on && !inner.source_offline {
                        self.watch_stall(inner, track_id.clone());
                    } else {
                        self.cancel_stall_wait();
                        inner.stall_token += 1;
                    }
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Stopped => {}
            PlayerEvent::Ended { track_id } => self.with(|inner, events| {
                if inner.queue.current().map(Track::id).as_deref() != Some(track_id.as_str()) {
                    return;
                }
                if std::mem::take(&mut inner.sleep_end) {
                    inner.status = Status::Idle;
                    inner.pending_start = 0.0;
                    self.cancel_recovery(inner, events);
                    self.remote().stop();
                    events.push(UiEvent::SleepTimer { remaining: None, end_of_track: false });
                    events.push(UiEvent::State(inner.state()));
                    return;
                }
                if inner.queue.advance(true).is_some() {
                    events.push(inner.queue_event());
                    self.start_current(inner, events);
                } else if inner.radio.as_ref().is_some_and(|r| !r.done) {
                    // Бесконечная очередь никогда не заканчивается: ждите следующей страницы.
                    if let Some(radio) = inner.radio.as_mut() {
                        radio.waiting = true;
                    }
                    inner.status = Status::Loading;
                    inner.position = 0.0;
                    events.push(UiEvent::State(inner.state()));
                    self.maybe_refill(inner);
                } else {
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    inner.pending_start = 0.0;
                    events.push(UiEvent::State(inner.state()));
                }
            }),
            PlayerEvent::Failed { track_id, error } => {
                let current = {
                    let inner = self.lock();
                    inner.queue.current().cloned().filter(|t| t.id() == track_id)
                };
                if let Some(track) = current {
                    self.stream_failed(track, error);
                }
            }
        }
    }
}

impl Controller {
    /// `make_sink` создает устройство вывода в аудиопотоке; `emit` получает каждый [`UiEvent`] (он не
    /// должен блокироваться).
    pub fn new<F>(
        client: VkClient,
        make_sink: F,
        emit: impl Fn(UiEvent) + Send + Sync + 'static,
        volume: f32,
    ) -> Self
    where
        F: FnMut() -> crate::Result<Box<dyn Sink>> + Send + 'static,
    {
        Self::with_fetch(client, make_sink, emit, volume, FetchConfig::new("PrivateMusicDesktop"))
    }

    /// Как [`Controller::new`] с явной конфигурацией потоковой выборки (тесты позволяют использовать
    /// сервер обратной связи таким образом).
    pub fn with_fetch<F>(
        client: VkClient,
        make_sink: F,
        emit: impl Fn(UiEvent) + Send + Sync + 'static,
        volume: f32,
        fetch_template: FetchConfig,
    ) -> Self
    where
        F: FnMut() -> crate::Result<Box<dyn Sink>> + Send + 'static,
    {
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                prefetch:true,
                advance_on_error:true,
                prefer_high_quality:true,
                prepared_tag:None,
                preparation_generation:0,
                sleep_end: false,
                user_paused: false,
                queue: Queue::default(),
                session: None,
                status: Status::Idle,
                position: 0.0,
                duration: 0.0,
                volume: volume.clamp(0.0, 1.0),
                rng: StdRng::from_entropy(),
                failures: 0,
                refreshed: HashSet::new(),
                pending_start: 0.0,
                radio: None,
                radio_generation: 0,
                recovery: None,
                recovery_token: 0,
                gave_up: false,
                stall_token: 0,
                network_epoch: 0,
                fresh: HashMap::new(),
                source_offline: false,
                loading_since: Instant::now(),
            }),
            taste: Mutex::new(crate::taste::Taste { novelty:50,..Default::default() }),
            client,
            remote: OnceLock::new(),
            emit: Box::new(emit),
            fetch_template,
            refresher: OnceLock::new(),
            offline: OnceLock::new(),
            history: OnceLock::new(),
            policy: Mutex::new(RecoveryPolicy::default()),
            sleep_cancel: Mutex::new(None),
            recovery_cancel: Mutex::new(None),
            stall_cancel: Mutex::new(None),
            sleep_deadline: Mutex::new(None),
        });
        let callback_shared = shared.clone();
        let player = PlayerHandle::spawn(make_sink, move |event| callback_shared.on_player_event(event));
        let _ = shared.remote.set(player.remote());
        player.set_volume(volume.clamp(0.0, 1.0));
        Self { shared, _player: player }
    }

    pub fn set_session(&self, session: Option<Session>) {
        if session.is_none() { let _ = self.set_sleep_timer(None, false); }
        self.shared.with(|inner, events| {
            let signed_out = session.is_none();
            inner.session = session;
            if signed_out {
                *self.shared.taste.lock().unwrap_or_else(|p|p.into_inner())=crate::taste::Taste{novelty:50,..Default::default()};
                self.shared.cancel_recovery(inner, events);
                inner.radio = None;
                inner.queue.clear();
                inner.status = Status::Idle;
                self.shared.remote().stop();
                events.push(inner.queue_event());
                events.push(UiEvent::State(inner.state()));
            }
        });
    }

    /// Как контроллер обновляет токен с истекшим сроком действия, когда он запрашивает у VK новый адрес
    /// потока. Устанавливается один раз с помощью приложения.
    pub fn set_refresher(&self, refresher: Refresher) {
        let _ = self.shared.refresher.set(refresher);
    }

    /// Изменяет способ восстановления поврежденных потоков (тесты сокращают время ожидания).
    pub fn set_recovery_policy(&self, policy: RecoveryPolicy) {
        *self.shared.policy.lock().unwrap_or_else(|p| p.into_inner()) = policy;
    }

    /// Маршрут к Интернету изменился, стабилизировался или исчез (см. [`net`]).
    pub fn on_network(&self, state: NetState) {
        self.shared.on_network(state);
    }

    /// То же, что [`on_network`](Self::on_network) для потока, пережившего это заимствование (сетевой
    /// монитор).
    pub fn network_listener(&self) -> impl Fn(NetState) + Send + Sync + 'static {
        let shared = self.shared.clone();
        move |state| shared.on_network(state)
    }

    /// Пользовательский "try again" для неудачного воспроизведения: свежие соединения, свежий URL, та
    /// же позиция, полный набор попыток.
    pub fn retry(&self) {
        self.shared.with(|inner, events| {
            if inner.queue.current().is_none() || inner.source_offline {
                return;
            }
            let position = inner.position;
            net::drop_connections();
            inner.network_epoch += 1;
            inner.failures = 0;
            inner.refreshed.clear();
            self.shared.cancel_recovery(inner, events);
            inner.pending_start = position;
            self.shared.start_current(inner, events);
        });
    }

    /// Откуда воспроизводятся скачанные треки. Устанавливается один раз с помощью приложения.
    pub fn set_offline(&self, store: Arc<OfflineStore>) {
        let _ = self.shared.offline.set(store);
    }

    /// Где записываются игры для истории и статистики. Устанавливается один раз с помощью приложения.
    pub fn set_history(&self, history: Arc<History>) {
        let _ = self.shared.history.set(history);
    }

    /// При воспроизведении используются настройки загрузки (пользовательский агент, ограничение
    /// размера).
    pub fn fetch_config(&self) -> FetchConfig {
        let inner = self.shared.lock();
        self.shared.fetch_config(&inner)
    }

    pub fn session(&self) -> Option<Session> {
        self.shared.lock().session.clone()
    }

    pub fn state(&self) -> State {
        self.shared.lock().state()
    }

    pub fn play_tracks(&self, tracks: Vec<Track>, index: usize) {
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.replace(tracks, index, rng);
            inner.radio = None;
            inner.failures = 0;
            inner.refreshed.clear();
            events.push(inner.queue_event());
            self.shared.start_current(inner, events);
        });
    }

    /// Воспроизводит очередь, которая продолжает пополняться из своего источника (см. [`radio`]).
    /// `offset` — это место, где продолжается микс потока, `seeds` — треки, о которых начинает
    /// спрашивать радиостанция; `tracks` считайте, как уже слышали.
    pub fn play_radio(&self, tracks: Vec<Track>, index: usize, kind: RadioKind, fetch: RadioFetch, offset: u32, seeds: Vec<Track>) {
        self.radio_queue(tracks, index, RadioSetup { kind, fetch, offset, seeds, source: None }, None);
    }

    /// `position` restores a paused queue; no stream starts before the caller resumes.
    pub fn radio_queue(&self, tracks: Vec<Track>, index: usize, setup: RadioSetup, position: Option<f64>) {
        let selected=tracks.get(index).map(Track::id);
        let tracks=self.recommendations(tracks);
        let index=tracks.iter().position(|track|Some(track.id())==selected).unwrap_or_default();
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.replace(tracks, index, rng);
            inner.radio_generation += 1;
            let mut radio = Radio::new(setup.kind, inner.radio_generation, setup.fetch, setup.offset, setup.seeds, inner.queue.tracks());
            radio.source = setup.source;
            inner.radio = Some(radio);
            inner.failures = 0;
            inner.refreshed.clear();
            events.push(inner.queue_event());
            if let Some(position) = position {
                self.shared.remote().stop();
                self.shared.cancel_recovery(inner, events);
                inner.status = Status::Idle;
                inner.pending_start = if inner.queue.current().map(Track::id) == selected { position } else { 0.0 };
                inner.position = inner.pending_start;
                inner.duration = inner.queue.current().map(|track| f64::from(track.duration)).unwrap_or_default();
                events.push(UiEvent::State(inner.state()));
            } else {
                self.shared.start_current(inner, events);
            }
        });
    }

    /// Ставит очередь на место без ее воспроизведения (восстановленный сеанс). Следующая игра начнется
    /// через `position` секунд.
    pub fn load_queue(&self, tracks: Vec<Track>, index: usize, position: f64) {
        self.shared.with(|inner, events| {
            self.shared.remote().stop();
            self.shared.cancel_recovery(inner,events);
            let Inner { queue, rng, .. } = inner;
            queue.replace(tracks, index, rng);
            inner.radio = None;
            inner.status = Status::Idle;
            inner.pending_start = if position.is_finite() { position.max(0.0) } else { 0.0 };
            inner.position = inner.pending_start;
            inner.duration = inner.queue.current().map(|t| f64::from(t.duration)).unwrap_or(0.0);
            events.push(inner.queue_event());
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn pause(&self) {
        self.shared.with(|inner, events| {
            inner.user_paused = true;
            if let Some(radio) = inner.radio.as_mut() { radio.waiting = false; }
            if inner.status == Status::Loading {
                // Пауза во время исцеления потока останавливает исцеление; Игра начинается снова
                // отсюда.
                let position = inner.position;
                self.shared.cancel_recovery(inner, events);
                inner.pending_start = position;
                inner.status = Status::Idle;
                self.shared.remote().stop();
                events.push(UiEvent::State(inner.state()));
            } else {
                self.shared.cancel_recovery(inner, events);
                self.shared.remote().pause();
            }
        });
    }

    pub fn resume(&self) {
        self.shared.with(|inner, _| inner.user_paused = false);
        self.shared.remote().resume();
    }

    pub fn toggle(&self) {
        let status = self.shared.lock().status;
        match status {
            Status::Playing | Status::Buffering => self.pause(),
            Status::Paused => self.resume(),
            Status::Idle => {
                // Запускайте все, что стоит в очереди, с начала дорожки.
                self.shared.with(|inner, events| {
                    if inner.queue.current().is_some() {
                        self.shared.start_current(inner, events);
                    }
                });
            }
            Status::Loading => self.pause(),
        }
    }

    pub fn seek(&self, seconds: f64) {
        if !seconds.is_finite() {
            return;
        }
        self.shared.remote().seek(seconds);
        self.shared.with(|inner, events| {
            inner.prepared_tag = None;
            if inner.queue.current().is_none() {
                return;
            }
            let target = seconds.max(0.0);
            inner.position = if inner.duration > 0.0 { target.min(inner.duration) } else { target };
            if inner.status == Status::Idle {
                // Ничего не играет: здесь начинается следующая игра.
                inner.pending_start = inner.position;
            }
            events.push(UiEvent::State(inner.state()));
        });
    }

    /// Переключается на другой вывод звука (`make_sink` оболочки решает, какой): устройство снова
    /// открывается, и воспроизведение продолжается с того места, где оно было.
    pub fn reopen_output(&self) {
        self.shared.with(|inner, events| {
            self.shared.remote().reopen_sink();
            let had_track = inner.queue.current().is_some();
            match inner.status {
                Status::Playing | Status::Buffering | Status::Loading if had_track => {
                    inner.pending_start = inner.position;
                    self.shared.start_current(inner, events);
                }
                _ => {
                    // Пауза или режим ожидания: следующее воспроизведение начинается с этого места на
                    // новом выходе.
                    if had_track {
                        inner.pending_start = inner.position;
                    }
                    inner.status = Status::Idle;
                    events.push(UiEvent::State(inner.state()));
                }
            }
        });
    }

    /// Усиления эквалайзера в dB (см. `audio::eq::BANDS`); `None` выключает его.
    pub fn set_audio_processing(&self, settings: crate::audio::processing::AudioSettings) { self.shared.remote().set_audio_processing(settings); }

    pub fn set_equalizer(&self, gains: Option<[f32; 10]>) {
        self.shared.remote().set_equalizer(gains);
    }

    pub fn set_taste(&self,taste:crate::taste::Taste)->crate::Result<()> {
        taste.validate()?;
        *self.shared.taste.lock().unwrap_or_else(|p|p.into_inner())=taste.clone();
        self.shared.with(|inner,events|{
            if inner.radio.is_some(){let allowed:HashSet<_>=taste.filter(inner.queue.tracks().to_vec()).iter().map(Track::id).collect();inner.queue.retain_upcoming(|track|allowed.contains(&track.id()));events.push(inner.queue_event());events.push(UiEvent::State(inner.state()));}
        });
        Ok(())
    }
    pub fn recommendations(&self,tracks:Vec<Track>)->Vec<Track>{self.shared.taste.lock().unwrap_or_else(|p|p.into_inner()).filter(tracks)}

    pub fn set_transitions(&self,prefetch:bool,gapless:bool,crossfade:f64){
        self.shared.remote().send(crate::audio::Command::SetTransitions{gapless,crossfade});
        self.shared.with(|inner,_|{inner.prefetch=prefetch;});
    }
    /// Obtain a buffer handle only; byte copies and file writes happen on the caller's worker.
    pub fn track_buffer(&self,track_id:&str)->Option<crate::stream::SharedBuffer> {self.shared.remote().track_buffer(track_id)}
    pub fn set_high_quality(&self,on:bool) {self.shared.with(|inner,_|{if inner.prefer_high_quality!=on {inner.prefer_high_quality=on;inner.prepared_tag=None;}});}
    pub fn set_advance_on_error(&self, on: bool) { self.shared.with(|inner, _| inner.advance_on_error = on); }
    pub fn set_normalization(&self,on:bool){
        self.shared.remote().send(crate::audio::Command::SetNormalization(on));
        self.shared.with(|inner,_|{inner.prepared_tag=None;});
    }
    pub fn set_analysis_directory(&self,path:Option<std::path::PathBuf>){self.shared.remote().send(crate::audio::Command::SetAnalysisDirectory(path));}

    /// Запускает или останавливает события спектра. По умолчанию отключено: анализ звука требует CPU,
    /// который необходим только для полноэкранного просмотра.
    pub fn set_visualizer(&self, on: bool) {
        self.shared.remote().set_visualizer(on);
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        self.shared.with(|inner, events| {
            inner.volume = volume;
            events.push(UiEvent::State(inner.state()));
        });
        self.shared.remote().set_volume(volume);
    }

    pub fn next(&self) {
        self.shared.with(|inner, events| {
            log::info!("next requested: queue_index={:?} queue_len={} available={} shuffle={} repeat={:?}",
                inner.queue.current_index(), inner.queue.len(), inner.queue.peek_next(false).is_some(), inner.queue.shuffle(), inner.queue.repeat());
            if inner.queue.advance(false).is_some() {
                inner.failures = 0;
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.shared.start_current(inner, events);
            } else if inner.queue.current().is_some() && inner.radio.as_ref().is_some_and(|r| !r.done) {
                // Manual Next at the live tail must wait for the same radio source.
                // Repeated taps share its existing request; no parallel refills.
                if let Some(radio) = inner.radio.as_mut() { radio.waiting = true; }
                inner.user_paused = false;
                self.shared.cancel_recovery(inner, events);
                inner.status = Status::Loading;
                inner.position = 0.0;
                inner.pending_start = 0.0;
                self.shared.remote().stop();
                self.shared.maybe_refill(inner);
                events.push(UiEvent::State(inner.state()));
            }
        });
    }

    pub fn previous(&self) { self.previous_with_restart(true); }

    pub fn previous_track(&self) { self.previous_with_restart(false); }

    fn previous_with_restart(&self, allow_restart: bool) {
        let restart = allow_restart && {
            let inner = self.shared.lock();
            inner.queue.current().is_some() && inner.position > RESTART_THRESHOLD
        };
        if restart {
            self.seek(0.0);
            return;
        }
        self.shared.with(|inner, events| {
            if inner.queue.previous().is_some() {
                inner.failures = 0;
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.shared.start_current(inner, events);
            }
        });
    }

    /// Воспроизводит трек с позиции `index` в указанной очереди.
    pub fn jump(&self, index: usize) {
        self.shared.with(|inner, events| {
            if inner.queue.jump(index).is_some() {
                inner.failures = 0;
                inner.refreshed.clear();
                events.push(inner.queue_event());
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn clear_upcoming(&self) {
        self.shared.with(|inner, events| {
            inner.radio = None;
            inner.radio_generation += 1;
            inner.queue.clear_upcoming();
            events.push(inner.queue_event());
            events.push(UiEvent::State(inner.state()));
        });
    }

    /// Таймер остается в движке, даже если вид игрока закрыт. Его замена немедленно пробуждает старого
    /// официанта вместо утечки потоков.
    pub fn set_sleep_timer(&self, seconds: Option<u64>, end_of_track: bool) -> crate::Result<()> {
        if seconds.is_some_and(|n| n == 0 || n > 86_400) || (seconds.is_some() && end_of_track) {
            return Err(Error::InvalidRequest);
        }
        let mut cancel = self.shared.sleep_cancel.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = cancel.take() {
            *old.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
            old.1.notify_all();
        }
        *self.shared.sleep_deadline.lock().unwrap_or_else(|p| p.into_inner()) = seconds.map(|n| Instant::now() + Duration::from_secs(n));
        self.shared.with(|inner, events| {
            inner.sleep_end = end_of_track;
            events.push(UiEvent::SleepTimer { remaining: seconds, end_of_track });
        });
        if let Some(seconds) = seconds {
            let wake = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            *cancel = Some(wake.clone());
            let weak = Arc::downgrade(&self.shared);
            std::thread::spawn(move || {
                let cancelled = wake.0.lock().unwrap_or_else(|p| p.into_inner());
                let (cancelled, _) = wake.1.wait_timeout_while(cancelled, Duration::from_secs(seconds), |v| !*v).unwrap_or_else(|p| p.into_inner());
                if *cancelled { return; }
                drop(cancelled);
                let Some(shared) = weak.upgrade() else { return };
                // Сериализуйте истечение срока действия с помощью cancellation/replacement таймера.
                let current = shared.sleep_cancel.lock().unwrap_or_else(|p| p.into_inner());
                if !current.as_ref().is_some_and(|v| Arc::ptr_eq(v, &wake)) { return; }
                *shared.sleep_deadline.lock().unwrap_or_else(|p| p.into_inner()) = None;
                shared.with(|inner, events| {
                    inner.user_paused = true;
                    shared.cancel_recovery(inner, events);
                    if inner.status == Status::Loading {
                        inner.pending_start = inner.position;
                        inner.status = Status::Idle;
                        shared.remote().stop();
                        events.push(UiEvent::State(inner.state()));
                    } else {
                        shared.remote().pause();
                    }
                    events.push(UiEvent::SleepTimer { remaining: None, end_of_track: false });
                });
            });
        }
        Ok(())
    }

    pub fn sleep_timer(&self) -> (Option<u64>, bool) {
        let remaining = self.shared.sleep_deadline.lock().unwrap_or_else(|p| p.into_inner()).map(|v| v.saturating_duration_since(Instant::now()).as_secs());
        (remaining, self.shared.lock().sleep_end)
    }

    pub fn move_upcoming(&self, from: usize, to: usize) {
        self.shared.with(|inner, events| {
            if inner.queue.move_upcoming(from, to) {
                events.push(inner.queue_event());
                events.push(UiEvent::State(inner.state()));
            }
        });
    }

    pub fn set_shuffle(&self, on: bool) {
        self.shared.with(|inner, events| {
            let Inner { queue, rng, .. } = inner;
            queue.set_shuffle(on, rng);
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        self.shared.with(|inner, events| {
            inner.queue.set_repeat(repeat);
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn play_next(&self, track: Track) {
        self.shared.with(|inner, events| {
            let was_empty = inner.queue.is_empty();
            inner.queue.play_next(track);
            events.push(inner.queue_event());
            if was_empty {
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn play_last(&self, track: Track) {
        self.shared.with(|inner, events| {
            let was_empty = inner.queue.is_empty();
            inner.queue.play_last(track);
            events.push(inner.queue_event());
            if was_empty {
                self.shared.start_current(inner, events);
            }
        });
    }

    pub fn remove_from_queue(&self, index: usize) {
        self.shared.with(|inner, events| {
            let was_last = inner.queue.at_last();
            let was_current = inner.queue.remove(index);
            if inner.queue.is_empty() {
                inner.radio = None;
            }
            if was_current && was_last && !inner.queue.is_empty() {
                // Удаление последнего трека очереди во время его воспроизведения: очередь окончена,
                // если она не повторяется. Никогда не переигрывайте предыдущее.
                if inner.queue.repeat() == Repeat::All {
                    inner.queue.rewind();
                } else {
                    events.push(inner.queue_event());
                    inner.status = Status::Idle;
                    inner.position = 0.0;
                    self.shared.remote().stop();
                    events.push(UiEvent::State(inner.state()));
                    return;
                }
            }
            events.push(inner.queue_event());
            if was_current {
                if inner.queue.is_empty() {
                    inner.status = Status::Idle;
                    self.shared.remote().stop();
                    events.push(UiEvent::State(inner.state()));
                } else {
                    self.shared.start_current(inner, events);
                }
            } else {
                events.push(UiEvent::State(inner.state()));
                self.shared.maybe_refill(inner);
            }
        });
    }

    pub fn stop(&self) {
        self.shared.with(|inner, events| {
            self.shared.cancel_recovery(inner, events);
            if let Some(radio) = inner.radio.as_mut() {
                radio.waiting = false;
            }
            inner.status = Status::Idle;
            inner.position = 0.0;
            self.shared.remote().stop();
            events.push(UiEvent::State(inner.state()));
        });
    }

    pub fn client(&self) -> &VkClient {
        &self.shared.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_the_controller_releases_a_long_recovery_wait() {
        let controller = Controller::new(VkClient::with_base("http://127.0.0.1:9").unwrap(),
            || Ok(Box::new(crate::audio::SimulatedSink::new(44100,2,1.0)) as Box<dyn Sink>), |_| {},1.0);
        let retained = Arc::downgrade(&controller.shared);
        controller.shared.with(|inner,_| controller.shared.schedule_attempt(inner,"unused".into(),Duration::from_secs(86400)));
        drop(controller);
        let deadline = Instant::now() + Duration::from_secs(1);
        while retained.upgrade().is_some() {
            assert!(Instant::now() < deadline,"a cancelled timer must release playback state");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn dropping_the_controller_releases_a_long_stall_watch() {
        let controller = Controller::new(VkClient::with_base("http://127.0.0.1:9").unwrap(),
            || Ok(Box::new(crate::audio::SimulatedSink::new(44100,2,1.0)) as Box<dyn Sink>), |_| {},1.0);
        let retained = Arc::downgrade(&controller.shared);
        controller.shared.with(|inner,_| controller.shared.watch_stall(inner,"unused".into()));
        drop(controller);
        let deadline = Instant::now() + Duration::from_secs(1);
        while retained.upgrade().is_some() {
            assert!(Instant::now() < deadline,"a cancelled stall watch must release playback state");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn radio_test_controller(fetch: RadioFetch) -> Controller {
        let controller = Controller::new(VkClient::with_base("http://127.0.0.1:9").unwrap(),
            || Ok(Box::new(crate::audio::SimulatedSink::new(44100,2,1.0)) as Box<dyn Sink>), |_| {},1.0);
        let track = crate::vk::model::parse_track(&serde_json::json!({
            "id":1,"owner_id":7,"title":"First","artist":"Artist","duration":100
        }), Some(7)).unwrap();
        controller.load_queue(vec![track], 0, 0.0);
        controller.shared.with(|inner, _| {
            inner.radio = Some(Radio::new(RadioKind::Personal, 1, fetch, 400, vec![], inner.queue.tracks()));
        });
        controller
    }

    fn wait_for_radio(controller: &Controller, predicate: impl Fn(&Inner) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if predicate(&controller.shared.lock()) { return; }
            assert!(Instant::now() < deadline, "radio did not settle: {:?}", controller.state());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn radio_next_at_the_tail_waits_and_retries_a_failed_round() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let controller = radio_test_controller(Arc::new(move |_| {
            let n = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 { return Err(Error::Timeout); }
            Ok(vec![crate::vk::model::parse_track(&serde_json::json!({
                "id":2,"owner_id":7,"title":"Next","artist":"Artist","duration":100
            }), Some(7)).unwrap()])
        }));
        assert!(controller.state().can_next, "a live radio still has a continuation");
        controller.next();
        wait_for_radio(&controller, |inner| inner.queue.current_index() == Some(1));
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 2);
        controller.stop();
    }

    #[test]
    fn radio_end_retries_empty_rounds_until_the_bounded_budget_is_exhausted() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let controller = radio_test_controller(Arc::new(move |_| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![])
        }));
        let track_id = controller.state().track.unwrap().id();
        controller.shared.on_player_event(PlayerEvent::Ended { track_id });
        wait_for_radio(&controller, |inner| inner.radio.as_ref().unwrap().done && inner.status == Status::Idle);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), (radio::ATTEMPTS * radio::MAX_BAD_ROUNDS) as usize);
        assert!(!controller.state().can_next);
    }

    #[test]
    fn pausing_during_radio_refill_does_not_restart_on_a_late_response() {
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let waiting = gate.clone();
        let controller = radio_test_controller(Arc::new(move |_| {
            let (lock, wake) = &*waiting;
            let mut ready = lock.lock().unwrap();
            while !*ready { ready = wake.wait(ready).unwrap(); }
            Ok(vec![crate::vk::model::parse_track(&serde_json::json!({
                "id":2,"owner_id":7,"title":"Next","artist":"Artist","duration":100
            }), Some(7)).unwrap()])
        }));
        controller.next();
        assert_eq!(controller.state().status, Status::Loading);
        crate::media_sync::apply(&controller, crate::media_sync::MediaAction::Pause);
        let paused_status = controller.state().status;
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert_eq!(paused_status, Status::Idle, "notification Pause must cancel loading too");
        wait_for_radio(&controller, |inner| inner.queue.len() == 2 && !inner.radio.as_ref().unwrap().in_flight);
        assert_eq!(controller.state().status, Status::Idle);
        assert_eq!(controller.state().queue_index, Some(0));
    }

    #[test]
    fn failures_are_told_apart_by_what_helps() {
        for error in [Error::Timeout, Error::Offline, Error::Transport("x".into()), Error::Http(503), Error::Http(429), Error::Vk { code: 9, message: String::new() }] {
            assert_eq!(classify(&error), Class::Connectivity, "{error:?}");
        }
        for error in [Error::Http(403), Error::Http(404), Error::Http(410), Error::Http(401), Error::Stream("decode".into())] {
            assert_eq!(classify(&error), Class::Content, "{error:?}");
        }
        for error in [Error::Unauthorized, Error::Format("x".into()), Error::Vk { code: 15, message: String::new() }, Error::InvalidRequest, Error::Cancelled] {
            assert_eq!(classify(&error), Class::Fatal, "{error:?}");
        }
    }

    #[test]
    fn log_names_never_carry_the_error_text() {
        // Тексты `Transport` и `Stream` могут содержать адреса; журнал получает фиксированное имя.
        assert_eq!(what(&Error::Transport("https://cdn/x?token=abc".into())), "connection lost");
        assert_eq!(what(&Error::Stream("https://cdn/x?token=abc".into())), "stream error");
        assert_eq!(what(&Error::Http(403)), "403");
        assert_eq!(what(&Error::Stream("decoder: bad packet https://secret".into())), "audio decoding failed");
        assert_eq!(classify(&Error::Stream("audio device disconnected or stopped responding".into())), Class::Connectivity);
    }

    #[test]
    fn the_schedule_is_bounded_jittered_and_backs_off() {
        let policy = RecoveryPolicy::default();
        let secs = |connectivity, attempt, unit| policy.delay(connectivity, attempt, unit).as_secs_f64();
        // Мертвый адрес повторно разрешается сразу в первый раз, а затем прекращается.
        assert_eq!(secs(false, 1, 0.5), 0.0);
        assert!((secs(false, 2, 0.5) - 1.2).abs() < 1e-9);
        assert!((secs(false, 3, 0.5) - 2.4).abs() < 1e-9);
        // Плохое соединение длится мгновение, затем 1,2, 2,4, 4,8… ограничено 20 с.
        assert!((0.3..0.5).contains(&secs(true, 1, 0.5)));
        let ladder: Vec<f64> = (2..=6).map(|n| secs(true, n, 0.5)).collect();
        assert!(ladder.windows(2).all(|w| w[1] > w[0]), "{ladder:?}");
        assert!(ladder.iter().all(|d| *d <= 20.0));
        // Джиттер остается в пределах ±15 %, и ничто никогда не превышает допустимые пределы.
        for attempt in 1..=40 {
            for unit in [0.0, 0.3, 1.0, f64::NAN] {
                assert!(secs(true, attempt, unit) <= 20.0 && secs(false, attempt, unit) <= 6.0);
            }
        }
        let (low, high) = (secs(true, 4, 0.0), secs(true, 4, 1.0));
        assert!(high / low < 1.4 && low < 4.8 && high > 4.8);
        // Общее время ожидания, прежде чем отказаться от проблемы с соединением, составляет менее
        // минуты.
        let total: f64 = (1..=policy.connectivity_attempts).map(|n| secs(true, n, 1.0)).sum();
        assert!(total < 50.0, "{total}");
        // После восстановления сети: короткая случайная пауза.
        assert!(policy.network_return_delay(0.0).as_secs_f64() >= 0.1 && policy.network_return_delay(1.0).as_secs_f64() <= 0.5);
        // Тесты сокращаются с каждым ожиданием.
        let fast = RecoveryPolicy { time_scale: 0.01, ..policy };
        assert!(fast.delay(true, 6, 1.0).as_secs_f64() < 0.25);
    }
}
