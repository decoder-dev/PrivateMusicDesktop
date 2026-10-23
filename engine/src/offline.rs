//! Загруженные треки: аудиофайлы в собственной папке приложения плюс индекс.
//!
//! Файл содержит элементарный поток, который плеер мог бы создать из сети (расшифровать,
//! демультиплексировать), поэтому для его воспроизведения не требуется ни VK, ни Интернета. Окно
//! никогда не называет файл: треки ищутся по идентификатору, а имена файлов здесь составляются только
//! из цифр.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::stream::fetch::{safe_offline_name, OFFLINE_SCHEME};
use crate::stream::ElementaryKind;
use crate::vk::Track;
use crate::{Error, Result};

const INDEX: &str = "index.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OfflineTrack {
    pub track: Track,
    pub file: String,
    pub bytes: u64,
    /// Unix-секунды.
    pub added: u64,
}

/// Public file facts; private cache names and paths are never serialized to the UI.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DownloadDetails {
    pub bytes: u64,
    pub added: u64,
    pub format: String,
    pub available: bool,
}

pub struct OfflineStore {
    writes: Mutex<()>,
    dir: PathBuf,
    index: Mutex<BTreeMap<String, OfflineTrack>>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl OfflineStore {
    /// Открывает (или создает) папку. Поврежденный индекс откладывается, это не фатально.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| Error::Stream(format!("downloads folder: {e}")))?;
        let index = match fs::read(dir.join(INDEX)) {
            Ok(bytes) => serde_json::from_slice::<Vec<OfflineTrack>>(&bytes).unwrap_or_else(|error| {
                log::warn!("downloads index unreadable ({error}); starting a new one");
                let _ = fs::rename(dir.join(INDEX), dir.join("index.damaged.json"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        // Сохраняйте только те записи, файлы которых действительно существуют и имеют безопасное имя.
        let index = index
            .into_iter()
            .filter(|e| safe_offline_name(&e.file) && dir.join(&e.file).is_file())
            .map(|e| (e.track.id(), e))
            .collect();
        Ok(Self { writes: Mutex::new(()), dir, index: Mutex::new(index) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, OfflineTrack>> {
        self.index.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Сначала самые новые.
    pub fn list(&self) -> Vec<OfflineTrack> {
        let mut all: Vec<OfflineTrack> = self.lock().values().cloned().collect();
        all.sort_by(|a, b| b.added.cmp(&a.added).then(a.file.cmp(&b.file)));
        all
    }

    pub fn details(&self, track_id: &str) -> Option<DownloadDetails> {
        let entry = self.lock().get(track_id)?.clone();
        if !safe_offline_name(&entry.file) { return None; }
        let format = match Path::new(&entry.file).extension()?.to_str()? {
            "mp3" => "MP3",
            "aac" => "AAC",
            _ => return None,
        };
        let available = fs::symlink_metadata(self.dir.join(&entry.file))
            .is_ok_and(|file| file.is_file() && file.len() > 0 && file.len() == entry.bytes);
        Some(DownloadDetails { bytes: entry.bytes, added: entry.added, format: format.into(), available })
    }

    pub fn contains(&self, track_id: &str) -> bool {
        self.lock().contains_key(track_id)
    }

    /// Адрес `offline:` понятен плееру, если он скачан.
    pub fn address(&self, track_id: &str) -> Option<String> {
        self.lock().get(track_id).map(|e| format!("{OFFLINE_SCHEME}{}", e.file))
    }

    pub fn total_bytes(&self) -> u64 {
        self.lock().values().map(|e| e.bytes).sum()
    }

    /// Записывает звук и записывает его. Файл отображается атомарно, поэтому сбой в середине записи
    /// никогда не оставляет в индексе половину файла.
    pub fn save(&self, track: &Track, kind: ElementaryKind, audio: &[u8]) -> Result<OfflineTrack> {
        let _write = self.writes.lock().unwrap_or_else(|p| p.into_inner());
        if track.track_id <= 0 || track.owner_id == 0 || audio.is_empty() {
            return Err(Error::InvalidRequest);
        }
        let extension = match kind {
            ElementaryKind::AdtsAac => "aac",
            ElementaryKind::Mp3 => "mp3",
        };
        let owner = if track.owner_id < 0 { format!("m{}", -track.owner_id) } else { track.owner_id.to_string() };
        let file = format!("{owner}_{}.{extension}", track.track_id);
        debug_assert!(safe_offline_name(&file));
        let temporary = self.dir.join(format!("{file}.part"));
        fs::write(&temporary, audio).map_err(|e| Error::Stream(format!("saving the download: {e}")))?;
        fs::rename(&temporary, self.dir.join(&file)).map_err(|e| Error::Stream(format!("saving the download: {e}")))?;
        let entry = OfflineTrack {
            track: Track { stream_url: None, ..track.clone() },
            file,
            bytes: audio.len() as u64,
            added: now(),
        };
        let mut index = self.lock();
        index.insert(track.id(), entry.clone());
        self.write_index(&index)?;
        Ok(entry)
    }

    pub fn remove(&self, track_id: &str) -> Result<bool> {
        let _write = self.writes.lock().unwrap_or_else(|p| p.into_inner());
        let mut index = self.lock();
        let Some(entry) = index.remove(track_id) else { return Ok(false) };
        let _ = fs::remove_file(self.dir.join(&entry.file));
        self.write_index(&index)?;
        Ok(true)
    }

    fn write_index(&self, index: &BTreeMap<String, OfflineTrack>) -> Result<()> {
        let list: Vec<&OfflineTrack> = index.values().collect();
        let json = serde_json::to_vec_pretty(&list).map_err(|e| Error::Format(e.to_string()))?;
        let temporary = self.dir.join(format!("{INDEX}.part"));
        fs::write(&temporary, json).map_err(|e| Error::Stream(format!("downloads index: {e}")))?;
        fs::rename(&temporary, self.dir.join(INDEX)).map_err(|e| Error::Stream(format!("downloads index: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(owner: i64, id: i64) -> Track {
        Track {
            track_id: id, owner_id: owner, title: format!("T{id}"), artist: "A".into(), album_title: None, duration: 10,
            stream_url: Some("https://cdn.example/x.m3u8".into()), artwork_url: None, access_key: Some("k".into()),
            lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None,
        }
    }

    #[test]
    fn saves_lists_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = OfflineStore::open(dir.path()).unwrap();
        let saved = store.save(&track(-5, 9), ElementaryKind::Mp3, b"audio").unwrap();
        assert_eq!(saved.file, "m5_9.mp3");
        assert_eq!(saved.track.stream_url, None, "expiring addresses are not kept");
        assert_eq!(store.address("-5_9").as_deref(), Some("offline:m5_9.mp3"));
        store.save(&track(7, 1), ElementaryKind::AdtsAac, b"aac!").unwrap();
        assert_eq!(store.total_bytes(), 9);

        let reopened = OfflineStore::open(dir.path()).unwrap();
        assert_eq!(reopened.list().len(), 2);
        assert!(reopened.contains("7_1"));
    }

    #[test]
    fn concurrent_saves_of_the_same_track_keep_complete_audio_and_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(OfflineStore::open(dir.path()).unwrap());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8).map(|value| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.save(&track(7, 1), ElementaryKind::Mp3, &vec![value; 65536]).unwrap();
            })
        }).collect();
        for worker in workers { worker.join().unwrap(); }
        let audio = fs::read(dir.path().join("7_1.mp3")).unwrap();
        assert_eq!(audio.len(), 65536);
        assert!(audio.iter().all(|byte| *byte == audio[0]));
        let reopened = OfflineStore::open(dir.path()).unwrap();
        assert_eq!(reopened.list().len(), 1);
        assert_eq!(reopened.total_bytes(), 65536);
    }

    #[test]
    fn removing_deletes_the_file_and_a_missing_file_drops_the_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = OfflineStore::open(dir.path()).unwrap();
        store.save(&track(7, 1), ElementaryKind::Mp3, b"a").unwrap();
        store.save(&track(7, 2), ElementaryKind::Mp3, b"b").unwrap();
        assert!(store.remove("7_1").unwrap());
        assert!(!dir.path().join("7_1.mp3").exists());
        assert!(!store.remove("7_1").unwrap());
        std::fs::remove_file(dir.path().join("7_2.mp3")).unwrap(); // удалено вручную
        assert!(OfflineStore::open(dir.path()).unwrap().list().is_empty());
    }

    #[test]
    fn a_damaged_or_hostile_index_cannot_point_outside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.json"), b"{not json").unwrap();
        assert!(OfflineStore::open(dir.path()).unwrap().list().is_empty());
        assert!(dir.path().join("index.damaged.json").exists());

        let outside = dir.path().join("secret.mp3");
        std::fs::write(&outside, b"x").unwrap();
        let sub = dir.path().join("downloads");
        let hostile = serde_json::json!([{ "track": track(7, 1), "file": "../secret.mp3", "bytes": 1, "added": 0 }]);
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("index.json"), hostile.to_string()).unwrap();
        assert!(OfflineStore::open(&sub).unwrap().list().is_empty());
    }

    #[test]
    fn details_detect_removed_truncated_or_replaced_files_without_exposing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = OfflineStore::open(dir.path()).unwrap();
        store.save(&track(7, 1), ElementaryKind::Mp3, b"audio").unwrap();
        let details = store.details("7_1").unwrap();
        assert!(details.available);
        assert_eq!(details.bytes, 5);
        assert_eq!(details.format, "MP3");
        let json = serde_json::to_value(details).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 4);
        assert!(json.get("file").is_none());
        assert!(json.get("track").is_none());
        fs::write(dir.path().join("7_1.mp3"), b"a").unwrap();
        assert!(!store.details("7_1").unwrap().available);
        fs::remove_file(dir.path().join("7_1.mp3")).unwrap();
        assert!(!store.details("7_1").unwrap().available);
        fs::create_dir(dir.path().join("7_1.mp3")).unwrap();
        assert!(!store.details("7_1").unwrap().available);
        assert!(store.details("../secret").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn details_do_not_treat_a_symlink_as_downloaded_audio() {
        let dir = tempfile::tempdir().unwrap();
        let store = OfflineStore::open(dir.path()).unwrap();
        store.save(&track(7, 1), ElementaryKind::AdtsAac, b"audio").unwrap();
        let outside = dir.path().join("outside");
        fs::write(&outside, b"audio").unwrap();
        fs::remove_file(dir.path().join("7_1.aac")).unwrap();
        std::os::unix::fs::symlink(outside, dir.path().join("7_1.aac")).unwrap();
        assert!(!store.details("7_1").unwrap().available);
    }

    #[test]
    fn names_are_checked() {
        for good in ["7_1.mp3", "m5_9.aac"] {
            assert!(safe_offline_name(good), "{good}");
        }
        for bad in ["", "../x.mp3", "a/b.mp3", "a\\\\b.mp3", ".hidden", "x..mp3", "C:x.mp3", &"a".repeat(200)] {
            assert!(!safe_offline_name(bad), "{bad}");
        }
    }
}
