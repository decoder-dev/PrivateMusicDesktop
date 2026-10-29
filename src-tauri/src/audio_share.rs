//! Audio files cross the Android boundary as a temporary content URI, never a raw WebView path.
use pm_engine::{app::App, vk::Track, Error, Result};
use tauri::AppHandle;

#[cfg(any(target_os = "android", test))]
mod staging {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    pub const MAX_BYTES: u64 = 512 * 1024 * 1024;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    pub fn cleanup_expired(root: &Path) {
        // Keep already granted URIs alive for a day; cleanup never touches other cache folders.
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("share-"))
                    && entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && entry
                        .metadata()
                        .ok()
                        .and_then(|value| value.modified().ok())
                        .and_then(|value| value.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(86_400))
                {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }
    pub struct Prepared {
        pub path: PathBuf,
        directory: PathBuf,
        pub keep: bool,
    }
    impl Drop for Prepared {
        fn drop(&mut self) {
            if !self.keep {
                let _ = fs::remove_dir_all(&self.directory);
            }
        }
    }
    pub fn prepare(
        source: &Path,
        root: &Path,
        title: &str,
        expected: u64,
        current: impl Fn() -> bool,
    ) -> Result<Prepared> {
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| matches!(*value, "aac" | "mp3" | "m4a" | "wav" | "flac"))
            .ok_or(Error::InvalidRequest)?;
        let metadata = fs::symlink_metadata(source).map_err(|_| Error::Unavailable)?;
        if !metadata.is_file()
            || expected == 0
            || expected > MAX_BYTES
            || metadata.len() != expected
        {
            return Err(Error::InvalidRequest);
        }
        if !current() {
            return Err(Error::Cancelled);
        }
        fs::create_dir_all(root).map_err(|_| Error::Unavailable)?;
        cleanup_expired(root);
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_nanos();
        let directory = root.join(format!(
            "share-{id:x}-{:x}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).map_err(|_| Error::Unavailable)?;
        let mut title: String = title
            .chars()
            .map(|ch| {
                if ch.is_control() || "<>:\"/\\|?*".contains(ch) {
                    '_'
                } else {
                    ch
                }
            })
            .take(96)
            .collect();
        // Filesystem limits count UTF-8 bytes, so emoji-heavy titles need a byte bound too.
        while title.len() > 200 {
            title.pop();
        }
        let title = title.trim().trim_matches('.');
        let path = directory.join(format!(
            "{}.{extension}",
            if title.is_empty() { "Audio" } else { title }
        ));
        let prepared = Prepared {
            path,
            directory,
            keep: false,
        };
        let mut input = fs::File::open(source)
            .map_err(|_| Error::Unavailable)?
            .take(MAX_BYTES + 1);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options
            .open(&prepared.path)
            .map_err(|_| Error::Unavailable)?;
        let mut buffer = [0; 64 * 1024];
        let mut total = 0;
        loop {
            if !current() {
                return Err(Error::Cancelled);
            }
            let read = input.read(&mut buffer).map_err(|_| Error::Unavailable)?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > expected {
                return Err(Error::InvalidRequest);
            }
            output
                .write_all(&buffer[..read])
                .map_err(|_| Error::Unavailable)?;
        }
        if total != expected {
            return Err(Error::InvalidRequest);
        }
        output.flush().map_err(|_| Error::Unavailable)?;
        Ok(prepared)
    }
}

#[cfg(target_os = "android")]
pub fn cleanup_cache(handle: &AppHandle) {
    use tauri::Manager;
    if let Ok(cache) = handle.path().app_cache_dir() {
        staging::cleanup_expired(&cache.join("audio-share"));
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportLyrics {pub text:String,pub lrc:String}
impl ExportLyrics {
    fn valid(&self)->bool {self.text.len()<=150_000 && self.lrc.len()<=200_000}
}

pub fn share(handle: &AppHandle, app: &App, track: &Track, allow_download: bool, lyrics:ExportLyrics) -> Result<bool> {
    if !lyrics.valid(){return Err(Error::InvalidRequest)}
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        use tauri_plugin_vk_login::VkLoginExt;
        let epoch = app.session_epoch()?;
        let folder = app
            .controller()
            .fetch_config()
            .offline_dir
            .ok_or(Error::InvalidRequest)?;
        let cached = app.saved_audio(&track.id()).filter(|entry| {
            entry.track.id() == track.id()
                && std::fs::symlink_metadata(folder.join(&entry.file))
                    .is_ok_and(|meta| meta.is_file() && meta.len() == entry.bytes && meta.len() > 0)
        });
        let entry = match cached {
            Some(entry) => entry,
            None if allow_download => app.download(track)?,
            None => return Ok(false),
        };
        if app.session_epoch()? != epoch {
            return Err(Error::Cancelled);
        }
        if std::path::Path::new(&entry.file)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(entry.file.as_str())
        {
            return Err(Error::InvalidRequest);
        }
        let root = handle
            .path()
            .app_cache_dir()
            .map_err(|_| Error::Unavailable)?
            .join("audio-share");
        let mut prepared = staging::prepare(
            &folder.join(&entry.file),
            &root,
            &format!("{} — {}", track.artist, track.title),
            entry.bytes,
            || app.session_epoch().is_ok_and(|current| current == epoch),
        )?;
        let artwork_path=track.artwork_url.as_deref().and_then(|url|pm_engine::artwork::export_jpeg(url).ok()).and_then(|jpeg|{
            let path=prepared.path.parent()?.join("cover.jpg");std::fs::write(&path,jpeg).ok()?;Some(path)
        });
        if app.session_epoch()? != epoch {
            return Err(Error::Cancelled);
        }
        let canonical = prepared
            .path
            .canonicalize()
            .map_err(|_| Error::Unavailable)?;
        handle
            .vk_login()
            .share_audio(&tauri_plugin_vk_login::AudioShareRequest {
                path:&canonical.to_string_lossy(),title:&track.title,artist:&track.artist,
                album:track.album_title.as_deref().unwrap_or(""),lyrics_text:&lyrics.text,lyrics_lrc:&lyrics.lrc,
                artwork_path:artwork_path.as_deref().and_then(|p|p.to_str()),
            })
            .map_err(|_| Error::Unavailable)?;
        prepared.keep = true;
        Ok(true)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (handle, app, track, allow_download,lyrics);
        Err(Error::InvalidRequest)
    }
}

#[cfg(test)]
mod tests {
    use super::staging::*;
    #[test]
    fn stages_only_the_audio_file_and_removes_unshared_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("track.aac");
        std::fs::write(&source, b"audio").unwrap();
        let root = directory.path().join("audio-share");
        let prepared = prepare(&source, &root, "Artist / Title", 5, || true).unwrap();
        assert_eq!(std::fs::read(&prepared.path).unwrap(), b"audio");
        assert_eq!(prepared.path.file_name().unwrap(), "Artist _ Title.aac");
        let path = prepared.path.clone();
        drop(prepared);
        assert!(!path.exists());
        assert!(source.exists());
    }
    #[test]
    fn rejects_changed_or_excessive_files_and_account_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("track.mp3");
        std::fs::write(&source, b"audio").unwrap();
        let root = directory.path().join("audio-share");
        assert!(prepare(&source, &root, "Title", 4, || true).is_err());
        assert!(prepare(&source, &root, "Title", MAX_BYTES + 1, || true).is_err());
        assert!(prepare(&source, &root, "Title", 5, || false).is_err());
        assert!(!root.exists());
    }
    #[test]
    fn account_change_during_copy_removes_the_partial_payload() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("track.mp3");
        std::fs::write(&source, vec![1; 131_072]).unwrap();
        let root = directory.path().join("audio-share");
        let checks = std::cell::Cell::new(0);
        assert!(prepare(&source, &root, "Title", 131_072, || {
            checks.set(checks.get() + 1);
            checks.get() < 3
        })
        .is_err());
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
        assert_eq!(std::fs::metadata(source).unwrap().len(), 131_072);
    }
    #[test]
    fn unicode_titles_fit_a_filesystem_component_without_splitting_utf8() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("track.mp3");
        std::fs::write(&source, b"audio").unwrap();
        let prepared = prepare(
            &source,
            &directory.path().join("audio-share"),
            &"🎵".repeat(120),
            5,
            || true,
        )
        .unwrap();
        assert!(prepared.path.file_name().unwrap().to_str().unwrap().len() < 255);
        assert_eq!(std::fs::read(&prepared.path).unwrap(), b"audio");
    }
}
