//! Очередь воспроизведения: порядок, перемешивание, повтор.
//!
//! `tracks` — список, каким его видит пользователь; `order` — это порядок воспроизведения (идентичность
//! или перемешивание, при котором текущая дорожка всегда остается первой). Идентификатор трека —
//! `Track::id()`, а не позиция, поэтому при редактировании текущий трек никогда не теряется.

use rand::seq::SliceRandom;
use rand::Rng;

use crate::vk::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    Off,
    All,
    One,
}

#[derive(Debug, Clone)]
pub struct Queue {
    tracks: Vec<Track>,
    order: Vec<usize>,
    cursor: Option<usize>,
    shuffle: bool,
    repeat: Repeat,
}

impl Default for Queue {
    fn default() -> Self {
        Self { tracks: Vec::new(), order: Vec::new(), cursor: None, shuffle: false, repeat: Repeat::Off }
    }
}

impl Queue {
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    /// Индекс `tracks()` текущего трека.
    pub fn current_index(&self) -> Option<usize> {
        self.cursor.and_then(|c| self.order.get(c)).copied()
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index().and_then(|i| self.tracks.get(i))
    }

    /// Заменяет очередь и начинается с `start`. При включенном случайном порядке выбранный трек
    /// воспроизводится первым, а остальные следуют в случайном порядке.
    pub fn replace<R: Rng>(&mut self, tracks: Vec<Track>, start: usize, rng: &mut R) {
        self.tracks = tracks;
        if self.tracks.is_empty() {
            self.order.clear();
            self.cursor = None;
            return;
        }
        let start = start.min(self.tracks.len() - 1);
        self.rebuild_order(start, rng);
    }

    fn rebuild_order<R: Rng>(&mut self, current: usize, rng: &mut R) {
        if self.shuffle {
            let mut rest: Vec<usize> = (0..self.tracks.len()).filter(|i| *i != current).collect();
            rest.shuffle(rng);
            self.order = std::iter::once(current).chain(rest).collect();
            self.cursor = Some(0);
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.cursor = Some(current);
        }
    }

    pub fn set_shuffle<R: Rng>(&mut self, on: bool, rng: &mut R) {
        if self.shuffle == on {
            return;
        }
        self.shuffle = on;
        if let Some(current) = self.current_index() {
            self.rebuild_order(current, rng);
        }
    }

    /// Трек после текущего. `auto` имеет значение true, когда предыдущая дорожка закончилась сама по
    /// себе (повторение одной дорожки затем повторяет ее); Следующее нажатие пропускает ложь. `None`
    /// означает, что очередь закончилась.
    pub fn advance(&mut self, auto: bool) -> Option<&Track> {
        let cursor = self.cursor?;
        if auto && self.repeat == Repeat::One {
            return self.current();
        }
        let next = cursor + 1;
        if next < self.order.len() {
            self.cursor = Some(next);
        } else if self.repeat == Repeat::All {
            self.cursor = Some(0);
        } else {
            return None;
        }
        self.current()
    }

    pub fn previous(&mut self) -> Option<&Track> {
        let cursor = self.cursor?;
        if cursor > 0 {
            self.cursor = Some(cursor - 1);
        } else if self.repeat == Repeat::All {
            self.cursor = Some(self.order.len() - 1);
        }
        self.current()
    }

    /// Переходит на позицию в `tracks()`.
    pub fn jump(&mut self, index: usize) -> Option<&Track> {
        let cursor = self.order.iter().position(|i| *i == index)?;
        self.cursor = Some(cursor);
        self.current()
    }

    /// Вставляется сразу после текущей дорожки.
    pub fn play_next(&mut self, track: Track) {
        let Some(cursor) = self.cursor else {
            self.tracks.push(track);
            self.order = vec![0];
            self.cursor = Some(0);
            return;
        };
        self.tracks.push(track);
        self.order.insert(cursor + 1, self.tracks.len() - 1);
    }

    pub fn play_last(&mut self, track: Track) {
        let was_empty = self.tracks.is_empty();
        self.tracks.push(track);
        self.order.push(self.tracks.len() - 1);
        if was_empty {
            self.cursor = Some(0);
        }
    }

    /// Треки, которые будут следовать после текущего, в порядке воспроизведения.
    pub fn upcoming(&self) -> usize {
        match self.cursor {
            Some(cursor) => self.order.len().saturating_sub(cursor + 1),
            None => self.order.len(),
        }
    }

    /// Добавляет треки, которые еще не имеются в очереди (по идентификатору трека), не более `room` из
    /// них, в конец порядка воспроизведения. Возвращает то, что было добавлено. Пустая очередь ничего
    /// не требует: бесконечный источник не должен восстанавливать очередь, которую очистил
    /// пользователь.
    pub fn append_unique(&mut self, tracks: Vec<Track>, room: usize) -> Vec<Track> {
        if self.cursor.is_none() {
            return Vec::new();
        }
        let mut known: std::collections::HashSet<String> = self.tracks.iter().map(Track::id).collect();
        let mut added = Vec::new();
        for track in tracks {
            if added.len() >= room {
                break;
            }
            if known.insert(track.id()) {
                self.tracks.push(track.clone());
                self.order.push(self.tracks.len() - 1);
                added.push(track);
            }
        }
        added
    }

