//! Воспроизведение с раздельными потоками декодирования и вывода звука.
//!
//! Поток декодера ждёт данные, декодирует их, преобразует в формат устройства
//! и передаёт готовые блоки через ограниченный канал. Ожидание сети не блокирует
//! поток управления аудио.
//!
//! Аудиопоток (`run`) владеет выходным устройством, подаёт готовые блоки,
//! применяет паузу, громкость, перемотку и сообщает события. Сетевые запросы
//! и декодирование выполняются отдельно.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver as StdReceiver, RecvTimeoutError, Sender as StdSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, SendTimeoutError, Sender, TryRecvError};

use super::convert::Converter;
use super::sink::Sink;
use super::eq::Equalizer;
use super::spectrum::{Analyzer, Tap, BANDS};
use crate::stream::{spawn_fetch, Decoder, FetchConfig, SharedBuffer};
use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// Трек был запрошен; звук не запустился.
    Loading { track_id: String },
    /// Звук выходит из динамиков (или возобновляется).
    Playing { track_id: String },
    Paused { track_id: String },
    /// Поток прервался в середине дорожки, и воспроизведение ожидает данных.
    Buffering { track_id: String, on: bool },
    Position { track_id: String, seconds: f64, duration: f64 },
    /// Уровни (0…1) того, что играет прямо сейчас; только при включенном визуализаторе.
    Spectrum { bands: [f32; BANDS] },
    /// Трек доигрался сам по себе.
    Ended { track_id: String },
    Stopped,
    Failed { track_id: String, error: Error },
}

#[derive(Debug)]
pub enum Command {
    Play { track_id: String, url: String, duration: f64, start: f64, fetch: FetchConfig },
    Pause,
    Resume,
    Seek(f64),
    SetVolume(f32),
    /// Включает или выключает спектральный анализ (и его события).
    SetVisualizer(bool),
    /// Усиление эквалайзера в dB для `eq::BANDS`; `None` (или все нули) отключает его.
    SetEqualizer(Option<[f32; 10]>),
    /// Закрывает звуковую карту, чтобы при следующем воспроизведении она открывалась снова
    /// (пользователь выбрал другой выход). Что бы ни играло, останавливается.
    ReopenSink,
    Stop,
    Shutdown,
}

enum Chunk {
    Audio { generation: u64, samples: Vec<f32> },
    End { generation: u64 },
    Failed { generation: u64, error: Error },
}

enum DecoderCommand {
    /// `near_end`: цель находится в пределах последних секунд длины VK, которая округляется в большую
    /// сторону и может находиться за пределами реального конца аудио.
    Seek { generation: u64, seconds: f64, near_end: bool },
}

/// Все о воспроизводимом треке.
struct Session {
    track_id: String,
    duration: f64,
    buffer: SharedBuffer,
    stop: Arc<AtomicBool>,
    chunks: Receiver<Chunk>,
    decoder_commands: Sender<DecoderCommand>,
    generation: u64,
    /// Время потока кадра приемника 0.
    base_seconds: f64,
    /// Преобразованные образцы ждут своего места в раковине.
    pending: Vec<f32>,
    pending_offset: usize,
    ended: bool,
    /// Устанавливается, когда поток умер; сообщается один раз, затем сеанс прерывается.
    failure: Option<Error>,
    started: bool,
    buffering: bool,
    tap: Tap,
    _fetch: JoinHandle<()>,
    _decoder: JoinHandle<()>,
}

impl Session {
    fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.buffer.cancel();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub struct PlayerHandle {
    commands: StdSender<Command>,
    thread: Option<JoinHandle<()>>,
}

/// Клонируемый способ общения с потоком игрока (из его собственного обратного вызова событий, из
/// рабочих потоков). Он не является владельцем потока.
#[derive(Clone)]
pub struct Remote {
    commands: StdSender<Command>,
}

impl Remote {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn play(&self, track_id: &str, url: &str, duration: f64, start: f64, fetch: FetchConfig) {
        self.send(Command::Play { track_id: track_id.to_string(), url: url.to_string(), duration, start, fetch });
    }

