//! A ten-band graphic equaliser on the device's audio, before the volume.
//!
//! Peaking biquads (RBJ cookbook) at the usual octave centres. A boost would
//! clip loud music, so the whole signal is lowered by the largest boost
//! (preamp). A linked stereo peak guard handles transients; retuning crossfades
//! complete filter banks instead of abruptly resetting their coefficients.

pub const BANDS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const MAX_GAIN_DB: f32 = 12.0;
const Q: f64 = 1.1;

#[derive(Clone, Copy, Default)]
struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefficients {
    /// Gain in dB at `hz`.
    fn response_db(&self, rate: f64, hz: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * hz / rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).log10()
    }

    fn peaking(rate: f64, centre: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * std::f64::consts::PI * centre / rate;
        let alpha = w.sin() / (2.0 * Q);
        let cos = w.cos();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

pub struct Equalizer {
    filters: Vec<Coefficients>,
    /// One state per band per channel.
    states: Vec<State>,
    channels: usize,
    preamp: f32,
    rate: u32,
    previous: Option<Box<Equalizer>>,
    fade_remaining: usize,
    fade_frames: usize,
    limiter_gain: f32,
    release: f32,
}

impl Equalizer {
    /// `None` when every gain is (about) zero: nothing to do.
    pub fn new(gains_db: &[f32; 10], rate: u32, channels: usize) -> Option<Self> {
        let gains: Vec<f32> =
            gains_db.iter().map(|g| if g.is_finite() { g.clamp(-MAX_GAIN_DB, MAX_GAIN_DB) } else { 0.0 }).collect();
        if gains.iter().all(|g| g.abs() < 0.05) || channels == 0 || rate == 0 {
            return None;
        }
        let rate_f = f64::from(rate);
        let usable: Vec<(f64, f64)> = BANDS
            .iter()
            .zip(&gains)
            .filter(|(centre, _)| f64::from(**centre) < rate_f / 2.0 * 0.9)
            .map(|(centre, gain)| (f64::from(*centre), f64::from(*gain)))
            .collect();
        // Neighbouring bands overlap, so their boosts and cuts add up. Adjust
        // the filter gains until the combined curve passes through what the
        // user set at every band centre (a few rounds are plenty).
        let mut applied: Vec<f64> = usable.iter().map(|(_, g)| *g).collect();
        let build = |applied: &[f64]| -> Vec<Coefficients> {
            usable.iter().zip(applied).map(|((centre, _), g)| Coefficients::peaking(rate_f, *centre, *g)).collect()
        };
        let total = |filters: &[Coefficients], hz: f64| filters.iter().map(|f| f.response_db(rate_f, hz)).sum::<f64>();
        for _ in 0..12 {
            let filters = build(&applied);
            for (i, (centre, wanted)) in usable.iter().enumerate() {
                applied[i] += (wanted - total(&filters, *centre)) * 0.7;
                applied[i] = applied[i].clamp(-2.0 * f64::from(MAX_GAIN_DB), 2.0 * f64::from(MAX_GAIN_DB));
            }
        }
        let filters = build(&applied);
        // Lower everything by the loudest point of the curve, so no boost clips.
        let peak = (0..1025)
            .map(|i| 10.0 * (rate_f * 0.049).powf(f64::from(i) / 1024.0)) // 10 Hz … 98% of Nyquist
            .filter(|hz| *hz < rate_f / 2.0)
            .map(|hz| total(&filters, hz))
            .fold(0.0f64, f64::max);
        let peak = if peak < 1.0 { 0.0 } else { peak + 0.25 };
        Some(Self {
            states: vec![State::default(); filters.len() * channels],
            filters,
            channels,
            preamp: 10f64.powf(-peak / 20.0) as f32,
            rate,
            previous: None,
            fade_remaining: 0,
            fade_frames: 1,
            limiter_gain: 1.0,
            release: (-1.0 / (0.08 * rate as f32)).exp(),
        })
    }

    /// Crossfade over 30 ms, including enabling and bypassing the equalizer.
    pub fn transition(previous: Option<Self>, gains: Option<&[f32; 10]>, rate: u32, channels: usize) -> Option<Self> {
        if rate == 0 || channels == 0 {
            return None;
        }
        let previous = previous.filter(|old| old.channels == channels && old.rate == rate);
        let next = gains.and_then(|g| Self::new(g, rate, channels));
        if previous.is_none() && next.is_none() {
            return None;
        }
        let identity = || Self {
            filters: Vec::new(),
            states: Vec::new(),
            channels,
            preamp: 1.0,
            rate,
            previous: None,
            fade_remaining: 0,
            fade_frames: 1,
            limiter_gain: 1.0,
            release: (-1.0 / (0.08 * rate as f32)).exp(),
        };
        let mut next = next.unwrap_or_else(identity);
        let mut old = previous.unwrap_or_else(identity);
        next.limiter_gain = old.limiter_gain;
        old.previous = None; // rapid retunes cannot accumulate filter banks
        next.fade_frames = (rate as usize * 30 / 1000).max(1);
        next.fade_remaining = next.fade_frames;
        next.previous = Some(Box::new(old));
        Some(next)
    }

    pub fn reset(&mut self) {
        self.states.fill(State::default());
        self.previous = None;
        self.fade_remaining = 0;
        self.limiter_gain = 1.0;
    }

    fn sample(&mut self, channel: usize, input: f32) -> f32 {
        let mut x = f64::from(input * self.preamp);
        for (band, c) in self.filters.iter().enumerate() {
            let s = &mut self.states[band * self.channels + channel];
            let y = c.b0 * x + c.b1 * s.x1 + c.b2 * s.x2 - c.a1 * s.y1 - c.a2 * s.y2;
            s.x2 = s.x1;
            s.x1 = x;
            s.y2 = s.y1;
            s.y1 = y;
            x = y;
        }
        if x.is_finite() {
            x as f32
        } else {
            self.reset();
            0.0
        }
    }

    /// Filters interleaved samples without allocation in the audio path.
    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(self.channels) {
            let mix = 1.0 - self.fade_remaining as f32 / self.fade_frames as f32;
            for (channel, sample) in frame.iter_mut().enumerate() {
                let dry = if sample.is_finite() { *sample } else { 0.0 };
                let wet = self.sample(channel, dry);
                *sample = if let Some(old) = self.previous.as_mut() {
                    old.sample(channel, dry) * (1.0 - mix) + wet * mix
                } else {
                    wet
                };
            }
            let peak = frame.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            let target = if peak > 1.0 { 1.0 / peak } else { 1.0 };
            self.limiter_gain = if target < self.limiter_gain {
                target
            } else {
                self.release * self.limiter_gain + (1.0 - self.release) * target
            };
            for sample in frame {
                *sample = (*sample * self.limiter_gain).clamp(-1.0, 1.0);
            }
            if self.fade_remaining > 0 {
                self.fade_remaining -= 1;
                if self.fade_remaining == 0 {
                    self.previous = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .flat_map(|i| {
                let v = 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin();
                [v, v]
            })
            .collect()
    }

    /// Gain in dB of the equaliser at `hz`, measured on the settled half.
    fn response(gains: [f32; 10], hz: f32) -> f32 {
        let rate = 48_000;
        let mut eq = Equalizer::new(&gains, rate, 2).unwrap();
        let mut samples = tone(hz, rate, 0.5);
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        let before = rms(&samples[samples.len() / 2..]);
        eq.process(&mut samples);
        20.0 * (rms(&samples[samples.len() / 2..]) / before).log10()
    }

    #[test]
    fn the_curve_passes_through_the_settings_even_when_bands_overlap() {
        // All bands down 6 dB: about 6 dB quieter everywhere, not 10.
        for hz in [62.0, 250.0, 1000.0, 4000.0] {
            let got = response([-6.0; 10], hz);
            assert!((got + 6.0).abs() < 1.0, "{hz} Hz: {got:.1} dB");
        }
        // A smile: low and high up, middle flat; after the preamp the middle
        // sits the boost below the ends.
        let smile = [8.0, 6.0, 3.0, 0.0, 0.0, 0.0, 0.0, 3.0, 6.0, 8.0];
        let (low, mid) = (response(smile, 31.0), response(smile, 1000.0));
        assert!((low - mid - 8.0).abs() < 1.5, "low {low:.1} mid {mid:.1}");
    }

    #[test]
    fn flat_settings_do_nothing() {
        assert!(Equalizer::new(&[0.0; 10], 48_000, 2).is_none());
        assert!(Equalizer::new(&[0.01; 10], 48_000, 2).is_none());
    }

    #[test]
    fn a_boosted_band_stands_out_by_its_gain_and_never_gets_louder() {
        let mut gains = [0.0; 10];
        gains[5] = 12.0; // 1 kHz
        let at_band = response(gains, 1000.0);
        let far = response(gains, 100.0);
        assert!((at_band - far - 12.0).abs() < 1.5, "1 kHz {at_band:.1} dB vs 100 Hz {far:.1} dB");
        assert!(at_band < 0.5, "the preamp keeps the peak at unity: {at_band:.1}");
    }

    #[test]
    fn a_cut_lowers_its_band_only() {
        let mut gains = [0.0; 10];
        gains[1] = -9.0; // 62 Hz
        assert!(response(gains, 62.0) < -7.5);
        let far = response(gains, 4000.0);
        assert!(far.abs() < 0.5, "4 kHz moved {far:.2} dB");
    }

    #[test]
    fn hostile_values_are_clamped_and_stay_stable() {
        let gains = [f32::NAN, f32::INFINITY, -1000.0, 1000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut eq = Equalizer::new(&gains, 44_100, 2).unwrap();
        let mut samples = tone(62.0, 44_100, 1.0);
        eq.process(&mut samples);
        assert!(samples.iter().all(|v| v.is_finite() && v.abs() <= 1.5));
    }

    #[test]
    fn bands_above_the_rate_limit_are_skipped() {
        let mut gains = [0.0; 10];
        gains[9] = 6.0; // 16 kHz cannot be filtered at 22.05 kHz
        let mut eq = Equalizer::new(&gains, 22_050, 1).unwrap();
        let mut samples = vec![0.1f32; 1000];
        eq.process(&mut samples);
        assert!(samples.iter().all(|v| v.is_finite()));
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn bypass_and_retune_start_at_the_previous_output() {
        let mut eq = Equalizer::transition(None, Some(&[-12.0; 10]), 48_000, 2).unwrap();
        let mut first = [0.5, -0.25];
        eq.process(&mut first);
        assert!((first[0] - 0.5).abs() < 0.00001 && (first[1] + 0.25).abs() < 0.00001);
        eq.process(&mut vec![0.1; 4000]);
        let mut bypass = Equalizer::transition(Some(eq), None, 48_000, 2).unwrap();
        bypass.process(&mut vec![0.1; 4000]);
        let mut frame = [0.5, -0.25];
        bypass.process(&mut frame);
        assert_eq!(frame, [0.5, -0.25]);
        assert!(bypass.previous.is_none());
    }
    #[test]
    fn transient_guard_is_finite_bounded_and_stereo_linked() {
        let mut eq =
            Equalizer::new(&[12.0, -12.0, 12.0, -12.0, 12.0, -12.0, 12.0, -12.0, 12.0, -12.0], 44_100, 2).unwrap();
        let mut input: Vec<_> = (0..44100)
            .flat_map(|i| {
                let v = if i % 7 < 3 { 1.0 } else { -1.0 };
                [v, v * 0.5]
            })
            .collect();
        eq.process(&mut input);
        for f in input.as_chunks::<2>().0 {
            assert!(f.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
            assert!((f[0] * 0.5 - f[1]).abs() < 0.00001);
        }
        eq.process(&mut [f32::NAN, f32::INFINITY]);
        let mut silence = [0.0; 128];
        eq.reset();
        eq.process(&mut silence);
        assert!(silence.iter().all(|v| *v == 0.0));
    }
}
