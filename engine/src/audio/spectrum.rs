//! Что показывает визуализатор: короткое окно звука, который сейчас воспроизводится, уменьшенное до
//! нескольких десятков логарифмических полос.
//!
//! Плеер записывает звук на звуковую карту на несколько десятых секунды раньше динамиков. [`Tap`]
//! сохраняет копию этого аудио (моно), поэтому анализ может учитывать момент фактического
//! воспроизведения устройства, а не момент, когда данные были поставлены в очередь. Здесь ничего не
//! запускается, если этого не попросит окно.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const BANDS: usize = 48;
const WINDOW: usize = 2048;
/// Примерно 1,5 секунды при 44,1 kHz: больше, чем когда-либо игрок стоял в очереди вперед.
const RING: usize = 1 << 16;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;
const FLOOR_DB: f32 = -72.0;
const CEILING_DB: f32 = -8.0;

/// Монокопия того, что было записано на звуковую карту с момента последней прошивки.
pub struct Tap {
    ring: Vec<f32>,
    /// Кадры записаны с момента последнего сброса.
    written: u64,
    pub enabled: bool,
}

impl Tap {
    pub fn new(enabled: bool) -> Self {
        Self { ring: Vec::new(), written: 0, enabled }
    }

    /// Далее следует очистка приемника: подсчет кадров снова начинается с нуля.
    pub fn reset(&mut self) {
        self.written = 0;
    }
    /// Включение анализа в середине непрерывного аудиовыхода сохраняет общий счётчик кадров.
    pub fn align(&mut self,frame:u64){self.written=frame;self.ring.fill(0.0);}

    pub fn push(&mut self, interleaved: &[f32], channels: usize) {
        if !self.enabled || channels == 0 {
            return;
        }
        if self.ring.is_empty() {
            self.ring = vec![0.0; RING];
        }
        for frame in interleaved.chunks_exact(channels) {
            let mono = frame.iter().sum::<f32>() / channels as f32;
            self.ring[(self.written % RING as u64) as usize] = mono;
            self.written += 1;
        }
    }

    /// Кадры `WINDOW`, заканчивающиеся на `frame` (счетчик воспроизведений устройства), сначала самые
    /// старые. `None`, когда этой части нет (или больше нет) на ринге.
    fn window(&self, frame: u64) -> Option<[f32; WINDOW]> {
        let end = frame.min(self.written);
        if self.ring.is_empty() || end < WINDOW as u64 || self.written - end > (RING - WINDOW) as u64 {
            return None;
        }
        let mut out = [0.0; WINDOW];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.ring[((end - WINDOW as u64 + i as u64) % RING as u64) as usize];
        }
        Some(out)
    }
}

pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    hann: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// Для каждой полосы охватываются ячейки FFT.
    edges: Vec<(usize, usize)>,
    levels: [f32; BANDS],
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let hann = (0..WINDOW)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos())
            .collect();
        let bin_hz = sample_rate as f32 / WINDOW as f32;
        let top = HIGH_HZ.min(sample_rate as f32 / 2.0 - bin_hz);
        let ratio = (top / LOW_HZ).powf(1.0 / BANDS as f32);
        let edges = (0..BANDS)
            .map(|band| {
                let from = LOW_HZ * ratio.powi(band as i32);
                let to = from * ratio;
                let first = ((from / bin_hz).floor() as usize).max(1);
                let last = ((to / bin_hz).ceil() as usize).max(first + 1).min(WINDOW / 2);
                (first, last)
            })
            .collect();
        Self {
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            hann,
            edges,
            levels: [0.0; BANDS],
        }
    }

    /// Полосы 0…1 для звука, воспроизводимого на `frame`, сглаживаются с течением времени: быстрое
    /// нарастание, медленное спад. Тишина (или окно еще не открыто) позволяет решетке тонуть.
    pub fn analyze(&mut self, tap: &Tap, frame: u64) -> [f32; BANDS] {
        let target = match tap.window(frame) {
            Some(window) => self.raw_bands(&window),
            None => [0.0; BANDS],
        };
        for (level, goal) in self.levels.iter_mut().zip(target) {
            *level = if goal > *level { *level + (goal - *level) * 0.65 } else { *level * 0.82 + goal * 0.18 };
        }
        self.levels
    }

    fn raw_bands(&mut self, window: &[f32; WINDOW]) -> [f32; BANDS] {
        for ((slot, sample), weight) in self.input.iter_mut().zip(window).zip(&self.hann) {
            *slot = sample * weight;
        }
        if self.fft.process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch).is_err() {
            return [0.0; BANDS];
        }
        let mut bands = [0.0; BANDS];
        for (band, &(first, last)) in bands.iter_mut().zip(&self.edges) {
            let peak = self.output[first..last].iter().map(|c| c.norm()).fold(0.0f32, f32::max);
            // Полномасштабный синус дает бин около WINDOW/4 после окна.
            let db = 20.0 * (peak / (WINDOW as f32 / 4.0)).max(1e-9).log10();
            *band = ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0);
        }
        bands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: u32, frames: usize, channels: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = 0.8 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin();
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    fn settled(analyzer: &mut Analyzer, tap: &Tap, frame: u64) -> [f32; BANDS] {
        let mut bands = [0.0; BANDS];
        for _ in 0..30 {
            bands = analyzer.analyze(tap, frame);
        }
        bands
    }

    #[test]
    fn a_sine_lights_one_place_and_higher_notes_land_further_right() {
        let rate = 44_100;
        let peak_band = |hz: f32| {
            let mut tap = Tap::new(true);
            tap.push(&sine(hz, rate, 8192, 2), 2);
            let mut analyzer = Analyzer::new(rate);
            let bands = settled(&mut analyzer, &tap, 8192);
            let (index, level) = bands.iter().enumerate().fold((0, 0.0), |a, (i, &l)| if l > a.1 { (i, l) } else { a });
            assert!(level > 0.7, "{hz} Hz should be loud, got {level}");
            index
        };
        let (low, mid, high) = (peak_band(110.0), peak_band(1000.0), peak_band(8000.0));
        assert!(low < mid && mid < high, "{low} {mid} {high}");
        // Интервал журнала: 1 kHz находится примерно в середине 40 Гц…16 kHz.
        assert!((16..32).contains(&mid), "{mid}");
    }

    #[test]
    fn silence_and_missing_audio_stay_at_zero() {
        let mut analyzer = Analyzer::new(48_000);
        let mut tap = Tap::new(true);
        assert_eq!(settled(&mut analyzer, &tap, 0), [0.0; BANDS]);
        tap.push(&vec![0.0; 8192 * 2], 2);
        assert_eq!(settled(&mut analyzer, &tap, 8192), [0.0; BANDS]);
    }

    #[test]
    fn bars_fall_after_the_sound_stops() {
        let rate = 44_100;
        let mut tap = Tap::new(true);
        tap.push(&sine(1000.0, rate, 4096, 1), 1);
        let mut analyzer = Analyzer::new(rate);
        let loud = settled(&mut analyzer, &tap, 4096).iter().cloned().fold(0.0, f32::max);
        tap.push(&vec![0.0; 8192], 1);
        let first = analyzer.analyze(&tap, 12288).iter().cloned().fold(0.0, f32::max);
        assert!(first < loud && first > 0.0, "decays, does not snap: {first} vs {loud}");
        let later = settled(&mut analyzer, &tap, 12288).iter().cloned().fold(0.0, f32::max);
        assert!(later < 0.01, "{later}");
    }

    #[test]
    fn the_window_follows_what_is_played_not_what_was_queued() {
        let rate = 44_100;
        let mut tap = Tap::new(true);
        // 0,3 с 200 Гц, затем 0,3 с 5 kHz — это queued...
        tap.push(&sine(200.0, rate, 13_000, 1), 1);
        tap.push(&sine(5000.0, rate, 13_000, 1), 1);
        let mut analyzer = Analyzer::new(rate);
        let band = |bands: [f32; BANDS]| bands.iter().enumerate().fold((0, 0.0), |a, (i, &l)| if l > a.1 { (i, l) } else { a }).0;
        // ...но устройство сыграло только первую часть.
        let playing_low = band(settled(&mut analyzer, &tap, 10_000));
        let mut analyzer = Analyzer::new(rate);
        let playing_high = band(settled(&mut analyzer, &tap, 24_000));
        assert!(playing_low < playing_high - 10, "{playing_low} {playing_high}");
    }

    #[test]
    fn a_disabled_tap_costs_nothing_and_a_flush_starts_over() {
        let mut off = Tap::new(false);
        off.push(&[0.5; 4096], 1);
        assert!(off.ring.is_empty() && off.written == 0);
        let mut tap = Tap::new(true);
        tap.push(&[0.5; 4096], 1);
        tap.reset();
        assert!(tap.window(4096).is_none());
    }
}
