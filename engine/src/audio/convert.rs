use rubato::{FftFixedIn, Resampler};

use crate::{Error, Result};

/// Channel layout and sample-rate conversion between a decoded stream and the
/// device. Input and output are interleaved f32.
pub struct Converter {
    in_channels: usize,
    out_channels: usize,
    resampler: Option<FftFixedIn<f32>>,
    /// Per-channel samples waiting for a full resampler chunk.
    pending: Vec<Vec<f32>>,
}

const CHUNK_FRAMES: usize = 1024;

/// Maps one interleaved frame of `input` onto `out_channels`.
fn map_channels(input: &[f32], in_channels: usize, out_channels: usize, out: &mut Vec<f32>) {
    if in_channels == out_channels {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(in_channels) {
        match (in_channels, out_channels) {
            (1, n) => out.extend(std::iter::repeat_n(frame[0], n)),
            (_, 1) => out.push(frame.iter().take(2).sum::<f32>() / frame.len().min(2) as f32),
            (_, n) => {
                // Keep front left/right, silence anything the device has more of.
                out.extend((0..n).map(|channel| if channel < 2 { frame[channel] } else { 0.0 }));
            }
        }
    }
}

impl Converter {
    pub fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Result<Self> {
        if in_channels == 0 || out_channels == 0 || in_rate == 0 || out_rate == 0 {
            return Err(Error::Stream("invalid audio format".into()));
        }
        let resampler = if in_rate == out_rate {
            None
        } else {
            Some(
                FftFixedIn::<f32>::new(in_rate as usize, out_rate as usize, CHUNK_FRAMES, 2, out_channels)
                    .map_err(|e| Error::Stream(format!("resampler: {e}")))?,
            )
        };
        Ok(Self { in_channels, out_channels, resampler, pending: vec![Vec::new(); out_channels] })
    }

    /// Forget buffered samples (after a seek).
    pub fn reset(&mut self) {
        for channel in &mut self.pending {
            channel.clear();
        }
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
    }

    /// Converts `input` and appends the result to `out`. Output may lag the
    /// input by up to one resampler chunk.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<()> {
        let usable = input.len() - input.len() % self.in_channels;
        let mut mapped = Vec::with_capacity(usable / self.in_channels * self.out_channels);
        map_channels(&input[..usable], self.in_channels, self.out_channels, &mut mapped);
        let Some(resampler) = &mut self.resampler else {
            out.extend_from_slice(&mapped);
            return Ok(());
        };
        for frame in mapped.chunks_exact(self.out_channels) {
            for (channel, sample) in frame.iter().enumerate() {
                self.pending[channel].push(*sample);
            }
        }
        loop {
            let needed = resampler.input_frames_next();
            if self.pending[0].len() < needed {
                return Ok(());
            }
            let block: Vec<Vec<f32>> = self.pending.iter_mut().map(|c| c.drain(..needed).collect()).collect();
            let resampled = resampler.process(&block, None).map_err(|e| Error::Stream(format!("resampler: {e}")))?;
            let frames = resampled[0].len();
            for frame in 0..frames {
                for channel in &resampled {
                    out.push(channel[frame]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, channels: usize, seconds: f64, hz: f64) -> Vec<f32> {
        let frames = (rate as f64 * seconds) as usize;
        let mut out = Vec::with_capacity(frames * channels);
        for n in 0..frames {
            let v = (2.0 * std::f64::consts::PI * hz * n as f64 / rate as f64).sin() as f32 * 0.5;
            out.extend(std::iter::repeat_n(v, channels));
        }
        out
    }

    fn crossings(samples: &[f32], channels: usize) -> usize {
        let mut count = 0;
        let mut previous = samples[0];
        for frame in samples.chunks_exact(channels).skip(1) {
            if (previous < 0.0) != (frame[0] < 0.0) {
                count += 1;
            }
            previous = frame[0];
        }
        count
    }

    #[test]
    fn same_format_is_a_copy() {
        let mut c = Converter::new(48_000, 2, 48_000, 2).unwrap();
        let input = sine(48_000, 2, 0.1, 440.0);
        let mut out = Vec::new();
        c.process(&input, &mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn resampling_keeps_pitch_and_length() {
        let mut c = Converter::new(44_100, 2, 48_000, 2).unwrap();
        let input = sine(44_100, 2, 2.0, 1000.0);
        let mut out = Vec::new();
        for block in input.chunks(4096) {
            c.process(block, &mut out).unwrap();
        }
        let seconds = out.len() as f64 / 2.0 / 48_000.0;
        assert!((seconds - 2.0).abs() < 0.05, "length {seconds}");
        // Skip the filter warm-up, then count cycles: 1000 Hz -> 2000 crossings/s.
        let body = &out[2 * 4800..];
        let rate = crossings(body, 2) as f64 / (body.len() as f64 / 2.0 / 48_000.0);
        assert!((rate - 2000.0).abs() < 20.0, "crossings/s {rate}");
    }

    #[test]
    fn channel_mapping() {
        let mut out = Vec::new();
        map_channels(&[0.5, 0.25], 1, 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5, 0.25, 0.25]);
        out.clear();
        map_channels(&[0.2, 0.4], 2, 1, &mut out);
        assert!((out[0] - 0.3).abs() < 1e-6);
        out.clear();
        map_channels(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 6, 2, &mut out);
        assert_eq!(out, vec![1.0, 2.0]);
        out.clear();
        map_channels(&[1.0, 2.0], 2, 4, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 0.0, 0.0]);
    }

    #[test]
    fn invalid_formats_are_rejected() {
        assert!(Converter::new(0, 2, 48_000, 2).is_err());
        assert!(Converter::new(44_100, 0, 48_000, 2).is_err());
    }

    #[test]
    fn reset_drops_buffered_samples() {
        let mut c = Converter::new(44_100, 1, 48_000, 1).unwrap();
        let mut out = Vec::new();
        c.process(&sine(44_100, 1, 0.01, 440.0), &mut out).unwrap();
        c.reset();
        assert!(c.pending[0].is_empty());
    }
}
