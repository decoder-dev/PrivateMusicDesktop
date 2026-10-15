use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::{Error, Result};

/// Where decoded audio goes. Lives on the audio thread and is not `Send`
/// (a cpal stream must stay on the thread that built it).
pub trait Sink {
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> usize;
    /// Interleaved samples that can be written right now without blocking.
    fn writable(&self) -> usize;
    fn write(&mut self, samples: &[f32]) -> usize;
    /// Frames the device has played since the last flush.
    fn played_frames(&self) -> u64;
    /// Frames written but not played yet.
    fn queued_frames(&self) -> usize;
    /// Drops everything queued and resets the played counter. Returns once
    /// the device side has acknowledged, so nothing written afterwards is lost.
    fn flush(&mut self);
    fn set_paused(&mut self, paused: bool);
    fn set_volume(&mut self, volume: f32);
    /// False after the device disappeared (unplugged headphones).
    fn is_healthy(&self) -> bool;
}

/// A pretend sound card that consumes audio in real time (optionally faster),
/// for tests and for machines without audio hardware.
pub struct SimulatedSink {
    rate: u32,
    channels: usize,
    capacity_frames: usize,
    speed: f64,
    state: std::cell::RefCell<SimState>,
    pub captured: Arc<std::sync::Mutex<Vec<f32>>>,
    pub volume: Arc<AtomicU32>,
}

struct SimState {
    queued: f64,
    played: f64,
    last: Instant,
    paused: bool,
}

impl SimulatedSink {
    /// `speed` > 1 makes the pretend device consume faster than real time.
    pub fn new(rate: u32, channels: usize, speed: f64) -> Self {
        Self {
            rate,
            channels,
            capacity_frames: rate as usize / 2,
            speed,
            state: std::cell::RefCell::new(SimState { queued: 0.0, played: 0.0, last: Instant::now(), paused: false }),
            captured: Arc::new(std::sync::Mutex::new(Vec::new())),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }
    }

    fn advance(&self) {
        let mut state = self.state.borrow_mut();
        let now = Instant::now();
        if !state.paused {
            let consumed = (now - state.last).as_secs_f64() * self.rate as f64 * self.speed;
            let consumed = consumed.min(state.queued);
            state.queued -= consumed;
            state.played += consumed;
        }
        state.last = now;
    }
}

impl Sink for SimulatedSink {
    fn sample_rate(&self) -> u32 { self.rate }
    fn channels(&self) -> usize { self.channels }
    fn writable(&self) -> usize {
        self.advance();
        let queued = self.state.borrow().queued as usize;
        self.capacity_frames.saturating_sub(queued) * self.channels
    }
    fn write(&mut self, samples: &[f32]) -> usize {
        let count = samples.len().min(self.writable()) / self.channels * self.channels;
        self.state.borrow_mut().queued += (count / self.channels) as f64;
        self.captured.lock().unwrap().extend_from_slice(&samples[..count]);
        count
    }
    fn played_frames(&self) -> u64 {
        self.advance();
        self.state.borrow().played as u64
    }
    fn queued_frames(&self) -> usize {
        self.advance();
        self.state.borrow().queued.ceil() as usize
    }
    fn flush(&mut self) {
        self.advance();
        let mut state = self.state.borrow_mut();
        state.queued = 0.0;
        state.played = 0.0;
        self.captured.lock().unwrap().clear();
    }
    fn set_paused(&mut self, paused: bool) {
        self.advance();
        self.state.borrow_mut().paused = paused;
    }
    fn set_volume(&mut self, volume: f32) {
        self.volume.store(volume.to_bits(), Ordering::Relaxed);
    }
    fn is_healthy(&self) -> bool { true }
}

struct Shared {
    played_frames: AtomicU64,
    paused: AtomicBool,
    volume_bits: AtomicU32,
    flush_requested: AtomicU64,
    flush_acknowledged: AtomicU64,
    broken: AtomicBool,
}

pub struct CpalSink {
    _stream: Stream,
    producer: HeapProd<f32>,
    shared: Arc<Shared>,
    rate: u32,
    channels: usize,
    capacity: usize,
}

