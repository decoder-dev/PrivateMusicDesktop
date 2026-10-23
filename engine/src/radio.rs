//! Бесконечное радио пополняет очередь из персонального микса, каталога
//! или рекомендаций «Похожие треки», сохраняя выбранный источник.
//!
//! Правила соответствуют мобильной версии (`ContinuationPrefetchPolicy`,
//! `MixTrackContinuationCursor`, `RadioRecommendationCursor`,
//! `PlaybackQueueBuilder.uniqueAdditions`):
//!
//! * пополнение начинается, когда после текущего осталось не больше
//!   [`REFILL_REMAINING`] треков; фоновое пополнение добавляет не больше
//!   [`UPCOMING_LIMIT`] треков вперёд и не ограничивает размер списка на экране;
//! * курсор потока сдвигается только после успешного ответа; для похожих треков
//!   исходный трек постепенно меняется, включая новые добавленные композиции;
//! * уже добавленные, прослушанные или удалённые в этой сессии треки исключаются;
//!   пустой результат повторяется ограниченное число раз;
//! * после [`MAX_BAD_ROUNDS`] неудачных раундов или недействительной сессии
//!   пополнение прекращается, и очередь доигрывает оставшиеся треки.
//!
//! Модуль хранит правила и курсор. Запросы выполняются вне аудиопотока
//! и вне блокировки контроллера.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::vk::Track;
use crate::{Error, Result};

/// Заполните его, когда после текущего трека останется столько или меньше треков.
pub const REFILL_REMAINING: usize = 6;
/// Большинство треков в очереди находятся после текущего (iOS `queueLimit`).
pub const UPCOMING_LIMIT: usize = 80;
/// Треки запрашиваются для каждой страницы стрим-микса (iOS `MixTrackRequestPolicy.pageSize`).
pub const PAGE_SIZE: u32 = 100;
/// Страницы, которые окно загружает при запуске микса (iOS `pageCount`).
pub const WINDOW_PAGES: u32 = 4;
/// Окно потока продолжается после загрузки страниц в окне.
pub const FIRST_REFILL_OFFSET: u32 = WINDOW_PAGES * PAGE_SIZE;
/// Запросов за раунд, когда страница не приносит ничего нового.
pub const ATTEMPTS: u32 = 3;
/// Делайте паузу между этими запросами (то же самое ждет предварительная выборка iOS).
pub const ATTEMPT_PAUSE: Duration = Duration::from_millis(180);
/// Раунды без новых треков или неудачные раунды подряд, прежде чем сдаться.
pub const MAX_BAD_ROUNDS: u32 = 3;
/// Семенные треки запомнились для «Похожие треки».
pub const SEED_LIMIT: usize = 32;

/// Из какого источника пополняется очередь; показано у окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioKind {
    /// Персональный стрим-микс (`common` из VK).
    Personal,
    /// Микс из каталога.
    Catalog,
    /// Рекомендации засеяны треком.
    Similar,
}

/// О чем просит один запрос VK.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // одно кратковременное значение на каждый запрос
pub enum RadioRequest {
    /// Страница микса потока, начинающаяся с `offset`.
    Page { offset: u32 },
    /// Рекомендации, засеянные этим треком.
    Seed(Track),
}

/// Выполняет один запрос к VK (с обновлением токена); построено приложением.
pub type RadioFetch = Arc<dyn Fn(&RadioRequest) -> Result<Vec<Track>> + Send + Sync>;

/// Metadata needed to resume a radio queue without storing playback addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioContinuation {
    pub source: crate::app::RadioSource,
    pub next_offset: u32,
}

pub struct RadioSetup {
    pub kind: RadioKind,
    pub fetch: RadioFetch,
    pub offset: u32,
    pub seeds: Vec<Track>,
    pub source: Option<crate::app::RadioSource>,
}

