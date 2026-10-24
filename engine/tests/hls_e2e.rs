//! Комплексно: ffmpeg создает реальные потоки, локальный сервер передает их, движок загружает,
//! расшифровывает, демультиплексирует и декодирует их, а мы измеряем выходной звук.

mod common;

use std::fs;

use common::*;
use pm_engine::stream::{spawn_fetch, Decoder, SharedBuffer};

fn assert_two_tones(pcm: &Pcm) {
    assert_eq!(pcm.rate, 44_100);
    assert_eq!(pcm.channels, 2);
    assert!((pcm.seconds() - 12.0).abs() < 0.25, "duration {}", pcm.seconds());
    assert!(pcm.rms() > 0.05, "signal is silent: {}", pcm.rms());
    let low = pcm.frequency(1.0, 5.0);
    let high = pcm.frequency(7.0, 11.0);
    assert!((low - 440.0).abs() < 8.0, "first half is {low} Hz");
    assert!((high - 880.0).abs() < 16.0, "second half is {high} Hz");
}

#[test]
fn decoder_preserves_network_errors_and_server_cooldowns_after_receiving_audio() {
    if !require_ffmpeg() { return; }
    let dir=tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(),&tone_args(&["-c:a","libmp3lame","-b:a","128k","tone.mp3"]));
    let bytes=fs::read(dir.path().join("tone.mp3")).unwrap();
    for error in [pm_engine::Error::Offline,pm_engine::Error::Timeout,pm_engine::Error::RetryAfter{status:429,seconds:120}] {
        let buffer=SharedBuffer::new(bytes.len()*2);
        buffer.set_kind(pm_engine::stream::ElementaryKind::Mp3);
        buffer.push(&bytes[..bytes.len()/2]);buffer.fail(error.clone());
        let mut decoder=Decoder::open(buffer.reader(),pm_engine::stream::ElementaryKind::Mp3).unwrap();
        let mut samples=Vec::new();
        let result=loop {match decoder.next_block(&mut samples) { Ok(true)=>{},Ok(false)=>panic!("network failure must not become end-of-track"),Err(error)=>break error}};
        assert!(!samples.is_empty(),"some audio must have played before the failure");
        assert_eq!(result,error);
    }
}

/// Зашифровано MPEG-TS HLS. `explicit_iv` выбирает форму IV в ключевой информации.
fn encrypted_hls(explicit_iv: bool) -> (tempfile::TempDir, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let key: Vec<u8> = (0..16u8).map(|i| i.wrapping_mul(17).wrapping_add(3)).collect();
    fs::write(dir.path().join("key.bin"), &key).unwrap();
    let iv_line = if explicit_iv { "\n00112233445566778899aabbccddeeff" } else { "" };
    fs::write(dir.path().join("key.info"), format!("key.bin\n{}/key.bin{}\n", dir.path().display(), iv_line)).unwrap();
    let args = tone_args(&[
        "-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_key_info_file", "key.info", "-hls_segment_filename", "seg%d.ts", "index.m3u8",
    ]);
    run_ffmpeg(dir.path(), &args);
    (dir, key)
}

#[test]
fn encrypted_ts_hls_with_sequence_iv() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(false);
    let playlist = fs::read_to_string(dir.path().join("index.m3u8")).unwrap();
    assert!(playlist.contains("#EXT-X-KEY:METHOD=AES-128"), "{playlist}");
    let server = Server::start(dir.path().to_path_buf());
    let pcm = decode_all(&format!("{}/index.m3u8", server.base)).unwrap();
    assert_two_tones(&pcm);
}

#[test]
fn encrypted_ts_hls_with_explicit_iv() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(true);
    let playlist = fs::read_to_string(dir.path().join("index.m3u8")).unwrap();
    assert!(playlist.contains("IV=0x"), "{playlist}");
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/index.m3u8", server.base)).unwrap());
}

#[test]
fn wrong_key_does_not_produce_audio() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(false);
    fs::write(dir.path().join("key.bin"), [0u8; 16]).unwrap();
    let server = Server::start(dir.path().to_path_buf());
    assert!(decode_all(&format!("{}/index.m3u8", server.base)).is_err());
}

#[test]
fn key_of_the_wrong_length_is_rejected() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(false);
    fs::write(dir.path().join("key.bin"), [1u8; 15]).unwrap();
    let server = Server::start(dir.path().to_path_buf());
    assert!(decode_all(&format!("{}/index.m3u8", server.base)).is_err());
}

