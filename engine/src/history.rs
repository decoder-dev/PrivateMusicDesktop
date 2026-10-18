//! Что воспроизводилось на этом компьютере: краткая история последних треков и компактная статистика
//! прослушивания, которые хранятся в одном небольшом файле JSON в папке данных приложения
//! (`listening-<VK user id>.json`).
//!
//! Это порт приложений iOS `ListeningProgressPolicy`, `ListeningHistoryStore` и `ListeningStats`:
//!
//! * трек считается прослушанным, когда *время фактического прослушивания* достигает 30 % его длины
//!   (поиск не учитывается, пауза не учитывается);
//! * в истории сохраняются 250 последних отдельных треков, сначала самые новые;
//! * статистика ведет счетчики за месяц, а не журнал игр за игрой.
//!
//! В файле хранятся только метаданные дорожки: нет токена сеанса, файлов cookie и адреса потока. Здесь
//! ничего и никогда никуда не отправляют.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};

use crate::vk::Track;
use crate::{Error, Result};

/// Доля трека, которая должна быть прослушана, прежде чем она будет засчитана как проигранная.
pub const REQUIRED_FRACTION: f64 = 0.30;
/// История хранит множество различных следов.
pub const MAX_ENTRIES: usize = 250;
/// Секунды прослушивания засчитываются в счетчики, как только их накопится столько.
pub const COMMIT_THRESHOLD: f64 = 10.0;
/// В месяц сохраняется не более такого количества следов; хвост с одной игрой опущен.
pub const TRACK_LIMIT: usize = 400;
pub const TRACK_KEEP: usize = 300;
/// День засчитывается как "with music" за одну минуту прослушивания.
pub const ACTIVE_DAY_SECONDS: f64 = 60.0;
/// Записи в каждом топ-листе.
pub const TOP_COUNT: usize = 5;
/// Шаг позиции больше этого — это поиск, а не прослушивание (игрок сообщает каждую четверть секунды, то
/// есть это три пропущенных сообщения).
const MAX_LISTEN_DELTA: f64 = 1.5;
const SAVE_DELAY: Duration = Duration::from_secs(5);
const MAX_FILE: u64 = 32 * 1024 * 1024;
const VERSION: u32 = 1;

/// Считается ли трек прослушанным в течение `listened` секунд из `duration` (iOS
/// `ListeningProgressPolicy.shouldMarkListened`).
pub fn should_mark_listened(listened: f64, duration: f64, already_marked: bool) -> bool {
    !already_marked && listened.is_finite() && duration.is_finite() && duration > 0.0 && listened / duration >= REQUIRED_FRACTION
}

// ------------------------------------------------------------------ время

/// Момент местного времени пользователя, исходя из секунд Unix и смещения UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Local {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    /// 0 = воскресенье… 6 = суббота.
    weekday: u32,
}

fn local(unix: i64, offset_minutes: i32) -> Local {
    let seconds = unix.saturating_add(i64::from(offset_minutes) * 60);
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Гражданская дата отсчитывается от дней, прошедших с 1 января 1970 г. (алгоритм Х. Хиннанта).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Local { year, month, day, hour: (of_day / 3600) as u32, weekday: (days + 4).rem_euclid(7) as u32 }
}

fn month_key(at: Local) -> String {
    format!("{:04}-{:02}", at.year, at.month)
}

/// Имена исполнителей сравниваются свободно: регистр, ударения, ё и й сложены, как на iOS.
fn artist_key(name: &str) -> String {
    name.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ё' => 'е',
            'й' => 'и',
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ñ' | 'ń' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'ý' | 'ÿ' => 'y',
            'š' | 'ś' => 's',
            'ž' | 'ź' | 'ż' => 'z',
            other => other,
        })
        .collect()
}

