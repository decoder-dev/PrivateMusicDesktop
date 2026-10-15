use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder as SymDecoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use super::buffer::{BufferReader, ElementaryKind};
use crate::{Error, Result};

/// Decodes the elementary stream in a [`BufferReader`] to interleaved f32.
pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymDecoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: usize,
    /// Frames to throw away after a seek so it lands on the exact time.
    skip_frames: u64,
    scratch: Option<SampleBuffer<f32>>,
}

fn stream_error(error: SymError) -> Error {
    match error {
        SymError::IoError(io) if super::buffer::is_cancelled_io(&io) => Error::Cancelled,
        other => Error::Stream(other.to_string()),
    }
}

impl Decoder {
    /// Blocks until enough of the stream has been downloaded to identify it.
    pub fn open(reader: BufferReader, kind: ElementaryKind) -> Result<Self> {
        let mut hint = Hint::new();
        hint.with_extension(match kind {
            ElementaryKind::AdtsAac => "aac",
            ElementaryKind::Mp3 => "mp3",
        });
        let source = MediaSourceStream::new(Box::new(reader), MediaSourceStreamOptions::default());
        let probed = symphonia::default::get_probe()
            .format(&hint, source, &FormatOptions { enable_gapless: false, ..Default::default() }, &MetadataOptions::default())
            .map_err(stream_error)?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| Error::Stream("no audio track".into()))?;
        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.ok_or_else(|| Error::Stream("unknown sample rate".into()))?;
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2).max(1);
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(stream_error)?;
        Ok(Self { format, decoder, track_id, sample_rate, channels, skip_frames: 0, scratch: None })
    }

    /// Appends the next block of interleaved samples to `out`. `Ok(false)`
    /// is the end of the stream.
    pub fn next_block(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymError::IoError(io)) if io.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(SymError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(other) => return Err(stream_error(other)),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    self.channels = spec.channels.count().max(1);
                    self.sample_rate = spec.rate;
                    let needed = decoded.capacity() as u64;
                    let scratch = match &mut self.scratch {
                        Some(buf) if buf.capacity() as u64 >= needed * spec.channels.count() as u64 => buf,
                        slot => slot.insert(SampleBuffer::<f32>::new(needed, spec)),
                    };
                    scratch.copy_interleaved_ref(decoded);
                    let mut samples = scratch.samples();
                    if self.skip_frames > 0 {
                        let drop_samples = ((self.skip_frames as usize) * self.channels).min(samples.len());
                        self.skip_frames -= (drop_samples / self.channels) as u64;
                        samples = &samples[drop_samples..];
                    }
                    if samples.is_empty() {
                        continue;
                    }
                    out.extend_from_slice(samples);
                    return Ok(true);
                }
                // A corrupt frame costs a frame, not the track.
                Err(SymError::DecodeError(reason)) => {
                    log::debug!("skipping undecodable frame: {reason}");
                }
                Err(SymError::IoError(io)) if io.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(other) => return Err(stream_error(other)),
            }
        }
    }

    /// Moves the stream to `seconds`. Blocks while the download catches up.
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time { time: Time::from(seconds), track_id: Some(self.track_id) },
            )
            .map_err(stream_error)?;
        self.decoder.reset();
        self.skip_frames = seeked.required_ts.saturating_sub(seeked.actual_ts);
        Ok(())
    }
}