impl CpalSink {
    /// Opens the default output device.
    pub fn open_default() -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| Error::Stream("no audio output device".into()))?;
        let config = device.default_output_config().map_err(|e| Error::Stream(format!("audio output: {e}")))?;
        let rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let capacity = (rate as usize * channels) / 2; // half a second
        let (producer, consumer) = HeapRb::<f32>::new(capacity.max(4096)).split();
        let shared = Arc::new(Shared {
            played_frames: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            flush_requested: AtomicU64::new(0),
            flush_acknowledged: AtomicU64::new(0),
            broken: AtomicBool::new(false),
        });
        let stream_config: cpal::StreamConfig = config.clone().into();
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, &stream_config, consumer, shared.clone(), channels),
            SampleFormat::I16 => build::<i16>(&device, &stream_config, consumer, shared.clone(), channels),
            SampleFormat::U16 => build::<u16>(&device, &stream_config, consumer, shared.clone(), channels),
            SampleFormat::I32 => build::<i32>(&device, &stream_config, consumer, shared.clone(), channels),
            other => return Err(Error::Stream(format!("unsupported audio sample format {other:?}"))),
        }?;
        stream.play().map_err(|e| Error::Stream(format!("audio output: {e}")))?;
        Ok(Self { _stream: stream, producer, shared, rate, channels, capacity: capacity.max(4096) })
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: HeapCons<f32>,
    shared: Arc<Shared>,
    channels: usize,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let error_flag = shared.clone();
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| {
                // A flush asks the callback to drop what is queued. The
                // writer waits for the acknowledgement before writing again.
                let requested = shared.flush_requested.load(Ordering::Acquire);
                if requested != shared.flush_acknowledged.load(Ordering::Relaxed) {
                    while consumer.try_pop().is_some() {}
                    shared.played_frames.store(0, Ordering::Relaxed);
                    shared.flush_acknowledged.store(requested, Ordering::Release);
                }
                if shared.paused.load(Ordering::Relaxed) {
                    output.fill(T::EQUILIBRIUM);
                    return;
                }
                let volume = f32::from_bits(shared.volume_bits.load(Ordering::Relaxed));
                let mut frames_played = 0u64;
                for frame in output.chunks_mut(channels) {
                    if consumer.occupied_len() < channels {
                        frame.fill(T::EQUILIBRIUM);
                        continue;
                    }
                    for slot in frame.iter_mut() {
                        let sample = consumer.try_pop().unwrap_or(0.0);
                        *slot = T::from_sample((sample * volume).clamp(-1.0, 1.0));
                    }
                    frames_played += 1;
                }
                shared.played_frames.fetch_add(frames_played, Ordering::Relaxed);
            },
            move |err| {
                log::warn!("audio output error: {err}");
                error_flag.broken.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| Error::Stream(format!("audio output: {e}")))
}

impl Sink for CpalSink {
    fn sample_rate(&self) -> u32 { self.rate }
    fn channels(&self) -> usize { self.channels }
    fn writable(&self) -> usize { self.producer.vacant_len() }
    fn write(&mut self, samples: &[f32]) -> usize { self.producer.push_slice(samples) }
    fn played_frames(&self) -> u64 { self.shared.played_frames.load(Ordering::Relaxed) }
    fn queued_frames(&self) -> usize { (self.capacity - self.producer.vacant_len()) / self.channels }
    fn flush(&mut self) {
        let generation = self.shared.flush_requested.fetch_add(1, Ordering::AcqRel) + 1;
        let deadline = Instant::now() + Duration::from_millis(300);
        while self.shared.flush_acknowledged.load(Ordering::Acquire) != generation && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn set_paused(&mut self, paused: bool) { self.shared.paused.store(paused, Ordering::Relaxed); }
    fn set_volume(&mut self, volume: f32) {
        // `NaN.clamp(..)` is NaN, and a NaN gain turns every sample into noise.
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        self.shared.volume_bits.store(volume.to_bits(), Ordering::Relaxed);
    }
    fn is_healthy(&self) -> bool { !self.shared.broken.load(Ordering::Relaxed) }
}
