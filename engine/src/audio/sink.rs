use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::{Error, Result};
use super::health::CallbackWatch;

/// Куда идет декодированный звук. Находится в аудиопотоке и не является `Send` (поток cpal должен
/// оставаться в потоке, который его создал).
pub trait Sink {
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> usize;
    /// Перемежающиеся семплы, которые можно записать прямо сейчас без блокировки.
    fn writable(&self) -> usize;
    fn write(&mut self, samples: &[f32]) -> usize;
    /// Кадры, воспроизведенные устройством с момента последней очистки.
    fn played_frames(&self) -> u64;
    /// Кадры написаны, но еще не воспроизведены.
    fn queued_frames(&self) -> usize;
    /// Удаляет все, что стоит в очереди, и сбрасывает счетчик сыгранных игр. Возвращается после
    /// подтверждения со стороны устройства, поэтому ничего записанного впоследствии не теряется.
    fn flush(&mut self);
    fn set_paused(&mut self, paused: bool);
    fn set_volume(&mut self, volume: f32);
    /// Ложь после исчезновения устройства (отключены наушники).
    fn is_healthy(&self) -> bool;
    /// Насколько быстрее, чем в реальном времени, устройство потребляет звук. Всегда 1 для звуковой
    /// карты; смоделированное устройство в тестах может работать быстрее.
    fn time_scale(&self) -> f64 {
        1.0
    }
}

/// Имитационная звуковая карта, которая потребляет звук в реальном времени (возможно, быстрее), для
/// тестов и для машин без звукового оборудования.
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
    /// `speed` > 1 заставляет мнимое устройство потреблять энергию быстрее, чем в реальном времени.
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
    fn time_scale(&self) -> f64 { self.speed }
}

struct Shared {
    callbacks: AtomicU64,
    played_frames: AtomicU64,
    paused: AtomicBool,
    volume_bits: AtomicU32,
    flush_requested: AtomicU64,
    flush_acknowledged: AtomicU64,
    broken: AtomicBool,
}

pub struct CpalSink {
    watch: CallbackWatch,
    _stream: Stream,
    producer: HeapProd<f32>,
    shared: Arc<Shared>,
    rate: u32,
    channels: usize,
    capacity: usize,
}

/// Названия звуковых выходов, предлагаемые системой, сначала по умолчанию.
pub fn output_devices() -> Vec<String> {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .output_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.sort();
    names.dedup();
    if let Some(default) = default {
        names.retain(|n| *n != default);
        names.insert(0, default);
    }
    names
}

impl CpalSink {
    /// Открывает устройство вывода по умолчанию.
    pub fn open_default() -> Result<Self> {
        Self::open(None)
    }

    /// Открывает указанный выход или выход по умолчанию, если `name` равен `None` или это устройство
    /// отсутствует (отключение наушников не должно означать тишину).
    pub fn open(name: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let named = name.and_then(|wanted| {
            host.output_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(wanted))
        });
        if name.is_some() && named.is_none() {
            log::warn!("audio output {name:?} is not available, using the default one");
        }
        let device = match named {
            Some(device) => device,
            None => host.default_output_device().ok_or_else(|| Error::Stream("no audio output device".into()))?,
        };
        let config = device.default_output_config().map_err(|e| Error::Stream(format!("audio output: {e}")))?;
        let rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let capacity = (rate as usize * channels) / 2; // полсекунды
        let (producer, consumer) = HeapRb::<f32>::new(capacity.max(4096)).split();
        let shared = Arc::new(Shared {
            callbacks: AtomicU64::new(0),
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
        Ok(Self { watch: CallbackWatch::new(Instant::now()), _stream: stream, producer, shared, rate, channels, capacity: capacity.max(4096) })
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
            move |output: &mut [T], _| render_output(output, &mut consumer, &shared, channels),
            move |err| {
                log::warn!("audio output error: {err}");
                error_flag.broken.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| Error::Stream(format!("audio output: {e}")))
}

fn render_output<T: SizedSample + FromSample<f32>>(output: &mut [T], consumer: &mut HeapCons<f32>, shared: &Shared, channels: usize) {
    shared.callbacks.fetch_add(1, Ordering::Relaxed);
    // Флеш просит обратный вызов удалить то, что стоит в очереди. Автор ждет подтверждения, прежде чем
    // писать снова.
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
        if self.shared.flush_acknowledged.load(Ordering::Acquire) != generation {
            log::warn!("audio output stopped acknowledging flushes; reopening is required");
            self.shared.broken.store(true, Ordering::Relaxed);
        }
    }
    fn set_paused(&mut self, paused: bool) { self.shared.paused.store(paused, Ordering::Relaxed); }
    fn set_volume(&mut self, volume: f32) {
        // `NaN.clamp(..)` — это NaN, а усиление NaN превращает каждую выборку в шум.
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        self.shared.volume_bits.store(volume.to_bits(), Ordering::Relaxed);
    }
    fn is_healthy(&self) -> bool {
        if self.shared.broken.load(Ordering::Relaxed) {
            return false;
        }
        if !self.watch.healthy(self.shared.callbacks.load(Ordering::Relaxed), Instant::now()) {
            log::warn!("audio output callbacks stopped for 3 s; reopening is required");
            self.shared.broken.store(true, Ordering::Relaxed);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_outputs_silence_and_resume_preserves_queued_audio() {
        let shared = Shared {
            callbacks: AtomicU64::new(0), played_frames: AtomicU64::new(0),
            paused: AtomicBool::new(true), volume_bits: AtomicU32::new(1.0f32.to_bits()),
            flush_requested: AtomicU64::new(0), flush_acknowledged: AtomicU64::new(0),
            broken: AtomicBool::new(false),
        };
        let (mut producer, mut consumer) = HeapRb::<f32>::new(8).split();
        producer.push_slice(&[0.5, -0.5, 0.25, -0.25]);
        let mut output = [1.0f32; 4];
        render_output(&mut output, &mut consumer, &shared, 2);
        assert_eq!(output, [0.0; 4], "pause must silence the actual device buffer");
        assert_eq!(consumer.occupied_len(), 4);
        assert_eq!(shared.played_frames.load(Ordering::Relaxed), 0);
        assert_eq!(shared.callbacks.load(Ordering::Relaxed), 1, "paused callbacks still count as healthy");
        shared.paused.store(false, Ordering::Relaxed);
        render_output(&mut output, &mut consumer, &shared, 2);
        assert_eq!(output, [0.5, -0.5, 0.25, -0.25]);
        assert_eq!(shared.played_frames.load(Ordering::Relaxed), 2);
    }
}