// ------------------------------------------------------------------ данные

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub track: Track,
    /// Unix-секунды.
    pub played_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackTally {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
    pub plays: u32,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtistTally {
    pub name: String,
    pub plays: u32,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Month {
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub plays: u32,
    /// Секунды слышны в каждый час дня.
    #[serde(default)]
    pub hour_seconds: Vec<f64>,
    /// Секунды слышимости в каждый будний день, 0 = воскресенье.
    #[serde(default)]
    pub weekday_seconds: Vec<f64>,
    /// Секунды на день месяца, с кодом "01"…"31".
    #[serde(default)]
    pub day_seconds: BTreeMap<String, f64>,
    #[serde(default)]
    pub tracks: HashMap<String, TrackTally>,
    #[serde(default)]
    pub artists: HashMap<String, ArtistTally>,
}

impl Default for Month {
    fn default() -> Self {
        Self {
            seconds: 0.0,
            plays: 0,
            hour_seconds: vec![0.0; 24],
            weekday_seconds: vec![0.0; 7],
            day_seconds: BTreeMap::new(),
            tracks: HashMap::new(),
            artists: HashMap::new(),
        }
    }
}

impl Month {
    fn has_data(&self) -> bool {
        self.seconds > 0.0 || self.plays > 0
    }

    /// Файл, написанный вручную или в другой версии, не должен нарушать математические вычисления.
    fn sanitize(&mut self) {
        let fix = |v: &mut f64| {
            if !v.is_finite() || *v < 0.0 {
                *v = 0.0;
            }
        };
        fix(&mut self.seconds);
        self.hour_seconds.resize(24, 0.0);
        self.weekday_seconds.resize(7, 0.0);
        self.hour_seconds.iter_mut().for_each(fix);
        self.weekday_seconds.iter_mut().for_each(fix);
        self.day_seconds.values_mut().for_each(fix);
        self.tracks.values_mut().for_each(|t| fix(&mut t.seconds));
        self.artists.values_mut().for_each(|a| fix(&mut a.seconds));
        self.prune();
    }

    /// Месяц остается небольшим, независимо от того, как долго используется приложение: хвост
    /// прослушанных однажды треков все равно не может попасть в топ-лист.
    fn prune(&mut self) {
        if self.tracks.len() <= TRACK_LIMIT {
            return;
        }
        let mut all: Vec<(String, TrackTally)> = self.tracks.drain().collect();
        all.sort_by(|a, b| {
            b.1.plays
                .cmp(&a.1.plays)
                .then(b.1.seconds.total_cmp(&a.1.seconds))
                .then(a.0.cmp(&b.0))
        });
        all.truncate(TRACK_KEEP);
        self.tracks = all.into_iter().collect();
    }
}

/// Что статистика должна знать о треке.
#[derive(Debug, Clone, PartialEq)]
struct Subject {
    id: String,
    title: String,
    artist: String,
    artist_key: String,
    artwork_url: Option<String>,
}

impl Subject {
    fn of(track: &Track) -> Self {
        Self {
            id: track.id(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            artist_key: artist_key(&track.artist),
            artwork_url: track.artwork_url.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub months: BTreeMap<String, Month>,
}

impl Stats {
    fn tally(&mut self, subject: &Subject, key: String) -> &mut Month {
        let month = self.months.entry(key).or_default();
        month.tracks.entry(subject.id.clone()).or_insert_with(|| TrackTally {
            title: subject.title.clone(),
            artist: subject.artist.clone(),
            artwork_url: subject.artwork_url.clone(),
            plays: 0,
            seconds: 0.0,
        });
        if !subject.artist_key.is_empty() {
            month.artists.entry(subject.artist_key.clone()).or_insert_with(|| ArtistTally {
                name: subject.artist.clone(),
                plays: 0,
                seconds: 0.0,
            });
        }
        month
    }

    fn add_play(&mut self, subject: &Subject, at: Local) {
        let month = self.tally(subject, month_key(at));
        month.plays += 1;
        if let Some(track) = month.tracks.get_mut(&subject.id) {
            track.plays += 1;
        }
        if let Some(artist) = month.artists.get_mut(&subject.artist_key) {
            artist.plays += 1;
        }
        month.prune();
    }

    fn add_listening(&mut self, seconds: f64, subject: &Subject, at: Local) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        let month = self.tally(subject, month_key(at));
        month.seconds += seconds;
        month.hour_seconds[at.hour as usize % 24] += seconds;
        month.weekday_seconds[at.weekday as usize % 7] += seconds;
        *month.day_seconds.entry(format!("{:02}", at.day)).or_insert(0.0) += seconds;
        // Пьеса засчитывается через некоторое время после начала прослушивания, поэтому здесь можно
        // создать подсчеты, а их пьесы поднять позже.
        if let Some(track) = month.tracks.get_mut(&subject.id) {
            track.seconds += seconds;
        }
        if let Some(artist) = month.artists.get_mut(&subject.artist_key) {
            artist.seconds += seconds;
        }
        month.prune();
    }
}

// --------------------------------------------------------------- Резюме

/// Какие месяцы показывает экран статистики.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Period {
    /// "YYYY-MM"
    Month { key: String },
    Year { year: i32 },
    All,
}

impl Period {
    fn matches(&self, month_key: &str) -> bool {
        match self {
            Period::All => true,
            Period::Month { key } => key == month_key,
            Period::Year { year } => month_key.starts_with(&format!("{year:04}-")),
        }
    }
}

/// `YYYY-MM` с реальным месяцем.
fn valid_month_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() == 7
        && bytes[4] == b'-'
        && bytes[..4].iter().chain(&bytes[5..]).all(u8::is_ascii_digit)
        && matches!(key[5..].parse::<u32>(), Ok(1..=12))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SummaryEntry {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork_url: Option<String>,
    pub plays: u32,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BusyDay {
    /// "YYYY-MM-DD"
    pub key: String,
    pub seconds: f64,
}

/// Цифры, отображаемые на экране статистики за один период.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub period: Period,
    pub total_seconds: f64,
    pub plays: u32,
    pub active_days: u32,
    pub top_artists: Vec<SummaryEntry>,
    pub top_tracks: Vec<SummaryEntry>,
    pub hour_seconds: Vec<f64>,
    /// Час дня, когда больше всего слушают.
    pub peak_hour: Option<u32>,
    /// 0 = воскресенье… 6 = суббота.
    pub favourite_weekday: Option<u32>,
    pub busiest_day: Option<BusyDay>,
    pub is_empty: bool,
}

/// Индекс наибольшего значения; тот, кто раньше всех, выигрывает вничью. `None`, когда все ноль.
fn peak_index(values: &[f64]) -> Option<u32> {
    let mut best: Option<usize> = None;
    for (index, value) in values.iter().enumerate() {
        if *value > 0.0 && best.is_none_or(|b| *value > values[b]) {
            best = Some(index);
        }
    }
    best.map(|i| i as u32)
}

fn ranking(a: &SummaryEntry, b: &SummaryEntry) -> std::cmp::Ordering {
    b.plays.cmp(&a.plays).then(b.seconds.total_cmp(&a.seconds)).then(a.id.cmp(&b.id))
}

impl Stats {
    pub fn summary(&self, period: Period) -> Summary {
        let mut total_seconds = 0.0;
        let mut plays = 0;
        let mut hours = vec![0.0; 24];
        let mut weekdays = vec![0.0; 7];
        let mut days: BTreeMap<String, f64> = BTreeMap::new();
        let mut tracks: HashMap<&str, TrackTally> = HashMap::new();
        let mut artists: HashMap<&str, ArtistTally> = HashMap::new();

        // Сначала самые старые, поэтому отображаются названия самых новых месяцев.
        for (key, month) in self.months.iter().filter(|(key, _)| period.matches(key)) {
            total_seconds += month.seconds;
            plays += month.plays;
            for (sum, value) in hours.iter_mut().zip(&month.hour_seconds) {
                *sum += value;
            }
            for (sum, value) in weekdays.iter_mut().zip(&month.weekday_seconds) {
                *sum += value;
            }
            for (day, seconds) in &month.day_seconds {
                days.insert(format!("{key}-{day}"), *seconds);
            }
            for (id, tally) in &month.tracks {
                match tracks.get_mut(id.as_str()) {
                    Some(merged) => {
                        merged.plays += tally.plays;
                        merged.seconds += tally.seconds;
                        merged.title = tally.title.clone();
                        merged.artist = tally.artist.clone();
                        merged.artwork_url = tally.artwork_url.clone().or(merged.artwork_url.take());
                    }
                    None => {
                        tracks.insert(id, tally.clone());
                    }
                }
            }
            for (id, tally) in &month.artists {
                match artists.get_mut(id.as_str()) {
                    Some(merged) => {
                        merged.plays += tally.plays;
                        merged.seconds += tally.seconds;
                        merged.name = tally.name.clone();
                    }
                    None => {
                        artists.insert(id, tally.clone());
                    }
                }
            }
        }

        let mut top_tracks: Vec<SummaryEntry> = tracks
            .into_iter()
            .map(|(id, t)| SummaryEntry {
                id: id.to_string(),
                title: t.title,
                subtitle: (!t.artist.is_empty()).then_some(t.artist),
                artwork_url: t.artwork_url,
                plays: t.plays,
                seconds: t.seconds,
            })
            .collect();
        top_tracks.sort_by(ranking);
        top_tracks.truncate(TOP_COUNT);

        let mut top_artists: Vec<SummaryEntry> = artists
            .into_iter()
            .map(|(id, a)| SummaryEntry {
                id: id.to_string(),
                title: a.name,
                subtitle: None,
                artwork_url: None,
                plays: a.plays,
                seconds: a.seconds,
            })
            .collect();
        top_artists.sort_by(ranking);
        top_artists.truncate(TOP_COUNT);

        // Самый ранний день выигрывает вничью, как и в iOS.
        let busiest_day = days
            .iter()
            .filter(|(_, seconds)| **seconds > 0.0)
            .fold(None::<(&String, f64)>, |best, (key, seconds)| match best {
                Some((_, top)) if *seconds <= top => best,
                _ => Some((key, *seconds)),
            })
            .map(|(key, seconds)| BusyDay { key: key.clone(), seconds });

        Summary {
            is_empty: total_seconds <= 0.0 && plays == 0,
            period,
            total_seconds,
            plays,
            active_days: days.values().filter(|s| **s >= ACTIVE_DAY_SECONDS).count() as u32,
            top_artists,
            top_tracks,
            peak_hour: peak_index(&hours),
            favourite_weekday: peak_index(&weekdays),
            hour_seconds: hours,
            busiest_day,
        }
    }

    /// Месяцы, по которым есть данные, сначала самые новые.
    pub fn available_months(&self) -> Vec<String> {
        self.months.iter().rev().filter(|(_, m)| m.has_data()).map(|(k, _)| k.clone()).collect()
    }

    /// Годы, по которым есть данные, сначала самые новые.
    pub fn available_years(&self) -> Vec<i32> {
        let mut years: Vec<i32> = self.available_months().iter().filter_map(|k| k[..4].parse().ok()).collect();
        years.dedup();
        years
    }
}

/// Область действия, которую запрашивает окно: `month`, `year` или `all`, с возможностью выбора
/// (`"YYYY-MM"` или `"YYYY"`).
#[derive(Debug, Clone, Serialize)]
pub struct StatsView {
    pub summary: Summary,
    /// Месяцы с данными, сначала самые новые.
    pub months: Vec<String>,
    pub years: Vec<i32>,
}

// --------------------------------------------------------------- прогресс

/// Превращает отчеты о положении игрока в "seconds really listened".
#[derive(Debug, Default)]
struct Progress {
    track_id: Option<String>,
    last: Option<f64>,
    listened: f64,
    marked: bool,
}

struct Step {
    /// Секунды на счету.
    seconds: f64,
    /// Трек только что достиг порога воспроизведения.
    counted: bool,
}

impl Progress {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn advance(&mut self, position: f64, duration: f64, playing: bool) -> Step {
        let previous = self.last.replace(position);
        let mut step = Step { seconds: 0.0, counted: false };
        let Some(previous) = previous.filter(|_| playing) else { return step };
        let delta = position - previous;
        if !(delta > 0.0 && delta <= MAX_LISTEN_DELTA) {
            return step;
        }
        self.listened += delta;
        step.seconds = delta;
        if should_mark_listened(self.listened, duration, self.marked) {
            self.marked = true;
            step.counted = true;
        }
        step
    }
}

// ------------------------------------------------------------------ Магазин

#[derive(Serialize, Deserialize)]
struct FileData {
    version: u32,
    #[serde(default)]
    entries: Vec<HistoryEntry>,
    #[serde(default)]
    stats: Stats,
}

struct Pending {
    subject: Subject,
    seconds: f64,
    /// Когда начался фрагмент, то есть тот, который находится между полуночью, приходится на день
    /// начала прослушивания.
    since: i64,
}

struct Inner {
    dir: Option<PathBuf>,
    account: Option<i64>,
    entries: Vec<HistoryEntry>,
    stats: Stats,
    progress: Progress,
    pending: Option<Pending>,
    utc_offset: i32,
    dirty: bool,
    clock: Box<dyn Fn() -> i64 + Send>,
}

pub struct History {
    inner: Arc<Mutex<Inner>>,
    wake: Option<Sender<()>>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn file_path(dir: &Path, account: i64) -> PathBuf {
    dir.join(format!("listening-{account}.json"))
}

impl Inner {
    fn path(&self) -> Option<PathBuf> {
        Some(file_path(self.dir.as_deref()?, self.account?))
    }

    fn now_local(&self) -> Local {
        local((self.clock)(), self.utc_offset)
    }

    fn commit_pending(&mut self) {
        let Some(pending) = self.pending.take() else { return };
        if pending.seconds > 0.0 {
            let at = local(pending.since, self.utc_offset);
            self.stats.add_listening(pending.seconds, &pending.subject, at);
            self.dirty = true;
        }
    }

    fn listening(&mut self, seconds: f64, track: &Track) {
        let subject = Subject::of(track);
        if self.pending.as_ref().is_some_and(|p| p.subject.id != subject.id) {
            self.commit_pending();
        }
        let since = (self.clock)();
        let pending = self.pending.get_or_insert(Pending { subject, seconds: 0.0, since });
        pending.seconds += seconds;
        if pending.seconds >= COMMIT_THRESHOLD {
            self.commit_pending();
        }
    }

    fn played(&mut self, track: &Track) {
        self.commit_pending();
        let at = self.now_local();
        self.stats.add_play(&Subject::of(track), at);
        let id = track.id();
        if self.entries.first().is_none_or(|e| e.track.id() != id) {
            self.entries.retain(|e| e.track.id() != id);
            // Адрес потока представляет собой подписанную ссылку с истекающим сроком действия: никогда
            // не сохраняется.
            let track = Track { stream_url: None, ..track.clone() };
            self.entries.insert(0, HistoryEntry { track, played_at: (self.clock)() });
            self.entries.truncate(MAX_ENTRIES);
        }
        self.dirty = true;
    }

    fn save(&mut self) {
        self.commit_pending();
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(path) = self.path() else { return };
        let data = FileData { version: VERSION, entries: self.entries.clone(), stats: self.stats.clone() };
        if let Err(error) = write_atomic(&path, &data) {
            log::warn!("could not save the listening history: {error}");
        }
    }

    fn load(&mut self) {
        self.entries.clear();
        self.stats = Stats::default();
        self.pending = None;
        self.progress.reset();
        self.dirty = false;
        let Some(path) = self.path() else { return };
        let raw = match fs::metadata(&path) {
            Ok(meta) if meta.len() <= MAX_FILE => fs::read(&path),
            Ok(_) => Err(std::io::Error::other("file too large")),
            Err(error) => Err(error),
        };
        let bytes = match raw {
            Ok(bytes) => bytes,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("listening history unreadable ({error}); starting a new one");
                    let _ = fs::rename(&path, path.with_extension("damaged.json"));
                }
                return;
            }
        };
        match serde_json::from_slice::<FileData>(&bytes) {
            Ok(mut file) => {
                file.entries.truncate(MAX_ENTRIES);
                for entry in &mut file.entries {
                    entry.track.stream_url = None;
                }
                file.stats.months.retain(|key, _| valid_month_key(key));
                file.stats.months.values_mut().for_each(Month::sanitize);
                self.entries = file.entries;
                self.stats = file.stats;
            }
            Err(error) => {
                log::warn!("listening history damaged ({error}); starting a new one");
                let _ = fs::rename(&path, path.with_extension("damaged.json"));
            }
        }
    }
}

fn write_atomic(path: &Path, data: &FileData) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(data).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

impl History {
    /// `dir` — место хранения файла; `None` сохраняет все в памяти.
    pub fn open(dir: Option<PathBuf>) -> Arc<Self> {
        Self::build(dir, Box::new(unix_now), Some(SAVE_DELAY))
    }

    /// Для тестов: фиксированные часы и отсутствие фоновой заставки (вызов [`History::flush`]).
    pub fn with_clock(dir: Option<PathBuf>, clock: impl Fn() -> i64 + Send + 'static) -> Arc<Self> {
        Self::build(dir, Box::new(clock), None)
    }

    fn build(dir: Option<PathBuf>, clock: Box<dyn Fn() -> i64 + Send>, delay: Option<Duration>) -> Arc<Self> {
        let inner = Arc::new(Mutex::new(Inner {
            dir,
            account: None,
            entries: Vec::new(),
            stats: Stats::default(),
            progress: Progress::default(),
            pending: None,
            utc_offset: 0,
            dirty: false,
            clock,
        }));
        let wake = delay.and_then(|delay| {
            let (sender, receiver) = crossbeam_channel::unbounded::<()>();
            let shared = inner.clone();
            std::thread::Builder::new()
                .name("history-saver".into())
                .spawn(move || {
                    let save = || shared.lock().unwrap_or_else(|p| p.into_inner()).save();
                    while receiver.recv().is_ok() {
                        std::thread::sleep(delay);
                        while receiver.try_recv().is_ok() {}
                        save();
                    }
                    save();
                })
                .ok()
                .map(|_| sender)
        });
        Arc::new(Self { inner, wake })
    }

    /// Запускает `f` под блокировкой и будит заставку, когда она что-то испачкала.
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let (result, wake) = {
            let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            let was_dirty = inner.dirty;
            let result = f(&mut inner);
            (result, inner.dirty && !was_dirty)
        };
        if wake {
            if let Some(sender) = &self.wake {
                let _ = sender.send(());
            }
        }
        result
    }

    /// Переключается на другую учетную запись VK (или ни на одну: тогда ничего не записывается).
    /// Сначала сохраняются данные исходящего аккаунта.
    pub fn set_account(&self, account: Option<i64>) {
        self.with(|inner| {
            if inner.account == account {
                return;
            }
            inner.save();
            inner.account = account;
            inner.load();
        });
    }

    pub fn account(&self) -> Option<i64> {
        self.with(|inner| inner.account)
    }

    /// В нескольких минутах к востоку от UTC. Часы и дни недели в статистике соответствуют часам
    /// пользователя; окно знает зону и сообщает об этом.
    pub fn set_utc_offset(&self, minutes: i32) -> Result<()> {
        if !(-14 * 60..=14 * 60).contains(&minutes) {
            return Err(Error::InvalidRequest);
        }
        self.with(|inner| {
            inner.commit_pending();
            inner.utc_offset = minutes;
        });
        Ok(())
    }

    /// Игрок сообщил о своей позиции. `playing` истинно только тогда, когда звук действительно
    /// воспроизводится (а не во время паузы или буферизации).
    pub fn tick(&self, track: &Track, position: f64, duration: f64, playing: bool) {
        if !position.is_finite() {
            return;
        }
        self.with(|inner| {
            if inner.account.is_none() {
                return;
            }
            let id = track.id();
            if inner.progress.track_id.as_deref() != Some(id.as_str()) {
                inner.commit_pending();
                inner.progress.reset();
                inner.progress.track_id = Some(id);
            }
            let duration = if duration > 0.0 { duration } else { f64::from(track.duration) };
            let step = inner.progress.advance(position, duration, playing);
            if step.seconds > 0.0 {
                inner.listening(step.seconds, track);
            }
            if step.counted {
                inner.played(track);
            }
            if !playing {
                inner.commit_pending();
            }
        });
    }

    /// Трек (повторно) начинается с начала: повторение одного трека засчитывается снова.
    pub fn restart(&self) {
        self.with(|inner| {
            inner.commit_pending();
            inner.progress.reset();
        });
    }

    /// Свежие треки, сначала самые новые.
    pub fn entries(&self) -> Vec<HistoryEntry> {
        self.with(|inner| inner.entries.clone())
    }

    pub fn remove(&self, track_id: &str) -> bool {
        self.with(|inner| {
            let before = inner.entries.len();
            inner.entries.retain(|e| e.track.id() != track_id);
            let removed = inner.entries.len() != before;
            inner.dirty |= removed;
            removed
        })
    }

    /// Забывает недавние треки. Статистика остается.
    pub fn clear(&self) {
        self.with(|inner| {
            inner.entries.clear();
            inner.dirty = true;
        });
    }

    /// Забывает статистику. Недавние следы остаются.
    pub fn reset_stats(&self) {
        self.with(|inner| {
            inner.pending = None;
            inner.stats = Stats::default();
            inner.dirty = true;
        });
    }

    /// Статистика для области: `"month"`, `"year"` или `"all"`, с выбором (`YYYY-MM` / `YYYY`) или
    /// самого нового периода, по которому есть данные.
    pub fn month_plays(&self)->HashMap<String,u32>{
        self.with(|inner|{
            inner.commit_pending();
            let key=month_key(inner.now_local());
            inner.stats.months.get(&key).map(|month|month.tracks.iter().map(|(id,tally)|(id.clone(),tally.plays)).collect()).unwrap_or_default()
        })
    }

    pub fn stats(&self, scope: &str, choice: Option<&str>) -> Result<StatsView> {
        self.with(|inner| {
            inner.commit_pending();
            let months = inner.stats.available_months();
            let years = inner.stats.available_years();
            let now = inner.now_local();
            let period = match scope {
                "all" => Period::All,
                "month" => {
                    let key = match choice {
                        Some(key) if valid_month_key(key) => key.to_string(),
                        Some(_) => return Err(Error::InvalidRequest),
                        None => months.first().cloned().unwrap_or_else(|| month_key(now)),
                    };
                    Period::Month { key }
                }
                "year" => {
                    let year = match choice {
                        Some(text) if text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit()) => text.parse().unwrap_or(0),
                        Some(_) => return Err(Error::InvalidRequest),
                        None => years.first().copied().unwrap_or(now.year as i32),
                    };
                    Period::Year { year }
                }
                _ => return Err(Error::InvalidRequest),
            };
            Ok(StatsView { summary: inner.stats.summary(period), months, years })
        })
    }

    /// Записывает на диск то, что сейчас ожидает выполнения (используется при выходе и в тестах).
    pub fn flush(&self) {
        self.with(|inner| inner.save());
    }
}

