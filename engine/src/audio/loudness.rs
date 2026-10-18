//! Оценка воспринимаемой громкости: K-взвешивание, окна 400 мс и абсолютный/относительный пороги.
//! В кэше остаются только численная поправка и длительность, без метаданных и адресов потока.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Default)]
struct Filter {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}
impl Filter {
    fn shelf(rate: f64) -> Self {
        let a = 10f64.powf(4.0 / 40.0);
        let w = 2.0 * std::f64::consts::PI * 1681.974450955533 / rate;
        let c = w.cos();
        let alpha = w.sin() / (2.0 * 0.7071752369554196);
        let x = 2.0 * a.sqrt() * alpha;
        let d = (a + 1.0) - (a - 1.0) * c + x;
        Self {
            b: [
                a * ((a + 1.0) + (a - 1.0) * c + x) / d,
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c) / d,
                a * ((a + 1.0) + (a - 1.0) * c - x) / d,
            ],
            a: [
                2.0 * ((a - 1.0) - (a + 1.0) * c) / d,
                ((a + 1.0) - (a - 1.0) * c - x) / d,
            ],
            z: [0.0; 2],
        }
    }
    fn highpass(rate: f64) -> Self {
        let k = (std::f64::consts::PI * 38.13547087602444 / rate).tan();
        let q = 0.5003270373238773;
        let d = 1.0 + k / q + k * k;
        Self {
            b: [1.0 / d, -2.0 / d, 1.0 / d],
            a: [2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d],
            z: [0.0; 2],
        }
    }
    fn sample(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}
