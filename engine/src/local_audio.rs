//! On-device file imports. The system picker owns source selection; the engine
//! validates actual audio and creates a separate identity before storing a copy.
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;
use sha2::{Digest, Sha256};
use symphonia::core::{
    codecs::{DecoderOptions, CODEC_TYPE_NULL}, errors::Error as MediaError,
    formats::FormatOptions, io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::{MetadataOptions, MetadataRevision, StandardTagKey}, probe::Hint,
};
use crate::{Error, Result};
use crate::vk::Track;

pub const MAXIMUM_BYTES: u64 = 150_000_000;
pub const EXTENSIONS: &[&str] = &["mp3", "m4a", "aac", "wav", "flac"];

pub struct LocalAudio {
    pub track: Track,
    pub file: File,
    pub bytes: u64,
    pub digest: String,
    pub extension: String,
}

fn text(metadata: &[MetadataRevision], key: StandardTagKey) -> Option<String> {
    metadata.iter().flat_map(|m| m.tags()).find(|tag| tag.std_key == Some(key))
        .map(|tag| tag.value.to_string().trim().chars().filter(|c| !c.is_control()).take(500).collect::<String>())
        .filter(|value| !value.is_empty())
}

pub fn inspect(path: &Path, display_name: &str) -> Result<LocalAudio> {
    let extension = Path::new(display_name).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !EXTENSIONS.contains(&extension.as_str()) { return Err(Error::InvalidRequest); }
    let mut file = File::open(path).map_err(|_| Error::InvalidRequest)?;
    let facts = file.metadata().map_err(|_| Error::InvalidRequest)?;
    let bytes = facts.len();
    if !facts.is_file() || bytes == 0 || bytes > MAXIMUM_BYTES { return Err(Error::InvalidRequest); }
    let mut hash = Sha256::new();
    let mut chunk = [0; 65_536];
    let mut seen = 0u64;
    loop {
        let count = file.read(&mut chunk).map_err(|_| Error::InvalidRequest)?;
        if count == 0 { break; }
        seen += count as u64;
        if seen > MAXIMUM_BYTES { return Err(Error::InvalidRequest); }
        hash.update(&chunk[..count]);
    }
    if seen != bytes { return Err(Error::InvalidRequest); }
    let digest_bytes = hash.finalize();
    let digest = digest_bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let track_id = (i64::from_be_bytes(digest_bytes[..8].try_into().map_err(|_| Error::InvalidRequest)?) & ((1_i64 << 53) - 1)).max(1);
    file.rewind().map_err(|_| Error::InvalidRequest)?;
    let source = MediaSourceStream::new(Box::new(file.try_clone().map_err(|_| Error::InvalidRequest)?), MediaSourceStreamOptions::default());
    let mut hint = Hint::new(); hint.with_extension(&extension);
    let mut probed = symphonia::default::get_probe().format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|_| Error::InvalidRequest)?;
    let mut metadata = Vec::new();
    if let Some(mut extra) = probed.metadata.get() {
        while let Some(revision) = extra.pop() { metadata.push(revision); }
        if let Some(revision) = extra.current() { metadata.push(revision.clone()); }
    }
    let format = &mut probed.format;
    if let Some(revision) = format.metadata().current() { metadata.push(revision.clone()); }
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or(Error::InvalidRequest)?;
    let audio_id = track.id;
    let parameters = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs().make(&parameters, &DecoderOptions::default()).map_err(|_| Error::InvalidRequest)?;
    let mut playable = false;
    let mut last_timestamp = 0u64;
    let mut packets = 0usize;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(MediaError::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => return Err(Error::InvalidRequest),
        };
        packets += 1;
        if packets > 2_000_000 { return Err(Error::InvalidRequest); }
        if packet.track_id() != audio_id { continue; }
        last_timestamp = last_timestamp.max(packet.ts().saturating_add(packet.dur()));
        if !playable {
            if let Ok(samples) = decoder.decode(&packet) { playable = samples.frames() > 0; }
            if packets >= 64 && !playable { return Err(Error::InvalidRequest); }
        }
    }
    if !playable { return Err(Error::InvalidRequest); }
    let duration = if let Some(base) = parameters.time_base {
        let time = base.calc_time(last_timestamp.max(parameters.n_frames.unwrap_or(0)));
        (time.seconds as f64 + time.frac).ceil().clamp(1., f64::from(u32::MAX)) as u32
    } else { return Err(Error::InvalidRequest); };
    let title = text(&metadata, StandardTagKey::TrackTitle).unwrap_or_else(|| {
        Path::new(display_name).file_stem().unwrap_or_default().to_string_lossy().chars()
            .filter(|c| !c.is_control()).take(500).collect()
    });
    let track = Track { track_id, owner_id: 0, title, artist: text(&metadata, StandardTagKey::Artist).unwrap_or_default(),
        album_title: text(&metadata, StandardTagKey::Album), duration, stream_url: None, artwork_url: None,
        access_key: None, lyrics_id: None, is_hq: false, track_code: None, artists: Vec::new(), album: None };
    drop(probed);
    file.rewind().map_err(|_| Error::InvalidRequest)?;
    Ok(LocalAudio { track, file, bytes, digest, extension })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wav() -> Vec<u8> {
        let samples = 8000u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF"); bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt "); bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes()); bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes()); bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data"); bytes.extend_from_slice(&(samples * 2).to_le_bytes());
        bytes.resize(44 + samples as usize * 2, 0); bytes
    }
    #[test]
    fn import_is_validated_deduplicated_and_keeps_the_original() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Original.wav");
        std::fs::write(&source, wav()).unwrap();
        let inspected = inspect(&source, "Original.wav").unwrap();
        assert!(inspected.track.is_local());
        assert_eq!(inspected.track.title, "Original");
        assert_eq!(inspected.track.duration, 1);
        let store = crate::offline::OfflineStore::open(root.path().join("library")).unwrap();
        let saved = store.import_local(inspected).unwrap();
        let duplicate = store.import_local(inspect(&source, "Renamed.wav").unwrap()).unwrap();
        assert_eq!(saved.track.id(), duplicate.track.id());
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.details(&saved.track.id()).unwrap().format, "WAV");
        assert!(store.details(&saved.track.id()).unwrap().available);
        store.remove(&saved.track.id()).unwrap();
        assert!(source.is_file());
    }
    #[test]
    fn invalid_files_and_vk_zero_owner_records_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("invalid.mp3");
        std::fs::write(&source, b"ID3this is not audio").unwrap();
        assert!(inspect(&source, "invalid.mp3").is_err());
        std::fs::write(&source, wav()).unwrap();
        assert!(inspect(&source, "not-audio.exe").is_err());
        assert!(crate::vk::model::parse_track(&serde_json::json!({"id":1,"owner_id":0,"title":"bad"}), None).is_none());
    }
    #[test]
    fn every_supported_format_imports_and_decodes_through_the_real_offline_path() {
        let fixtures: &[(&str, &[u8])] = &[
            ("mp3", include_bytes!("../tests/fixtures/local-audio/sample.mp3")),
            ("m4a", include_bytes!("../tests/fixtures/local-audio/sample.m4a")),
            ("aac", include_bytes!("../tests/fixtures/local-audio/sample.aac")),
            ("wav", include_bytes!("../tests/fixtures/local-audio/sample.wav")),
            ("flac", include_bytes!("../tests/fixtures/local-audio/sample.flac")),
        ];
        let root = tempfile::tempdir().unwrap();
        let store = crate::offline::OfflineStore::open(root.path().join("library")).unwrap();
        for (extension, data) in fixtures {
            let name = format!("Source.{extension}");
            let path = root.path().join(&name);
            std::fs::write(&path, data).unwrap();
            let imported = inspect(&path, &name).unwrap_or_else(|error| panic!("{extension}: {error}"));
            assert!(imported.track.track_id <= 9_007_199_254_740_991, "identities must survive the JavaScript bridge");
            let saved = store.import_local(imported).unwrap();
            let mut config = crate::stream::FetchConfig::new("Private Music local audio test");
            config.offline_dir = Some(store.dir().to_owned());
            let buffer = crate::stream::SharedBuffer::new(1_000_000);
            let worker = crate::stream::spawn_fetch(store.address(&saved.track.id()).unwrap(), config, buffer.clone());
            let kind = buffer.wait_for_kind().unwrap();
            let mut decoder = crate::stream::Decoder::open(buffer.reader(), kind).unwrap();
            let mut samples = Vec::new();
            while decoder.next_block(&mut samples).unwrap() {}
            assert!(!samples.is_empty() && samples.iter().all(|sample| sample.is_finite()), "{extension}");
            worker.join().unwrap();
        }
    }

    #[test]
    fn copied_content_must_still_match_the_validated_digest() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Original.wav");
        std::fs::write(&source, wav()).unwrap();
        let inspected = inspect(&source, "Original.wav").unwrap();
        let mut changed = wav(); changed[60] = 3;
        std::fs::write(&source, changed).unwrap();
        let store = crate::offline::OfflineStore::open(root.path().join("library")).unwrap();
        assert!(store.import_local(inspected).is_err());
        assert!(store.list().is_empty());
        assert!(std::fs::read_dir(store.dir()).unwrap().next().is_none());
    }
}
