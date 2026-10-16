//! Endless radio: a queue started from a mix, the personal stream or "more
//! like this" keeps filling from the same source while it plays.
//!
//! The rules come from the iOS app (`ContinuationPrefetchPolicy`,
//! `MixTrackContinuationCursor`, `RadioRecommendationCursor`,
//! `PlaybackQueueBuilder.uniqueAdditions`):
//!
//! * a refill starts when at most [`REFILL_REMAINING`] tracks are left after
//!   the current one, and a queue never holds more than [`UPCOMING_LIMIT`]
//!   tracks after it;
//! * a stream mix is read window by window and the window only moves on after
//!   a page arrived; "more like this" asks VK for recommendations seeded with
//!   a rotating track, newly queued tracks becoming seeds themselves;
//! * everything already queued, played or removed in this session is
//!   skipped, and a round that brings nothing new is retried a few times;
//! * a round that fails or stays empty never loops: it stops until the next
//!   track starts, and after [`MAX_BAD_ROUNDS`] in a row (or at once for a
//!   dead session) the radio gives up and the queue simply ends.
//!
//! This module holds the policy and the cursor; the controller runs the
//! requests (off the audio thread and outside its lock).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::vk::Track;
use crate::{Error, Result};

/// Refill when this many tracks or fewer remain after the current one.
pub const REFILL_REMAINING: usize = 6;
/// Most tracks the queue holds after the current one (iOS `queueLimit`).
pub const UPCOMING_LIMIT: usize = 80;
/// Tracks asked for per stream-mix page (iOS `MixTrackRequestPolicy.pageSize`).
pub const PAGE_SIZE: u32 = 100;
/// Where the stream window continues after the two pages of 50 the window
/// loads when it starts a mix.
pub const FIRST_REFILL_OFFSET: u32 = 100;
/// Requests per round when a page brings nothing new.
pub const ATTEMPTS: u32 = 3;
/// Pause between those requests (the iOS prefetch waits the same).
pub const ATTEMPT_PAUSE: Duration = Duration::from_millis(180);
/// Rounds without new tracks, or failed rounds, in a row before giving up.
pub const MAX_BAD_ROUNDS: u32 = 3;
/// Seed tracks remembered for "more like this".
pub const SEED_LIMIT: usize = 32;

/// What kind of source the queue refills from; shown by the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioKind {
    /// The personal stream mix (VK's `common`).
    Personal,
    /// A catalogue mix.
    Catalog,
    /// Recommendations seeded with a track.
    Similar,
}

/// What one request asks VK for.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // one short-lived value per request
pub enum RadioRequest {
    /// A page of a stream mix, starting at `offset`.
    Page { offset: u32 },
    /// Recommendations seeded with this track.
    Seed(Track),
}

/// Runs one request against VK (with token renewal); built by the app.
pub type RadioFetch = Arc<dyn Fn(&RadioRequest) -> Result<Vec<Track>> + Send + Sync>;

/// A running radio. Lives inside the controller's state.
pub struct Radio {
    pub kind: RadioKind,
    /// Tells a stale worker that the radio it started was replaced.
    pub generation: u64,
    pub fetch: RadioFetch,
    /// A round is being fetched.
    pub in_flight: bool,
    /// The queue ran out while a round was fetched: play on when it lands.
    pub waiting: bool,
    /// The radio gave up.
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

    /// Whether a round should start now, given the tracks left in the queue.
    pub fn wants_refill(&self, upcoming: usize) -> bool {
        !self.done && !self.in_flight && upcoming <= REFILL_REMAINING
    }

    /// The next request, or `None` when there is nothing to ask with.
    pub fn next_request(&self) -> Option<RadioRequest> {
        match self.kind {
            RadioKind::Personal | RadioKind::Catalog => Some(RadioRequest::Page { offset: self.offset }),
            RadioKind::Similar => self.seeds.get(self.seed_at % self.seeds.len().max(1)).cloned().map(RadioRequest::Seed),
        }
    }

    /// A request was answered: the window moves on (only now, so a failed
    /// request is asked again rather than skipped).
    pub fn answered(&mut self, request: &RadioRequest) {
        match request {
            RadioRequest::Page { offset } => self.offset = offset.saturating_add(PAGE_SIZE),
            RadioRequest::Seed(_) => self.seed_at += 1,
        }
    }

    /// What of a page is new to this radio.
    pub fn fresh(&self, proposed: Vec<Track>) -> Vec<Track> {
        let mut known = HashSet::new();
        proposed.into_iter().filter(|t| !self.seen.contains(&t.id()) && known.insert(t.id())).collect()
    }

    /// Remembers tracks that joined the queue; for "more like this" they
    /// become the freshest seeds.
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

    /// Books the end of a round: `added` tracks joined the queue, or the
    /// round failed with `failure`.
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
        assert_eq!(first, RadioRequest::Page { offset: 100 });
        assert_eq!(r.next_request().unwrap(), first, "a failed request is asked again");
        r.answered(&first);
        assert_eq!(r.next_request().unwrap(), RadioRequest::Page { offset: 100 + PAGE_SIZE });
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