#[test]
fn unencrypted_ts_hls() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    let args = tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]);
    run_ffmpeg(dir.path(), &args);
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/index.m3u8", server.base)).unwrap());
}

#[test]
fn master_playlist_picks_the_audio_variant() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(false);
    fs::write(
        dir.path().join("master.m3u8"),
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=96000,CODECS=\"mp4a.40.2\"\nindex.m3u8\n",
    )
    .unwrap();
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/master.m3u8", server.base)).unwrap());
}

#[test]
fn live_fetch_honors_the_hls_quality_preference() {
    if !require_ffmpeg() { return; }
    let dir=tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(),&tone_args(&["-c:a","aac","-b:a","96k","-f","hls","-hls_time","2","-hls_list_size","0","-hls_segment_filename","low%d.ts","low.m3u8"]));
    fs::write(dir.path().join("master.m3u8"),"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=96000,CODECS=\"mp4a.40.2\"\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=320000,CODECS=\"mp4a.40.2\"\nunavailable.m3u8\n").unwrap();
    let server=Server::start(dir.path().to_path_buf());let url=format!("{}/master.m3u8",server.base);
    let mut config=lab_config();config.prefer_high_quality=false;
    assert!(pm_engine::stream::fetch::fetch_to_end(&url,&config).is_ok());
    config.prefer_high_quality=true;
    assert_eq!(pm_engine::stream::fetch::fetch_to_end(&url,&config).unwrap_err(),pm_engine::Error::Http(404));
}

#[test]
fn progressive_mp3_and_adts_files() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "libmp3lame", "-b:a", "128k", "tone.mp3"]));
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "adts", "tone.aac"]));
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/tone.mp3", server.base)).unwrap());
    assert_two_tones(&decode_all(&format!("{}/tone.aac", server.base)).unwrap());
}

#[test]
fn id3_tagged_mp3_is_played_from_the_audio() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "libmp3lame", "-b:a", "128k", "-id3v2_version", "3", "-metadata", "title=Тест", "tone.mp3"]));
    let bytes = read_file(&dir.path().join("tone.mp3"));
    assert_eq!(&bytes[..3], b"ID3");
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/tone.mp3", server.base)).unwrap());
}

#[test]
fn mp3_with_large_and_repeated_id3_tags_reaches_the_audio() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(dir.path(), &tone_args(&["-c:a", "libmp3lame", "-b:a", "128k", "tone.mp3"]));
    let audio = fs::read(dir.path().join("tone.mp3")).unwrap();
    // Размер превышает первый сетевой блок. Два тега проверяют последовательное
    // удаление метаданных без потери первого звукового кадра.
    let size = 96 * 1024usize;
    let mut tagged = Vec::new();
    for _ in 0..2 {
        tagged.extend_from_slice(b"ID3\x04\0\0");
        tagged.extend_from_slice(&[
            ((size >> 21) & 127) as u8,
            ((size >> 14) & 127) as u8,
            ((size >> 7) & 127) as u8,
            (size & 127) as u8,
        ]);
        tagged.resize(tagged.len() + size, 0);
    }
    tagged.extend_from_slice(&audio);
    fs::write(dir.path().join("large.mp3"), tagged).unwrap();
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/large.mp3", server.base)).unwrap());
}

#[test]
fn truncated_id3_metadata_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("broken.mp3"), b"ID3\x04\0\0\0\x01\0\0incomplete").unwrap();
    let server = Server::start(dir.path().to_path_buf());
    assert!(decode_all(&format!("{}/broken.mp3", server.base)).is_err());
}

#[test]
fn byte_range_segments_in_one_file() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    let args = tone_args(&[
        "-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0",
        "-hls_flags", "single_file", "-hls_segment_filename", "all.ts", "index.m3u8",
    ]);
    run_ffmpeg(dir.path(), &args);
    let playlist = fs::read_to_string(dir.path().join("index.m3u8")).unwrap();
    assert!(playlist.contains("#EXT-X-BYTERANGE"), "{playlist}");
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/index.m3u8", server.base)).unwrap());
}