    /// Истинно, если текущая дорожка является последней в порядке воспроизведения.
    pub fn at_last(&self) -> bool {
        self.cursor.is_some_and(|c| c + 1 == self.order.len())
    }

    /// Делает первый трек в порядке воспроизведения текущим (перенос с повторением всех).
    pub fn rewind(&mut self) {
        self.cursor = if self.order.is_empty() { None } else { Some(0) };
    }

    /// Удаляет трек по индексу `tracks()`. Возвращает true, если это была текущая дорожка (вызывающая
    /// сторона должна начать следующую дорожку или остановиться).
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.tracks.len() {
            return false;
        }
        let removed_cursor = self.order.iter().position(|i| *i == index);
        let was_current = removed_cursor.is_some() && removed_cursor == self.cursor;
        self.tracks.remove(index);
        if let Some(position) = removed_cursor {
            self.order.remove(position);
        }
        for slot in self.order.iter_mut() {
            if *slot > index {
                *slot -= 1;
            }
        }
        if self.order.is_empty() {
            self.cursor = None;
            return was_current;
        }
        if let (Some(cursor), Some(position)) = (self.cursor, removed_cursor) {
            if position < cursor {
                self.cursor = Some(cursor - 1);
            } else if position == cursor && cursor >= self.order.len() {
                // Текущая дорожка была последней: курсор теперь указывает на новую последнюю дорожку, и
                // `was_current` приказывает вызывающему объекту остановиться.
                self.cursor = Some(self.order.len() - 1);
            }
        }
        was_current
    }

    /// Заменяет обновленную копию трека (новый поток URL) везде, где он появляется. Позиция и порядок
    /// не тронуты.
    pub fn replace_track(&mut self, fresh: &Track) {
        let id = fresh.id();
        for slot in self.tracks.iter_mut().filter(|t| t.id() == id) {
            *slot = fresh.clone();
        }
    }

    pub fn clear(&mut self) {
        *self = Self { shuffle: self.shuffle, repeat: self.repeat, ..Self::default() };
    }

    /// Удалите только предстоящие треки, сохранив историю и текущий элемент.
    pub fn clear_upcoming(&mut self) {
        let Some(cursor) = self.cursor else { return };
        let mut remove = self.order[cursor + 1..].to_vec();
        remove.sort_unstable_by(|a, b| b.cmp(a));
        for index in remove { self.remove(index); }
    }

    /// Измените порядок предстоящих треков по отображаемым индексам. Ни проигрываемые треки, ни текущий
    /// нельзя перемещать, в том числе в случайном порядке.
    pub fn move_upcoming(&mut self, from: usize, to: usize) -> bool {
        let Some(cursor) = self.cursor else { return false };
        let (Some(a), Some(b)) = (self.order.iter().position(|i| *i == from), self.order.iter().position(|i| *i == to)) else { return false };
        if a <= cursor || b <= cursor || from == to { return false; }
        let item = self.order.remove(a);
        self.order.insert(b, item);
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        for index in &mut self.order {
            *index = if *index == from { to }
                else if from < to && *index > from && *index <= to { *index - 1 }
                else if from > to && *index >= to && *index < from { *index + 1 }
                else { *index };
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn upcoming_edits_keep_the_current_track_in_normal_and_shuffled_queues() {
        for shuffle in [false, true] {
            let (mut q, mut rng) = queue(8);
            q.set_shuffle(shuffle, &mut rng);
            q.advance(false);
            let current = q.current().unwrap().track_id;
            let cursor = q.cursor.unwrap();
            let from = q.order[cursor + 1];
            let to = *q.order.last().unwrap();
            let moved = q.tracks[from].track_id;
            assert!(q.move_upcoming(from, to));
            assert_eq!(q.current().unwrap().track_id, current);
            assert_eq!(q.tracks[*q.order.last().unwrap()].track_id, moved);
            assert!(!q.move_upcoming(q.current_index().unwrap(), to));
            q.clear_upcoming();
            assert_eq!(q.current().unwrap().track_id, current);
            assert_eq!(q.upcoming(), 0);
            assert!(q.advance(false).is_none());
        }
    }

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

    fn queue(n: i64) -> (Queue, StdRng) {
        let mut rng = StdRng::seed_from_u64(7);
        let mut q = Queue::default();
        q.replace((1..=n).map(track).collect(), 0, &mut rng);
        (q, rng)
    }

    fn ids(q: &Queue) -> Vec<i64> {
        q.order.iter().map(|i| q.tracks[*i].track_id).collect()
    }

    #[test]
    fn plays_in_order_and_stops_at_the_end() {
        let (mut q, _) = queue(3);
        assert_eq!(q.current().unwrap().track_id, 1);
        assert_eq!(q.advance(true).unwrap().track_id, 2);
        assert_eq!(q.advance(false).unwrap().track_id, 3);
        assert!(q.advance(true).is_none());
        assert_eq!(q.current().unwrap().track_id, 3, "a finished queue keeps its last track");
    }

    #[test]
    fn repeat_modes() {
        let (mut q, _) = queue(2);
        q.set_repeat(Repeat::One);
        assert_eq!(q.advance(true).unwrap().track_id, 1, "auto-advance repeats the track");
        assert_eq!(q.advance(false).unwrap().track_id, 2, "Next press still moves on");
        assert!(q.advance(false).is_none() || true);
        q.set_repeat(Repeat::All);
        q.jump(1);
        assert_eq!(q.advance(true).unwrap().track_id, 1, "wraps to the start");
        assert_eq!(q.previous().unwrap().track_id, 2, "previous wraps back");
    }

    #[test]
    fn repeat_one_next_on_last_track_does_not_jump_to_start() {
        let (mut q, _) = queue(2);
        q.set_repeat(Repeat::One);
        q.jump(1);
        assert!(q.advance(false).is_none());
    }

    #[test]
    fn shuffle_keeps_current_first_and_every_track_once() {
        let (mut q, mut rng) = queue(20);
        q.jump(5);
        q.set_shuffle(true, &mut rng);
        assert_eq!(q.current().unwrap().track_id, 6);
        let mut seen = ids(&q);
        assert_eq!(seen[0], 6);
        seen.sort_unstable();
        assert_eq!(seen, (1..=20).collect::<Vec<_>>());
        q.set_shuffle(false, &mut rng);
        assert_eq!(q.current().unwrap().track_id, 6, "unshuffling keeps the playing track");
        assert_eq!(ids(&q), (1..=20).collect::<Vec<_>>());
    }

    #[test]
    fn replace_with_shuffle_starts_on_the_chosen_track() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut q = Queue::default();
        q.set_shuffle(true, &mut rng);
        q.replace((1..=10).map(track).collect(), 4, &mut rng);
        assert_eq!(q.current().unwrap().track_id, 5);
    }

    #[test]
    fn play_next_and_last() {
        let (mut q, _) = queue(3);
        q.play_next(track(10));
        q.play_last(track(11));
        assert_eq!(ids(&q), vec![1, 10, 2, 3, 11]);
        assert_eq!(q.advance(false).unwrap().track_id, 10);
        let mut empty = Queue::default();
        empty.play_next(track(1));
        assert_eq!(empty.current().unwrap().track_id, 1);
    }

    #[test]
    fn removing_tracks_keeps_the_current_one() {
        let (mut q, _) = queue(5);
        q.jump(2);
        assert!(!q.remove(0), "removing an earlier track is not the current one");
        assert_eq!(q.current().unwrap().track_id, 3);
        assert!(!q.remove(3));
        assert_eq!(q.current().unwrap().track_id, 3);
        assert!(q.remove(1), "removing the current track is reported");
        assert_eq!(q.current().unwrap().track_id, 4, "the next track takes its place");
        assert!(!q.remove(99));
    }

    #[test]
    fn removing_the_last_current_track_points_at_the_new_last() {
        let (mut q, _) = queue(2);
        q.jump(1);
        assert!(q.remove(1));
        assert_eq!(q.current().unwrap().track_id, 1);
        assert!(q.remove(0));
        assert!(q.current().is_none());
        assert!(q.is_empty());
    }

    #[test]
    fn replace_track_updates_every_copy_without_moving_anything() {
        let (mut q, _) = queue(3);
        q.play_last(track(2));
        q.jump(1);
        let mut fresh = track(2);
        fresh.stream_url = Some("https://new.example/a".into());
        q.replace_track(&fresh);
        assert_eq!(q.current().unwrap().stream_url.as_deref(), Some("https://new.example/a"));
        assert_eq!(q.tracks().iter().filter(|t| t.stream_url.is_some()).count(), 2);
        assert_eq!(q.current_index(), Some(1));
    }

    #[test]
    fn appending_skips_known_tracks_and_respects_the_room() {
        let (mut q, _) = queue(3);
        assert_eq!(q.upcoming(), 2);
        let added = q.append_unique(vec![track(2), track(4), track(4), track(5), track(6)], 2);
        assert_eq!(added.iter().map(|t| t.track_id).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(ids(&q), vec![1, 2, 3, 4, 5]);
        assert_eq!(q.upcoming(), 4);
        q.advance(false);
        q.advance(false);
        assert_eq!(q.upcoming(), 2);
        assert!(q.append_unique(vec![track(9)], 0).is_empty());
        let mut empty = Queue::default();
        assert!(empty.append_unique(vec![track(1)], 5).is_empty(), "an emptied queue stays empty");
    }

    #[test]
    fn empty_queue_is_safe() {
        let mut q = Queue::default();
        assert!(q.advance(true).is_none());
        assert!(q.previous().is_none());
        assert!(q.jump(0).is_none());
        assert!(!q.remove(0));
    }
}
