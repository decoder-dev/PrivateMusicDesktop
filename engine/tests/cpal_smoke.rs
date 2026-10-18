//! Открывает реальное устройство вывода по умолчанию. Требуется аудиоустройство (CI использует нулевое
//! устройство ALSA), поэтому оно работает только с `--ignored`.

use pm_engine::audio::{CpalSink, Sink};
use std::time::{Duration, Instant};

fn sine(rate: u32, channels: usize, seconds: f64) -> Vec<f32> {
    let frames = (rate as f64 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * channels);
    for n in 0..frames {
        let v = (2.0 * std::f64::consts::PI * 440.0 * n as f64 / rate as f64).sin() as f32 * 0.2;
        out.extend(std::iter::repeat_n(v, channels));
    }
    out
}

#[test]
#[ignore = "needs an audio output device"]
fn the_real_sink_plays_pauses_flushes_and_reports_position() {
    let mut sink = CpalSink::open_default().expect("an output device");
    let (rate, channels) = (sink.sample_rate(), sink.channels());
    assert!(rate >= 8_000 && channels >= 1);
    assert!(sink.is_healthy());
    let audio = sine(rate, channels, 3.0);

    // Кормите как настоящий игрок: продолжайте писать, пока устройство разряжается.
    let mut written = 0;
    let started = Instant::now();
    while sink.played_frames() < (rate as u64) / 4 {
        let room = sink.writable();
        let end = (written + room).min(audio.len());
        written += sink.write(&audio[written..end]);
        assert!(started.elapsed() < Duration::from_secs(10), "the device never consumed audio");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(sink.played_frames() > 0);

    // Пауза удерживает позицию.
    sink.set_paused(true);
    std::thread::sleep(Duration::from_millis(80));
    let held = sink.played_frames();
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(sink.played_frames(), held, "position moved while paused");
    sink.set_paused(false);

    // Flush удаляет очередь и сбрасывает счетчик; что написано после того, как оно сохранилось.
    sink.flush();
    assert_eq!(sink.queued_frames(), 0);
    assert!(sink.played_frames() < rate as u64 / 10, "counter reset");
    let n = sink.write(&sine(rate, channels, 0.2));
    assert!(n > 0);
    let wait = Instant::now();
    while sink.played_frames() < rate as u64 / 10 {
        assert!(wait.elapsed() < Duration::from_secs(5), "audio written after a flush was lost");
        std::thread::sleep(Duration::from_millis(5));
    }
    sink.set_volume(0.0);
    sink.set_volume(f32::NAN); // должен стать вменяемым выигрышем, а не шумом
    sink.set_volume(f32::INFINITY);
}