pub struct Meter {
    filters: Vec<[Filter; 2]>,
    window: std::collections::VecDeque<f64>,
    sum: f64,
    frames: usize,
    window_frames: usize,
    hop: usize,
    blocks: Vec<f64>,
    peak: f32,
}
impl Meter {
    pub fn new(rate: u32, channels: usize) -> Self {
        Self {
            filters: vec![
                [
                    Filter::shelf(f64::from(rate)),
                    Filter::highpass(f64::from(rate))
                ];
                channels
            ],
            window: std::collections::VecDeque::new(),
            sum: 0.0,
            frames: 0,
            window_frames: (f64::from(rate) * 0.4) as usize,
            hop: (rate as usize / 10).max(1),
            blocks: Vec::new(),
            peak: 0.0,
        }
    }
    pub fn push(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(self.filters.len()) {
            let mut energy = 0.0;
            for (sample, filters) in frame.iter().zip(&mut self.filters) {
                let x = if sample.is_finite() { *sample } else { 0.0 };
                self.peak = self.peak.max(x.abs());
                let shelf = filters[0].sample(f64::from(x));
                let y = filters[1].sample(shelf);
                energy += y * y;
            }
            self.window.push_back(energy);
            self.sum += energy;
            if self.window.len() > self.window_frames {
                self.sum -= self.window.pop_front().unwrap_or_default();
            }
            self.frames += 1;
            if self.frames.is_multiple_of(self.hop)
                && self.window.len() == self.window_frames
                && self.blocks.len() < 72000
            {
                self.blocks
                    .push((self.sum / self.window_frames as f64).max(0.0));
            }
        }
    }
    pub fn gain(&self) -> Option<f32> {
        let absolute = 10f64.powf((-70.0 + 0.691) / 10.0);
        let initial: Vec<_> = self
            .blocks
            .iter()
            .copied()
            .filter(|v| v.is_finite() && *v > absolute)
            .collect();
        if initial.is_empty() {
            return None;
        }
        let mean = initial.iter().sum::<f64>() / initial.len() as f64;
        let relative = mean * 0.1;
        let gated: Vec<_> = initial.into_iter().filter(|v| *v >= relative).collect();
        if gated.is_empty() {
            return None;
        }
        let lufs = -0.691 + 10.0 * (gated.iter().sum::<f64>() / gated.len() as f64).log10();
        let gain = 10f64.powf((-16.0 - lufs) / 20.0) as f32;
        Some(gain.clamp(0.25, 2.0).min(0.98 / self.peak.max(0.0001)))
    }
}
#[derive(Serialize, Deserialize)]
struct Entry {
    version: u32,
    duration: f64,
    gain: f32,
}
pub struct Normalizer {
    meter: Meter,
    cached: Option<f32>,
    gain: f32,
    path: Option<PathBuf>,
    duration: f64,
    complete: bool,
    channels: usize,
    step: f32,
    target: f32,
    update_frames: usize,
    rate: u32,
}
impl Normalizer {
    pub fn new(
        rate: u32,
        channels: usize,
        path: Option<PathBuf>,
        duration: f64,
        start: f64,
    ) -> Self {
        let cached = path.as_ref().and_then(|path| {
            if std::fs::metadata(path).ok()?.len() > 1024 {
                return None;
            }
            let bytes = std::fs::read(path).ok()?;
            if bytes.len() > 1024 {
                return None;
            }
            let entry: Entry = serde_json::from_slice(&bytes).ok()?;
            (entry.version == 1
                && (entry.duration - duration).abs() < 0.1
                && entry.gain.is_finite()
                && (0.0..=2.0).contains(&entry.gain))
            .then_some(entry.gain)
        });
        Self {
            meter: Meter::new(rate, channels),
            cached,
            gain: cached.unwrap_or(1.0),
            path,
            duration,
            complete: start == 0.0,
            channels,
            step: 1.0 - (-1.0 / (0.15 * rate as f32)).exp(),
            target: cached.unwrap_or(1.0),
            update_frames: 0,
            rate,
        }
    }
    pub fn seeked(&mut self) {
        self.complete = false;
    }
    pub fn process(&mut self, samples: &mut [f32]) {
        self.meter.push(samples);
        if self.cached.is_none()
            && self.meter.frames.saturating_sub(self.update_frames) >= self.rate as usize
        {
            self.target = self.meter.gain().unwrap_or(1.0);
            self.update_frames = self.meter.frames;
        }
        let target = self.target;
        for frame in samples.chunks_exact_mut(self.channels) {
            self.gain += (target - self.gain) * self.step;
            let peak = frame.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            let gain = self.gain.min(0.98 / peak.max(0.0001));
            for sample in frame {
                *sample = if sample.is_finite() {
                    *sample * gain
                } else {
                    0.0
                };
            }
        }
    }
    pub fn finish(&self) {
        if !self.complete || self.cached.is_some() {
            return;
        }
        let (Some(path), Some(gain)) = (&self.path, self.meter.gain()) else {
            return;
        };
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&Entry {
            version: 1,
            duration: self.duration,
            gain,
        }) else {
            return;
        };
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        if std::fs::write(&temporary, bytes).is_err() {
            return;
        }
        // Это восстанавливаемый кэш анализа; Windows не заменяет существующий файл через rename.
        if std::fs::rename(&temporary, path).is_err() {
            let _ = std::fs::remove_file(path);
            if std::fs::rename(&temporary, path).is_err() {
                let _ = std::fs::remove_file(temporary);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sine(amplitude: f32) -> Vec<f32> {
        (0..48000 * 4)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 48000.0).sin() * amplitude)
            .collect()
    }
    #[test]
    fn level_is_independent_of_source_amplitude() {
        let mut quiet = Meter::new(48000, 1);
        quiet.push(&sine(0.2));
        let mut loud = Meter::new(48000, 1);
        loud.push(&sine(0.8));
        assert!((quiet.gain().unwrap() * 0.2 - loud.gain().unwrap() * 0.8).abs() < 0.001);
    }
    #[test]
    fn silence_and_nonfinite_samples_never_produce_invalid_gain() {
        let mut meter = Meter::new(48000, 2);
        meter.push(&vec![f32::NAN; 96000]);
        assert_eq!(meter.gain(), None);
    }
    #[test]
    fn complete_analysis_is_cached_but_a_seek_never_saves_a_partial_result() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gain.json");
        let mut normalizer = Normalizer::new(48000, 1, Some(path.clone()), 4.0, 0.0);
        normalizer.process(&mut sine(0.2));
        normalizer.finish();
        assert!(path.is_file());
        assert!(Normalizer::new(48000, 1, Some(path), 4.0, 0.0)
            .cached
            .is_some());
        let partial = dir.path().join("partial.json");
        let mut normalizer = Normalizer::new(48000, 1, Some(partial.clone()), 4.0, 0.0);
        normalizer.process(&mut sine(0.2));
        normalizer.seeked();
        normalizer.finish();
        assert!(!partial.exists());
    }
}