impl Drop for History {
    fn drop(&mut self) {
        // Закрытие канала позволяет потоку сохранения выполнить окончательное сохранение.
        self.wake = None;
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    fn track(n: i64, artist: &str, duration: u32) -> Track {
        Track {
            track_id: n,
            owner_id: 7,
            title: format!("Трек {n}"),
            artist: artist.into(),
            album_title: None,
            duration,
            stream_url: Some("https://cdn.example/secret-signed-link".into()),
            artwork_url: Some(format!("https://cdn.example/{n}.jpg")),
            access_key: None,
            lyrics_id: None,
            is_hq: false,
            track_code: None,
            artists: Vec::new(),
            album: None,
        }
    }

    /// 04.03.2026 (среда) 10:30:00 UTC.
    const T0: i64 = 1_772_620_200;

    fn rig(dir: Option<PathBuf>) -> (Arc<History>, Arc<AtomicI64>) {
        let now = Arc::new(AtomicI64::new(T0));
        let clock = now.clone();
        let history = History::with_clock(dir, move || clock.load(Ordering::Relaxed));
        history.set_account(Some(7));
        (history, now)
    }

    /// Воспроизводит `seconds` из `track`, начиная с `from`, так как 4 сообщает в секунду.
    fn listen(history: &History, track: &Track, from: f64, seconds: f64) -> f64 {
        let mut position = from;
        let end = from + seconds;
        while position < end - 1e-9 {
            position += 0.25;
            history.tick(track, position, f64::from(track.duration), true);
        }
        position
    }

    #[test]
    fn the_threshold_is_thirty_percent_of_the_duration() {
        assert!(!should_mark_listened(29.9, 100.0, false));
        assert!(should_mark_listened(30.0, 100.0, false));
        assert!(!should_mark_listened(99.0, 100.0, true), "only once");
        assert!(!should_mark_listened(10.0, 0.0, false) && !should_mark_listened(f64::NAN, 100.0, false));
        assert!(!should_mark_listened(10.0, f64::INFINITY, false));
    }

    #[test]
    fn dates_come_out_in_the_users_zone() {
        let utc = local(T0, 0);
        assert_eq!((utc.year, utc.month, utc.day, utc.hour, utc.weekday), (2026, 3, 4, 10, 3));
        let tokyo = local(T0, 9 * 60);
        assert_eq!((tokyo.day, tokyo.hour), (4, 19));
        let la = local(T0, -8 * 60);
        assert_eq!((la.day, la.hour, la.weekday), (4, 2, 3));
        let before_epoch = local(-1, 0);
        assert_eq!((before_epoch.year, before_epoch.month, before_epoch.day, before_epoch.hour, before_epoch.weekday), (1969, 12, 31, 23, 3));
        let leap = local(1_709_164_800, 0); // 2024-02-29
        assert_eq!((leap.year, leap.month, leap.day, leap.weekday), (2024, 2, 29, 4));
        assert_eq!(month_key(local(T0 + 40 * 86_400, 0)), "2026-04");
    }

    #[test]
    fn a_play_counts_after_thirty_percent_of_real_listening() {
        let (history, _) = rig(None);
        let song = track(1, "Кино", 100);
        let at = listen(&history, &song, 0.0, 29.0);
        assert!(history.entries().is_empty(), "29 s of 100 is not a play");
        listen(&history, &song, at, 2.0);
        let entries = history.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].played_at, T0);
        assert_eq!(entries[0].track.stream_url, None, "the signed stream link is never kept");
        // Продолжение не считается дважды.
        listen(&history, &song, 31.0, 60.0);
        assert_eq!(history.stats("all", None).unwrap().summary.plays, 1);
    }

    #[test]
    fn seeking_and_pausing_do_not_count_as_listening() {
        let (history, _) = rig(None);
        let song = track(1, "Кино", 100);
        for second in 0..20 {
            // Скачок вперед в каждом отчете: ни один шаг не вызывает доверия.
            history.tick(&song, f64::from(second) * 10.0, 100.0, true);
        }
        assert!(history.entries().is_empty());
        // В приостановленных отчетах курсор воспроизведения перемещается, но ничего не указывается.
        for step in 0..400 {
            history.tick(&song, f64::from(step) * 0.25, 100.0, false);
        }
        assert!(history.entries().is_empty());
        assert!(history.stats("all", None).unwrap().summary.is_empty);
        // Обратно тоже не слушается.
        history.tick(&song, 50.0, 100.0, true);
        history.tick(&song, 10.0, 100.0, true);
        assert!(history.stats("all", None).unwrap().summary.total_seconds <= 0.0);
    }

    #[test]
    fn listening_before_and_after_a_seek_adds_up() {
        let (history, _) = rig(None);
        let song = track(1, "Кино", 100);
        listen(&history, &song, 0.0, 20.0);
        history.tick(&song, 70.0, 100.0, true); // искать
        assert!(history.entries().is_empty());
        listen(&history, &song, 70.0, 11.0);
        assert_eq!(history.entries().len(), 1, "20 s + 11 s of 100");
    }

    #[test]
    fn repeating_a_track_counts_it_again_only_after_a_restart() {
        let (history, _) = rig(None);
        let song = track(1, "Кино", 20);
        listen(&history, &song, 0.0, 7.0);
        listen(&history, &song, 7.0, 13.0);
        assert_eq!(history.stats("all", None).unwrap().summary.plays, 1);
        history.restart();
        listen(&history, &song, 0.0, 7.0);
        assert_eq!(history.stats("all", None).unwrap().summary.plays, 2);
        assert_eq!(history.entries().len(), 1, "one entry per track");
    }

    #[test]
    fn the_history_is_newest_first_without_duplicates_and_bounded() {
        let (history, now) = rig(None);
        for n in 0..(MAX_ENTRIES as i64 + 20) {
            let song = track(n, "A", 10);
            history.restart();
            listen(&history, &song, 0.0, 4.0);
            now.fetch_add(1, Ordering::Relaxed);
        }
        let entries = history.entries();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].track.track_id, MAX_ENTRIES as i64 + 19);
        assert_eq!(entries.last().unwrap().track.track_id, 20);
        // Повторное воспроизведение старой игры перемещает ее на передний план.
        let again = track(100, "A", 10);
        history.restart();
        listen(&history, &again, 0.0, 4.0);
        let entries = history.entries();
        assert_eq!(entries[0].track.track_id, 100);
        assert_eq!(entries.iter().filter(|e| e.track.track_id == 100).count(), 1);
        assert_eq!(entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn statistics_bucket_by_the_users_local_hour_weekday_and_day() {
        let (history, _) = rig(None);
        history.set_utc_offset(9 * 60).unwrap(); // 19:30 в среду, 4-го числа.
        let song = track(1, "Кино", 100);
        listen(&history, &song, 0.0, 40.0);
        let view = history.stats("month", None).unwrap();
        assert_eq!(view.months, ["2026-03"]);
        let s = &view.summary;
        assert_eq!(s.period, Period::Month { key: "2026-03".into() });
        assert_eq!((s.plays, s.peak_hour, s.favourite_weekday), (1, Some(19), Some(3)));
        assert!((s.total_seconds - 39.75).abs() < 1e-6, "{}", s.total_seconds);
        assert_eq!(s.busiest_day.as_ref().unwrap().key, "2026-03-04");
        assert_eq!(s.active_days, 0, "40 s is under a minute");
        assert_eq!(s.top_tracks[0].title, "Трек 1");
        assert_eq!(s.top_artists[0].title, "Кино");
        assert_eq!(view.years, [2026]);
    }

    #[test]
    fn periods_pick_the_right_months_and_reject_bad_choices() {
        let (history, now) = rig(None);
        let song = track(1, "Кино", 20);
        listen(&history, &song, 0.0, 7.0);
        now.store(T0 + 40 * 86_400, Ordering::Relaxed); // апрель
        history.restart();
        let other = track(2, "Кино", 20);
        listen(&history, &other, 0.0, 7.0);
        let all = history.stats("all", None).unwrap();
        assert_eq!(all.summary.plays, 2);
        assert_eq!(all.summary.top_artists[0].plays, 2, "same artist across months");
        let march = history.stats("month", Some("2026-03")).unwrap();
        assert_eq!(march.summary.plays, 1);
        assert_eq!(march.months, ["2026-04", "2026-03"], "newest first");
        assert_eq!(history.stats("year", Some("2026")).unwrap().summary.plays, 2);
        assert_eq!(history.stats("year", Some("2025")).unwrap().summary.plays, 0);
        for (scope, choice) in [("month", "2026-13"), ("month", "x"), ("year", "26"), ("decade", "")] {
            assert_eq!(history.stats(scope, Some(choice)).unwrap_err(), Error::InvalidRequest, "{scope} {choice}");
        }
    }

    #[test]
    fn clear_and_reset_are_independent() {
        let (history, _) = rig(None);
        let song = track(1, "Кино", 20);
        listen(&history, &song, 0.0, 7.0);
        history.reset_stats();
        assert_eq!(history.entries().len(), 1, "resetting the stats keeps the history");
        assert!(history.stats("all", None).unwrap().summary.is_empty);
        history.restart();
        listen(&history, &song, 0.0, 7.0);
        history.clear();
        assert!(history.entries().is_empty());
        assert_eq!(history.stats("all", None).unwrap().summary.plays, 1, "clearing the history keeps the stats");
        assert!(!history.remove("7_1"));
    }

    #[test]
    fn single_entries_can_be_removed() {
        let (history, _) = rig(None);
        for n in 1..=2 {
            history.restart();
            listen(&history, &track(n, "A", 10), 0.0, 4.0);
        }
        assert!(history.remove("7_1"));
        assert_eq!(history.entries().len(), 1);
        assert_eq!(history.entries()[0].track.track_id, 2);
    }

    #[test]
    fn nothing_is_recorded_without_an_account_and_accounts_are_separate() {
        let dir = tempfile::tempdir().unwrap();
        let (history, _) = rig(Some(dir.path().into()));
        history.set_account(None);
        listen(&history, &track(1, "A", 10), 0.0, 5.0);
        assert!(history.entries().is_empty());
        history.set_account(Some(7));
        listen(&history, &track(1, "A", 10), 0.0, 5.0);
        assert_eq!(history.entries().len(), 1);
        history.set_account(Some(8));
        assert!(history.entries().is_empty(), "another account starts empty");
        history.set_account(Some(7));
        assert_eq!(history.entries().len(), 1, "and the first one is back");
        assert!(dir.path().join("listening-7.json").exists());
    }

    #[test]
    fn data_survives_a_restart_and_holds_no_secrets() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (history, _) = rig(Some(dir.path().into()));
            listen(&history, &track(1, "Кино", 20), 0.0, 8.0);
            history.flush();
        }
        let text = fs::read_to_string(dir.path().join("listening-7.json")).unwrap();
        for forbidden in ["secret-signed-link", "stream_url\":\"", "token", "cookie"] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
        let (again, _) = rig(Some(dir.path().into()));
        assert_eq!(again.entries().len(), 1);
        let stats = again.stats("all", None).unwrap().summary;
        assert_eq!(stats.plays, 1);
        assert!((stats.total_seconds - 7.75).abs() < 1e-6, "the first report only sets the start");
    }

    #[test]
    fn a_damaged_or_hostile_file_is_set_aside_or_repaired() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("listening-7.json"), b"{ not json").unwrap();
        let (history, _) = rig(Some(dir.path().into()));
        assert!(history.entries().is_empty());
        assert!(dir.path().join("listening-7.damaged.json").exists());

        let hostile = r#"{"version":1,"entries":[],"stats":{"months":{
            "../../x":{"seconds":5},
            "2026-02":{"seconds":-4,"plays":1,"hour_seconds":[1,2],"weekday_seconds":[],"day_seconds":{"01":-1}}}}}"#;
        fs::write(dir.path().join("listening-8.json"), hostile).unwrap();
        history.set_account(Some(8));
        let view = history.stats("all", None).unwrap();
        assert_eq!(view.months, ["2026-02"], "a bad month key is dropped");
        assert_eq!(view.summary.hour_seconds.len(), 24);
        assert!(view.summary.total_seconds >= 0.0 && view.summary.total_seconds.is_finite());
    }

    #[test]
    fn a_months_track_list_stays_bounded() {
        let mut stats = Stats::default();
        let at = local(T0, 0);
        for n in 0..(TRACK_LIMIT as i64 + 50) {
            let subject = Subject::of(&track(n, "A", 10));
            stats.add_play(&subject, at);
            if n < 10 {
                stats.add_play(&subject, at);
            }
        }
        let month = &stats.months["2026-03"];
        assert!(month.tracks.len() <= TRACK_LIMIT);
        assert_eq!(month.tracks.len(), TRACK_KEEP + 49);
        assert!(month.tracks.contains_key("7_0"), "the most played survive");
    }

    #[test]
    fn artist_names_are_compared_loosely() {
        assert_eq!(artist_key("  Ёлка "), artist_key("елка"));
        assert_eq!(artist_key("Beyoncé"), artist_key("BEYONCE"));
        assert_eq!(artist_key("Йога"), artist_key("иога"));
        assert_eq!(artist_key(" "), "");
    }

    #[test]
    fn the_utc_offset_is_checked() {
        let (history, _) = rig(None);
        assert!(history.set_utc_offset(14 * 60).is_ok() && history.set_utc_offset(-12 * 60).is_ok());
        assert!(history.set_utc_offset(15 * 60).is_err() && history.set_utc_offset(i32::MIN).is_err());
    }
}