/// Работающее радио. Живет внутри состояния контроллера.
pub struct Radio {
    pub kind: RadioKind,
    pub source: Option<crate::app::RadioSource>,
    /// Сообщает залежавшемуся рабочему, что радио, которое он запускал, заменили.
    pub generation: u64,
    pub fetch: RadioFetch,
    /// Выполняется выборка раунда.
    pub in_flight: bool,
    /// Очередь закончилась, пока был получен раунд: играйте, когда он приземлится.
    pub waiting: bool,
    /// Радио сдалось.
    pub done: bool,
    offset: u32,
    seeds: Vec<Track>,
    seed_at: usize,
    seen: HashSet<String>,
    bad_rounds: u32,
}

impl Radio {
    pub fn new(kind: RadioKind, generation: u64, fetch: RadioFetch, offset: u32, seeds: Vec<Track>, queued: &[Track]) -> Self {
        let mut radio = Self {
            kind,
            source: None,
            generation,
            fetch,
            in_flight: false,
            waiting: false,
            done: false,
            offset,
            seeds: Vec::new(),
            seed_at: 0,
            seen: queued.iter().map(Track::id).collect(),
            bad_rounds: 0,
        };
        radio.rotate_seeds(&seeds);
        radio
    }

    pub fn continuation(&self) -> Option<RadioContinuation> {
        if self.done { return None; }
        Some(RadioContinuation { source: self.source.clone()?, next_offset: self.offset })
    }

    /// Должен ли раунд начинаться сейчас, учитывая треки, оставшиеся в очереди.
    pub fn wants_refill(&self, upcoming: usize) -> bool {
        !self.done && !self.in_flight && upcoming <= REFILL_REMAINING
    }

    /// Следующий запрос, или `None`, когда спросить нечего.
    pub fn next_request(&self) -> Option<RadioRequest> {
        match self.kind {
            RadioKind::Personal | RadioKind::Catalog => Some(RadioRequest::Page { offset: self.offset }),
            RadioKind::Similar => self.seeds.get(self.seed_at % self.seeds.len().max(1)).cloned().map(RadioRequest::Seed),
        }
    }

    /// На запрос был дан ответ: окно перемещается дальше (только сейчас, поэтому неудавшийся запрос
    /// запрашивается повторно, а не пропускается).
    pub fn answered(&mut self, request: &RadioRequest) {
        match request {
            RadioRequest::Page { offset } => self.offset = offset.saturating_add(PAGE_SIZE),
            RadioRequest::Seed(_) => self.seed_at += 1,
        }
    }

    /// Что за страница новая для этого радио.
    pub fn fresh(&self, proposed: Vec<Track>) -> Vec<Track> {
        let mut known = HashSet::new();
        proposed.into_iter().filter(|t| !self.seen.contains(&t.id()) && known.insert(t.id())).collect()
    }

    /// Запоминает треки, попавшие в очередь; для «Похожие треки» они становятся самыми свежими
    /// семенами.
    pub fn record(&mut self, added: &[Track]) {
        self.seen.extend(added.iter().map(Track::id));
        if self.kind == RadioKind::Similar {
            self.rotate_seeds(added);
        }
    }

    fn rotate_seeds(&mut self, fresh: &[Track]) {
        let mut known = HashSet::new();
        let merged: Vec<Track> = fresh.iter().chain(self.seeds.iter()).filter(|t| known.insert(t.id())).take(SEED_LIMIT).cloned().collect();
        self.seeds = merged;
        self.seed_at = 0;
    }