    pub fn pause(&self) { self.send(Command::Pause); }
    pub fn resume(&self) { self.send(Command::Resume); }
    pub fn seek(&self, seconds: f64) { self.send(Command::Seek(seconds)); }
    pub fn set_volume(&self, volume: f32) { self.send(Command::SetVolume(volume)); }
    pub fn set_visualizer(&self, on: bool) { self.send(Command::SetVisualizer(on)); }
    pub fn set_equalizer(&self, gains: Option<[f32; 10]>) { self.send(Command::SetEqualizer(gains)); }
    pub fn reopen_sink(&self) { self.send(Command::ReopenSink); }
    pub fn stop(&self) { self.send(Command::Stop); }
}

impl PlayerHandle {
    /// `make_sink` выполняется в аудиопотоке лениво при первом воспроизведении, поэтому приложение
    /// запускается без звуковой карты, и поток никогда не пересекает потоки.
    pub fn spawn<F>(make_sink: F, events: impl Fn(PlayerEvent) + Send + 'static) -> Self
    where
        F: FnMut() -> crate::Result<Box<dyn Sink>> + Send + 'static,
    {
        let (commands, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("pm-audio".into())
            .spawn(move || run(receiver, make_sink, events))
            .expect("spawn audio thread");
        Self { commands, thread: Some(thread) }
    }

    pub fn remote(&self) -> Remote {
        Remote { commands: self.commands.clone() }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn play(&self, track_id: &str, url: &str, duration: f64, start: f64, fetch: FetchConfig) {
        self.send(Command::Play { track_id: track_id.to_string(), url: url.to_string(), duration, start, fetch });
    }

    pub fn pause(&self) { self.send(Command::Pause); }
    pub fn resume(&self) { self.send(Command::Resume); }
    pub fn seek(&self, seconds: f64) { self.send(Command::Seek(seconds)); }
    pub fn set_volume(&self, volume: f32) { self.send(Command::SetVolume(volume)); }
    pub fn set_visualizer(&self, on: bool) { self.send(Command::SetVisualizer(on)); }
    pub fn set_equalizer(&self, gains: Option<[f32; 10]>) { self.send(Command::SetEqualizer(gains)); }
    pub fn reopen_sink(&self) { self.send(Command::ReopenSink); }
    pub fn stop(&self) { self.send(Command::Stop); }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

const POSITION_INTERVAL: Duration = Duration::from_millis(250);
/// Поиск так близко к длине VK (округленной) может оказаться за пределами реального конца.
const NEAR_END_SECONDS: f64 = 2.0;
/// Около 30 кадров в секунду для визуализатора.
const SPECTRUM_INTERVAL: Duration = Duration::from_millis(33);
/// Наполните раковину, когда она вмещает меньше этого значения, до `HIGH_WATER`. Пакетное заполнение
/// позволяет каждому потоку спать на несколько сотен миллисекунд за раз вместо того, чтобы просыпаться
/// десятки раз в секунду, что предотвращает расход заряда батареи при воспроизведении трека.
const LOW_WATER_SECONDS: f64 = 0.2;
const HIGH_WATER_SECONDS: f64 = 0.45;
/// Декодированные фрагменты (около 23 мс каждый) декодер может запускать раньше приемника.
const CHUNK_QUEUE: usize = 24;

fn run<F>(commands: StdReceiver<Command>, mut make_sink: F, events: impl Fn(PlayerEvent))
where
    F: FnMut() -> crate::Result<Box<dyn Sink>>,
{
    let mut sink: Option<Box<dyn Sink>> = None;
    let mut session: Option<Session> = None;
    let mut paused = false;
    let mut volume = 1.0f32;
    let mut last_position = Instant::now();
    let mut visualizer = false;
    let mut analyzer: Option<Analyzer> = None;
    let mut last_spectrum = Instant::now();
    let mut eq_gains: Option<[f32; 10]> = None;
    let mut eq: Option<Equalizer> = None;
    let mut eq_for: Option<(u32, usize)> = None; // формат устройства, для которого были созданы фильтры

    loop {
        // Сначала команды, поэтому Pause/Stop никогда не откладывает работу звука.
        let timeout = match (session.as_ref(), sink.as_ref()) {
            (Some(active), Some(output)) if !paused => {
                let wake = wake_after(active, output.as_ref(), last_position);
                if visualizer {
                    wake.min(SPECTRUM_INTERVAL.saturating_sub(last_spectrum.elapsed()).max(Duration::from_millis(5)))
                } else {
                    wake
                }
            }
            (Some(_), _) => Duration::from_millis(250),
            _ => Duration::from_millis(500),
        };
        let mut next = match commands.recv_timeout(timeout) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        while let Some(command) = next.take().or_else(|| commands.try_recv().ok()) {
            match command {
                Command::Shutdown => return,
                Command::SetVolume(v) => {
                    volume = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
                    if let Some(sink) = sink.as_mut() {
                        sink.set_volume(volume);
                    }
                }
                Command::SetEqualizer(gains) => {
                    eq_gains = gains;
                    eq_for = None; // восстановить ниже
                }
                Command::SetVisualizer(on) => {
                    visualizer = on;
                    if let Some(s) = session.as_mut() {
                        s.tap.enabled = on;
                        s.tap.reset();
                    }
                    if !on {
                        analyzer = None;
                    }
                }
                Command::ReopenSink => {
                    session = None;
                    sink = None;
                    paused = false;
                    events(PlayerEvent::Stopped);
                }
                Command::Stop => {
                    session = None;
                    if let Some(sink) = sink.as_mut() {
                        sink.flush();
                    }
                    paused = false;
                    events(PlayerEvent::Stopped);
                }
                Command::Pause => {
                    if let (Some(s), Some(sink)) = (session.as_ref(), sink.as_mut()) {
                        if !paused {
                            paused = true;
                            sink.set_paused(true);
                            log::info!("pause applied to audio output at {:.2} s", position_seconds(s, sink.as_ref()));
                            events(PlayerEvent::Paused { track_id: s.track_id.clone() });
                        }
                    }
                }
                Command::Resume => {
                    if let (Some(s), Some(sink)) = (session.as_ref(), sink.as_mut()) {
                        if paused {
                            paused = false;
                            sink.set_paused(false);
                            log::info!("resume applied to audio output at {:.2} s", position_seconds(s, sink.as_ref()));
                            if s.started {
                                events(PlayerEvent::Playing { track_id: s.track_id.clone() });
                            }
                        }
                    }
                }
                Command::Seek(seconds) => {
                    if let (Some(s), Some(sink)) = (session.as_mut(), sink.as_mut()) {
                        seek(s, sink.as_mut(), seconds);
                        if let Some(eq) = eq.as_mut() { eq.reset(); }
                        s.tap.reset();
                        events(position_event(s, sink.as_ref()));
                        last_position = Instant::now();
                    }
                }
                Command::Play { track_id, url, duration, start, fetch } => {
                    session = None;
                    if let Some(eq) = eq.as_mut() { eq.reset(); }
                    // Здесь повторяется попытка устройства, которое исчезло (или никогда не
                    // открывалось).
                    if sink.as_ref().map(|s| !s.is_healthy()).unwrap_or(true) {
                        sink = None;
                        match make_sink() {
                            Ok(mut opened) => {
                                opened.set_volume(volume);
                                sink = Some(opened);
                            }
                            Err(error) => {
                                events(PlayerEvent::Failed { track_id, error });
                                continue;
                            }
                        }
                    }
                    let sink = sink.as_mut().expect("opened above");
                    sink.flush();
                    if !sink.is_healthy() {
                        events(PlayerEvent::Failed { track_id, error: Error::Stream("audio output did not acknowledge a flush".into()) });
                        continue;
                    }
                    sink.set_paused(false);
                    paused = false;
                    events(PlayerEvent::Loading { track_id: track_id.clone() });
                    match start_session(track_id.clone(), &url, duration, start, fetch, (sink.sample_rate(), sink.channels()), visualizer) {
                        Ok(started) => session = Some(started),
                        Err(error) => events(PlayerEvent::Failed { track_id, error }),
                    }
                }
            }
        }

        let (Some(active), Some(output)) = (session.as_mut(), sink.as_mut()) else { continue };
        if !output.is_healthy() {
            // Устройство вышло из строя: сообщите и позвольте приложению принять решение.
            let id = active.track_id.clone();
            session = None;
            log::warn!("audio output is unresponsive; stopping the current stream for recovery");
            events(PlayerEvent::Failed { track_id: id, error: Error::Stream("audio device disconnected or stopped responding".into()) });
            continue;
        }

        let format = (output.sample_rate(), output.channels());
        if eq_for != Some(format) {
            eq = Equalizer::transition(eq.take(), eq_gains.as_ref(), format.0, format.1);
            eq_for = Some(format);
        }
        if !paused {
            pump(active, output.as_mut(), eq.as_mut(), &events);
            if visualizer && active.started && last_spectrum.elapsed() >= SPECTRUM_INTERVAL {
                last_spectrum = Instant::now();
                let analyzer = analyzer.get_or_insert_with(|| Analyzer::new(output.sample_rate()));
                events(PlayerEvent::Spectrum { bands: analyzer.analyze(&active.tap, output.played_frames()) });
            }
        }
        if let Some(error) = active.failure.take() {
            let id = active.track_id.clone();
            session = None;
            output.flush();
            events(PlayerEvent::Failed { track_id: id, error });
            continue;
        }

        if active.ended && !active.buffering && output.queued_frames() == 0 && active.pending_offset >= active.pending.len() {
            let id = active.track_id.clone();
            session = None;
            events(PlayerEvent::Ended { track_id: id });
            continue;
        }
        if !paused && last_position.elapsed() >= POSITION_INTERVAL {
            events(position_event(active, output.as_ref()));
            last_position = Instant::now();
        }
    }
}

fn position_seconds(session: &Session, sink: &dyn Sink) -> f64 {
    let played = sink.played_frames() as f64 / sink.sample_rate() as f64;
    let position = session.base_seconds + played;
    if session.duration > 0.0 { position.min(session.duration) } else { position }
}

fn position_event(session: &Session, sink: &dyn Sink) -> PlayerEvent {
    PlayerEvent::Position {
        track_id: session.track_id.clone(),
        seconds: position_seconds(session, sink),
        duration: session.duration,
    }
}

/// Как долго аудиопоток может спать, прежде чем он снова начнет работать: до тех пор, пока сток не
/// опустеет до нижней отметки (или раньше, пока он ожидает декодера), и никогда не пройдет мимо
/// следующего отчета о местоположении.
fn wake_after(session: &Session, sink: &dyn Sink, last_position: Instant) -> Duration {
    let scale = sink.time_scale().max(1.0);
    let queued = sink.queued_frames() as f64 / sink.sample_rate() as f64;
    let until_refill = if session.pending_offset < session.pending.len() || queued < LOW_WATER_SECONDS {
        // Ожидается работа или устройство скоро разрядится.
        0.008
    } else {
        (queued - LOW_WATER_SECONDS) / scale
    };
    let until_report = POSITION_INTERVAL.saturating_sub(last_position.elapsed()).as_secs_f64();
    let end_check = if session.ended { 0.05 } else { f64::INFINITY };
    Duration::from_secs_f64(until_refill.min(until_report).min(end_check).clamp(0.005, 0.25))
}

/// Перемещает фрагменты из потока декодера в приемник.
fn pump(session: &mut Session, sink: &mut dyn Sink, mut eq: Option<&mut Equalizer>, events: &impl Fn(PlayerEvent)) {
    loop {
        // Сначала закончите то, что осталось от предыдущего куска.
        if session.pending_offset < session.pending.len() {
            let room = sink.writable();
            if room == 0 {
                return;
            }
            let end = (session.pending_offset + room).min(session.pending.len());
            let written = sink.write(&session.pending[session.pending_offset..end]);
            session.tap.push(&session.pending[session.pending_offset..session.pending_offset + written], sink.channels());
            session.pending_offset += written;
            if !session.started && written > 0 {
                session.started = true;
                events(PlayerEvent::Playing { track_id: session.track_id.clone() });
            }
            if session.pending_offset < session.pending.len() {
                return;
            }
            session.pending.clear();
            session.pending_offset = 0;
        }
        // Не ставьте в очередь больше верхней отметки: поиск должен смыть немного.
        if sink.queued_frames() as f64 / sink.sample_rate() as f64 > HIGH_WATER_SECONDS {
            return;
        }
        match session.chunks.try_recv() {
            Ok(Chunk::Audio { generation, samples }) if generation == session.generation => {
                if session.buffering {
                    session.buffering = false;
                    events(PlayerEvent::Buffering { track_id: session.track_id.clone(), on: false });
                }
                session.pending = samples;
                session.pending_offset = 0;
                if let Some(eq) = eq.as_deref_mut() {
                    eq.process(&mut session.pending);
                }
            }
            Ok(Chunk::End { generation }) if generation == session.generation => {
                session.ended = true;
                // Больше ничего не приходит, так что данных больше не ждем.
                if session.buffering {
                    session.buffering = false;
                    events(PlayerEvent::Buffering { track_id: session.track_id.clone(), on: false });
                }
            }
            Ok(Chunk::Failed { generation, error }) if generation == session.generation => {
                session.failure = Some(error);
                session.cancel();
                return;
            }
            Ok(_) => {} // устаревший фрагмент перед поиском
            Err(TryRecvError::Empty) => {
                // Ничего не декодировано и сток почти сухой: буферизация.
                if !session.ended && session.started && !session.buffering && sink.queued_frames() == 0 {
                    session.buffering = true;
                    events(PlayerEvent::Buffering { track_id: session.track_id.clone(), on: true });
                }
                return;
            }
            Err(TryRecvError::Disconnected) => {
                // Нить декодера исчезла. После End это нормально; в любой другой раз он умер, что не
                // должно выглядеть как законченный трек.
                if !session.ended {
                    session.failure = Some(Error::Stream("the decoder stopped unexpectedly".into()));
                    session.cancel();
                }
                return;
            }
        }
    }
}

fn seek(session: &mut Session, sink: &mut dyn Sink, seconds: f64) {
    let mut target = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    if session.duration > 0.0 {
        target = target.min((session.duration - 0.05).max(0.0));
    }
    session.generation += 1;
    session.ended = false;
    session.pending.clear();
    session.pending_offset = 0;
    let near_end = session.duration > 0.0 && target >= session.duration - NEAR_END_SECONDS;
    let _ = session.decoder_commands.send(DecoderCommand::Seek { generation: session.generation, seconds: target, near_end });
    sink.flush();
    session.base_seconds = target;
}

fn start_session(
    track_id: String,
    url: &str,
    duration: f64,
    start: f64,
    fetch: FetchConfig,
    (out_rate, out_channels): (u32, usize),
    visualizer: bool,
) -> crate::Result<Session> {
    let buffer = SharedBuffer::new(fetch.max_stream_bytes);
    let fetch_thread = spawn_fetch(url.to_string(), fetch, buffer.clone());
    let (chunk_sender, chunks) = bounded::<Chunk>(CHUNK_QUEUE);
    let (decoder_commands, command_receiver) = unbounded::<DecoderCommand>();
    let stop = Arc::new(AtomicBool::new(false));
    let start = if start.is_finite() { start.max(0.0) } else { 0.0 };

    let decoder_thread = {
        let buffer = buffer.clone();
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("pm-decode".into())
            .spawn(move || decode_loop(buffer, stop, chunk_sender, command_receiver, start, out_rate, out_channels))
            .map_err(|e| Error::Stream(e.to_string()))?
    };
    Ok(Session {
        track_id,
        duration: duration.max(0.0),
        buffer,
        stop,
        chunks,
        decoder_commands,
        generation: 0,
        base_seconds: start,
        pending: Vec::new(),
        pending_offset: 0,
        ended: false,
        failure: None,
        started: false,
        buffering: false,
        tap: Tap::new(visualizer),
        _fetch: fetch_thread,
        _decoder: decoder_thread,
    })
}

/// Отправляет фрагмент, блокируя до тех пор, пока не останется место. Он просыпается, когда аудиопоток
/// принимает кусок (без опроса), и прекращает работу, если сеанс тем временем отменяется.
fn send_chunk(sender: &Sender<Chunk>, stop: &AtomicBool, mut chunk: Chunk) -> bool {
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        match sender.send_timeout(chunk, Duration::from_millis(250)) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(back)) => chunk = back,
            Err(SendTimeoutError::Disconnected(_)) => return false,
        }
    }
}

fn decode_loop(
    buffer: SharedBuffer,
    stop: Arc<AtomicBool>,
    sender: Sender<Chunk>,
    commands: Receiver<DecoderCommand>,
    start: f64,
    out_rate: u32,
    out_channels: usize,
) {
    let mut generation = 0u64;
    let opened = (|| {
        let kind = buffer.wait_for_kind()?;
        let mut decoder = Decoder::open(buffer.reader(), kind)?;
        if start > 0.05 {
            decoder.seek(start)?;
        }
        let converter = Converter::new(decoder.sample_rate, decoder.channels, out_rate, out_channels)?;
        Ok::<_, Error>((decoder, converter))
    })();
    let (mut decoder, mut converter) = match opened {
        Ok(pair) => pair,
        Err(Error::Cancelled) => return,
        Err(error) => {
            send_chunk(&sender, &stop, Chunk::Failed { generation, error:buffer.failure().unwrap_or(error) });
            return;
        }
    };

    let mut decoded = Vec::new();
    let mut converted = Vec::new();
    let mut at_end = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // Последний Seek побеждает; более старые заменяются.
        let mut seek = None;
        while let Ok(DecoderCommand::Seek { generation: g, seconds, near_end }) = commands.try_recv() {
            seek = Some((g, seconds, near_end));
        }
        // В конце потока нечего декодировать: спать до тех пор, пока не придет Seek (или сеанс не будет
        // прерван), без опроса.
        if at_end && seek.is_none() {
            match commands.recv_timeout(Duration::from_millis(250)) {
                Ok(DecoderCommand::Seek { generation: g, seconds, near_end }) => seek = Some((g, seconds, near_end)),
                Err(_) => continue,
            }
        }
        if let Some((g, seconds, near_end)) = seek {
            generation = g;
            at_end = false;
            converter.reset();
            if let Err(error) = decoder.seek(seconds) {
                if error == Error::Cancelled {
                    return;
                }
                if near_end {
                    // После реального окончания звука: трек окончен, что не является сбоем (в случае
                    // сбоя он перезапускается с 0:00).
                    at_end = true;
                    if !send_chunk(&sender, &stop, Chunk::End { generation }) {
                        return;
                    }
                    continue;
                }
                send_chunk(&sender, &stop, Chunk::Failed { generation, error });
                return;
            }
        }
        if at_end {
            continue;
        }
        decoded.clear();
        match decoder.next_block(&mut decoded) {
            Ok(true) => {
                converted.clear();
                if let Err(error) = converter.process(&decoded, &mut converted) {
                    send_chunk(&sender, &stop, Chunk::Failed { generation, error });
                    return;
                }
                if !converted.is_empty() && !send_chunk(&sender, &stop, Chunk::Audio { generation, samples: std::mem::take(&mut converted) }) {
                    return;
                }
            }
            Ok(false) => {
                if let Some(error) = buffer.failure() {
                    send_chunk(&sender, &stop, Chunk::Failed { generation, error });
                    return;
                }
                at_end = true;
                if !send_chunk(&sender, &stop, Chunk::End { generation }) {
                    return;
                }
            }
            Err(Error::Cancelled) => return,
            Err(error) => {
                send_chunk(&sender, &stop, Chunk::Failed { generation, error:buffer.failure().unwrap_or(error) });
                return;
            }
        }
    }
}
