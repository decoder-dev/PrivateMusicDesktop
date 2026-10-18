//! Локальные предпочтения Selena применяются к первой странице и фоновому продолжению радио.
use crate::vk::Track;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Taste {
    #[serde(default)]
    pub tracks: Vec<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub less: Vec<String>,
    #[serde(default)]
    pub familiar: Vec<String>,
    #[serde(default = "default_novelty")]
    pub novelty: u8,
}
fn default_novelty() -> u8 {
    50
}
impl Default for Taste {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            artists: Vec::new(),
            less: Vec::new(),
            familiar: Vec::new(),
            novelty: 50,
        }
    }
}
impl Taste {
    pub fn validate(&self) -> Result<()> {
        if self.tracks.len() > 5000
            || self.artists.len() > 1000
            || self.less.len() > 5000
            || self.familiar.len() > 20000
            || self.novelty > 100
            || self
                .tracks
                .iter()
                .chain(&self.less)
                .chain(&self.familiar)
                .any(|s| s.len() > 64)
            || self.artists.iter().any(|s| s.len() > 1024)
        {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }
    pub fn filter(&self, tracks: Vec<Track>) -> Vec<Track> {
        let excluded: HashSet<_> = self.tracks.iter().collect();
        let artists: HashSet<_> = self
            .artists
            .iter()
            .map(|a| a.trim().to_lowercase())
            .collect();
        let less: HashSet<_> = self.less.iter().collect();
        let familiar: HashSet<_> = self.familiar.iter().collect();
        let mut known = Vec::new();
        let mut new = Vec::new();
        let mut later = Vec::new();
        for track in tracks {
            if excluded.contains(&track.id())
                || artists.contains(&track.artist.trim().to_lowercase())
                || track
                    .artists
                    .iter()
                    .any(|a| artists.contains(&a.name.trim().to_lowercase()))
            {
                continue;
            }
            if less.contains(&track.id()) {
                later.push(track);
            } else if familiar.contains(&track.id()) {
                known.push(track);
            } else {
                new.push(track);
            }
        }
        let mut known = known.into_iter().peekable();
        let mut new = new.into_iter().peekable();
        let mut output = Vec::new();
        let mut balance = 50i32;
        while known.peek().is_some() || new.peek().is_some() {
            balance += i32::from(self.novelty);
            let pick_new = balance >= 100;
            if pick_new {
                balance -= 100;
            }
            let track = if pick_new {
                new.next().or_else(|| known.next())
            } else {
                known.next().or_else(|| new.next())
            };
            if let Some(track) = track {
                output.push(track);
            }
        }
        output.extend(later);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(id: i64, artist: &str) -> Track {
        Track {
            track_id: id,
            owner_id: 7,
            title: id.to_string(),
            artist: artist.into(),
            album_title: None,
            duration: 120,
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
    #[test]
    fn exclusions_and_less_apply_before_a_radio_window_is_filled() {
        let taste = Taste {
            tracks: vec!["7_1".into()],
            artists: vec!["  ARTIST  ".into()],
            less: vec!["7_3".into()],
            ..Default::default()
        };
        let filtered = taste.filter(vec![
            track(1, "A"),
            track(2, "Artist"),
            track(3, "B"),
            track(4, "C"),
        ]);
        assert_eq!(
            filtered.iter().map(Track::id).collect::<Vec<_>>(),
            ["7_4", "7_3"]
        );
    }
    #[test]
    fn novelty_orders_available_tracks_without_inventing_or_dropping_candidates() {
        let taste = Taste {
            familiar: vec!["7_1".into(), "7_2".into()],
            novelty: 100,
            ..Default::default()
        };
        let filtered = taste.filter(vec![
            track(1, "A"),
            track(3, "B"),
            track(2, "C"),
            track(4, "D"),
        ]);
        assert_eq!(
            filtered.iter().map(Track::id).collect::<Vec<_>>(),
            ["7_3", "7_4", "7_1", "7_2"]
        );
    }
    #[test]
    fn oversized_preferences_are_rejected() {
        assert!(Taste {
            novelty: 101,
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