    /// Записывает конец раунда: треки `added` присоединились к очереди, или раунд провалился с
    /// `failure`.
    pub fn finish_round(&mut self, added: usize, failure: Option<&Error>) {
        if added > 0 {
            self.bad_rounds = 0;
            return;
        }
        self.bad_rounds += 1;
        if self.bad_rounds >= MAX_BAD_ROUNDS || matches!(failure, Some(Error::Unauthorized)) {
            self.done = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: i64) -> Track {
        Track {
            track_id: n,
            owner_id: 1,
            title: format!("t{n}"),
            artist: "a".into(),
            album_title: None,
            duration: 100,
            stream_url: None,
            artwork_url: None,
            access_key: None,
            lyrics_id: None,
            is_hq: false,
            track_code: None,
            artists: Vec::new(),
            album: None,
        }
    }

    fn fetch() -> RadioFetch {
        Arc::new(|_| Ok(Vec::new()))
    }

    fn radio(kind: RadioKind) -> Radio {
        let queued: Vec<Track> = (1..=3).map(track).collect();
        Radio::new(kind, 1, fetch(), FIRST_REFILL_OFFSET, queued.clone(), &queued)
    }

    #[test]
    fn a_refill_starts_at_the_threshold_and_never_twice_at_once() {
        let mut r = radio(RadioKind::Catalog);
        assert!(!r.wants_refill(REFILL_REMAINING + 1));
        assert!(r.wants_refill(REFILL_REMAINING));
        assert!(r.wants_refill(0));
        r.in_flight = true;
        assert!(!r.wants_refill(0));
        r.in_flight = false;
        r.done = true;
        assert!(!r.wants_refill(0));
    }

    #[test]
    fn the_stream_window_moves_on_only_after_an_answer() {
        let mut r = radio(RadioKind::Personal);
        let first = r.next_request().unwrap();
        assert_eq!(first, RadioRequest::Page { offset: FIRST_REFILL_OFFSET });
        assert_eq!(r.next_request().unwrap(), first, "a failed request is asked again");
        r.answered(&first);
        assert_eq!(r.next_request().unwrap(), RadioRequest::Page { offset: FIRST_REFILL_OFFSET + PAGE_SIZE });
    }

    #[test]
    fn fresh_drops_what_was_queued_played_or_repeated() {
        let mut r = radio(RadioKind::Catalog);
        let fresh = r.fresh(vec![track(1), track(4), track(4), track(5)]);
        assert_eq!(fresh.iter().map(|t| t.track_id).collect::<Vec<_>>(), vec![4, 5]);
        r.record(&fresh);
        assert!(r.fresh(vec![track(4), track(5)]).is_empty(), "added tracks are remembered even after they leave the queue");
    }

    #[test]
    fn similar_rotates_through_seeds_and_adopts_new_tracks() {
        let mut r = radio(RadioKind::Similar);
        let RadioRequest::Seed(first) = r.next_request().unwrap() else { panic!("a seed request") };
        assert_eq!(first.track_id, 1);
        r.answered(&RadioRequest::Seed(first));
        let RadioRequest::Seed(second) = r.next_request().unwrap() else { panic!("a seed request") };
        assert_eq!(second.track_id, 2);
        r.record(&[track(10)]);
        let RadioRequest::Seed(newest) = r.next_request().unwrap() else { panic!("a seed request") };
        assert_eq!(newest.track_id, 10, "the freshest track seeds the next round");
        let many: Vec<Track> = (100..200).map(track).collect();
        r.record(&many);
        assert_eq!(r.seeds.len(), SEED_LIMIT);
    }

    #[test]
    fn similar_without_seeds_has_nothing_to_ask() {
        let r = Radio::new(RadioKind::Similar, 1, fetch(), 0, Vec::new(), &[]);
        assert!(r.next_request().is_none());
    }

    #[test]
    fn bad_rounds_stop_the_radio_and_a_good_one_resets_the_count() {
        let mut r = radio(RadioKind::Catalog);
        r.finish_round(0, None);
        r.finish_round(0, Some(&Error::Offline));
        assert!(!r.done);
        r.finish_round(5, None);
        r.finish_round(0, None);
        r.finish_round(0, None);
        assert!(!r.done);
        r.finish_round(0, None);
        assert!(r.done);
        let mut dead = radio(RadioKind::Catalog);
        dead.finish_round(0, Some(&Error::Unauthorized));
        assert!(dead.done, "a dead session ends it at once");
    }
}