#[test]
fn seeking_lands_on_the_requested_time() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(false);
    let server = Server::start(dir.path().to_path_buf());
    let url = format!("{}/index.m3u8", server.base);

    let buffer = SharedBuffer::new(64 * 1024 * 1024);
    let _fetch = spawn_fetch(url, lab_config(), buffer.clone());
    let kind = buffer.wait_for_kind().unwrap();
    let mut decoder = Decoder::open(buffer.reader(), kind).unwrap();

    let read_seconds = |decoder: &mut Decoder, seconds: f64| {
        let mut samples = Vec::new();
        let wanted = (seconds * decoder.sample_rate as f64) as usize * decoder.channels;
        while samples.len() < wanted {
            assert!(decoder.next_block(&mut samples).unwrap(), "stream ended early");
        }
        Pcm { rate: decoder.sample_rate, channels: decoder.channels, samples }
    };

    // Середина второго тона.
    decoder.seek(9.0).unwrap();
    let after = read_seconds(&mut decoder, 1.0);
    assert!((after.frequency(0.0, 1.0) - 880.0).abs() < 16.0);

    // Вернитесь к первому тону.
    decoder.seek(2.0).unwrap();
    let after = read_seconds(&mut decoder, 1.0);
    assert!((after.frequency(0.0, 1.0) - 440.0).abs() < 8.0);

    // Прямо при соединении: за 100 мс до изменения звук все еще имеет частоту 440 Гц.
    decoder.seek(5.9).unwrap();
    let after = read_seconds(&mut decoder, 0.6);
    assert!((after.frequency(0.0, 0.07) - 440.0).abs() < 20.0, "before the join: {}", after.frequency(0.0, 0.07));
    assert!((after.frequency(0.15, 0.55) - 880.0).abs() < 20.0, "after the join: {}", after.frequency(0.15, 0.55));
}

#[test]
fn playback_can_start_before_the_download_finishes() {
    if !require_ffmpeg() { return; }
    let (dir, _) = encrypted_hls(false);
    let server = Server::start(dir.path().to_path_buf());
    let buffer = SharedBuffer::new(64 * 1024 * 1024);
    let fetch = spawn_fetch(format!("{}/index.m3u8", server.base), lab_config(), buffer.clone());
    let kind = buffer.wait_for_kind().unwrap();
    let mut decoder = Decoder::open(buffer.reader(), kind).unwrap();
    let mut samples = Vec::new();
    assert!(decoder.next_block(&mut samples).unwrap());
    fetch.join().unwrap();
    assert!(buffer.is_complete());
}

#[test]
fn fragmented_mp4_hls() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    let args = tone_args(&[
        "-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_segment_type", "fmp4", "-hls_fmp4_init_filename", "init.mp4",
        "-hls_time", "2", "-hls_list_size", "0", "-hls_segment_filename", "seg%d.m4s", "index.m3u8",
    ]);
    run_ffmpeg(dir.path(), &args);
    let playlist = fs::read_to_string(dir.path().join("index.m3u8")).unwrap();
    assert!(playlist.contains("#EXT-X-MAP"), "{playlist}");
    let server = Server::start(dir.path().to_path_buf());
    assert_two_tones(&decode_all(&format!("{}/index.m3u8", server.base)).unwrap());
}

#[test]
fn cancelling_never_hangs_the_download_or_a_reader() {
    if !require_ffmpeg() { return; }
    use std::io::Read;
    let (dir, _) = encrypted_hls(false);
    let server = Server::start(dir.path().to_path_buf());
    let buffer = SharedBuffer::new(64 * 1024 * 1024);
    let fetch = spawn_fetch(format!("{}/index.m3u8", server.base), lab_config(), buffer.clone());
    buffer.cancel();
    fetch.join().unwrap();
    assert!(buffer.is_cancelled());
    // Все, что поступило до отмены, можно прочитать; тогда читатель выдает ошибку вместо того, чтобы
    // ждать вечно.
    let mut sink = Vec::new();
    let _ = buffer.reader().read_to_end(&mut sink);
}

#[test]
fn http_errors_and_garbage_are_reported_not_hung() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("junk.bin"), b"this is not audio at all, not even close").unwrap();
    fs::write(dir.path().join("empty.m3u8"), "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-ENDLIST\n").unwrap();
    let server = Server::start(dir.path().to_path_buf());
    assert!(decode_all(&format!("{}/missing.m3u8", server.base)).is_err());
    assert!(decode_all(&format!("{}/junk.bin", server.base)).is_err());
    assert!(decode_all(&format!("{}/empty.m3u8", server.base)).is_err());
}

#[test]
fn plain_http_to_a_real_host_is_refused() {
    let buffer = SharedBuffer::new(1 << 20);
    let fetch = spawn_fetch("http://example.com/a/index.m3u8".into(), pm_engine::stream::FetchConfig::new("ua"), buffer.clone());
    fetch.join().unwrap();
    assert!(buffer.wait_for_kind().is_err());
}
