//! The player thread.
//!
//! Two threads cooperate per track:
//!
//! * the **decoder thread** waits for the download, decodes, converts to the
//!   device format and sends chunks through a bounded channel. It may block
//!   for long (a seek past what has been downloaded) without affecting
//!   anything else;
//! * the **audio thread** (this file's `run`) owns the sink, writes chunks to
//!   it, applies pause/volume/seek and reports events. It never blocks on the
//!   network or on decoding, so Pause and Stop always answer immediately.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::convert::Converter;
use super::sink::Sink;
use crate::stream::{spawn_fetch, Decoder, FetchConfig, SharedBuffer};
use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A track was requested; audio has not started.
    Loading { track_id: String },
    /// Audio is coming out of the speakers (or resumed).
    Playing { track_id: String },
    Paused { track_id: String },
    /// The stream ran dry mid-track and playback waits for data.
    Buffering { track_id: String, on: bool },
    Position { track_id: String, seconds: f64, duration: f64 },
    /// The track played to its end by itself.
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
    Stop,
    Shutdown,
}

enum Chunk {
    Audio { generation: u64, samples: Vec<f32> },
    End { generation: u64 },
    Failed { generation: u64, error: Error },
}

enum DecoderCommand {
    Seek { generation: u64, seconds: f64 },
}

/// Everything about the track being played.
struct Session {
    track_id: String,
    duration: f64,
    buffer: SharedBuffer,
    stop: Arc<AtomicBool>,
    chunks: Receiver<Chunk>,
    decoder_commands: Sender<DecoderCommand>,
    generation: u64,
    /// Stream time of the sink's frame 0.
    base_seconds: f64,
    /// Converted samples waiting for room in the sink.
    pending: Vec<f32>,
    pending_offset: usize,
    ended: bool,
    /// Set when the stream died; reported once, then the session is dropped.
    failure: Option<Error>,
    started: bool,
    buffering: bool,
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
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

/// A cloneable way to talk to the player thread (from its own event callback,
/// from worker threads). It does not own the thread.
#[derive(Clone)]
pub struct Remote {
    commands: Sender<Command>,
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
    pub fn stop(&self) { self.send(Command::Stop); }
}

impl PlayerHandle {
    /// `make_sink` runs on the audio thread, lazily at the first Play, so the
    /// app starts without a sound card and a stream never crosses threads.
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
const TICK: Duration = Duration::from_millis(5);
/// Keep this much decoded audio queued in the sink before pausing writes.
const SINK_TARGET_SECONDS: f64 = 0.4;

fn run<F>(commands: Receiver<Command>, mut make_sink: F, events: impl Fn(PlayerEvent))
where
    F: FnMut() -> crate::Result<Box<dyn Sink>>,
{
    let mut sink: Option<Box<dyn Sink>> = None;
    let mut session: Option<Session> = None;
    let mut paused = false;
    let mut volume = 1.0f32;
    let mut last_position = Instant::now();

    loop {
        // Commands first, so Pause/Stop never wait behind audio work.
        let timeout = if session.is_some() && !paused { TICK } else { Duration::from_millis(50) };
        let mut next = match commands.recv_timeout(timeout) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        while let Some(command) = next.take().or_else(|| commands.try_recv().ok()) {
            match command {
                Command::Shutdown => return,
                Command::SetVolume(v) => {
                    volume = v.clamp(0.0, 1.0);
                    if let Some(sink) = sink.as_mut() {
                        sink.set_volume(volume);
                    }
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
                            events(PlayerEvent::Paused { track_id: s.track_id.clone() });
                        }
                    }
                }
                Command::Resume => {
                    if let (Some(s), Some(sink)) = (session.as_ref(), sink.as_mut()) {
                        if paused {
                            paused = false;
                            sink.set_paused(false);
                            if s.started {
                                events(PlayerEvent::Playing { track_id: s.track_id.clone() });
                            }
                        }
                    }
                }
                Command::Seek(seconds) => {
                    if let (Some(s), Some(sink)) = (session.as_mut(), sink.as_mut()) {
                        seek(s, sink.as_mut(), seconds);
                        events(position_event(s, sink.as_ref()));
                        last_position = Instant::now();
                    }
                }
                Command::Play { track_id, url, duration, start, fetch } => {
                    session = None;
                    // A device that vanished (or never opened) is retried here.
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
                    sink.set_paused(false);
                    paused = false;
                    events(PlayerEvent::Loading { track_id: track_id.clone() });
                    match start_session(track_id.clone(), &url, duration, start, fetch, sink.sample_rate(), sink.channels()) {
                        Ok(started) => session = Some(started),
                        Err(error) => events(PlayerEvent::Failed { track_id, error }),
                    }
                }
            }
        }

        let (Some(active), Some(output)) = (session.as_mut(), sink.as_mut()) else { continue };
        if !output.is_healthy() {
            // Device gone mid-track: report and let the app decide.
            let id = active.track_id.clone();
            session = None;
            events(PlayerEvent::Failed { track_id: id, error: Error::Stream("audio device was disconnected".into()) });
            continue;
        }

        if !paused {
            pump(active, output.as_mut(), &events);
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

/// Moves chunks from the decoder thread into the sink.
fn pump(session: &mut Session, sink: &mut dyn Sink, events: &impl Fn(PlayerEvent)) {
    loop {
        // Finish what is left of the previous chunk first.
        if session.pending_offset < session.pending.len() {
            let room = sink.writable();
            if room == 0 {
                return;
            }
            let end = (session.pending_offset + room).min(session.pending.len());
            let written = sink.write(&session.pending[session.pending_offset..end]);
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
        // Do not queue more than the target: a seek should flush little.
        if sink.queued_frames() as f64 / sink.sample_rate() as f64 > SINK_TARGET_SECONDS {
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
            }
            Ok(Chunk::End { generation }) if generation == session.generation => session.ended = true,
            Ok(Chunk::Failed { generation, error }) if generation == session.generation => {
                session.failure = Some(error);
                session.cancel();
                return;
            }
            Ok(_) => {} // stale chunk from before a seek
            Err(mpsc::TryRecvError::Empty) => {
                // Nothing decoded and the sink is nearly dry: buffering.
                if !session.ended && session.started && !session.buffering && sink.queued_frames() == 0 {
                    session.buffering = true;
                    events(PlayerEvent::Buffering { track_id: session.track_id.clone(), on: true });
                }
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // The decoder thread is gone. After End that is normal; any
                // other time it died, which must not look like a finished track.
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
    let _ = session.decoder_commands.send(DecoderCommand::Seek { generation: session.generation, seconds: target });
    sink.flush();
    session.base_seconds = target;
}

fn start_session(
    track_id: String,
    url: &str,
    duration: f64,
    start: f64,
    fetch: FetchConfig,
    out_rate: u32,
    out_channels: usize,
) -> crate::Result<Session> {
    let buffer = SharedBuffer::new(fetch.max_stream_bytes);
    let fetch_thread = spawn_fetch(url.to_string(), fetch, buffer.clone());
    let (chunk_sender, chunks) = mpsc::sync_channel::<Chunk>(6);
    let (decoder_commands, command_receiver) = mpsc::channel::<DecoderCommand>();
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
        _fetch: fetch_thread,
        _decoder: decoder_thread,
    })
}

/// Sends a chunk, giving up if the session is cancelled meanwhile.
fn send_chunk(sender: &SyncSender<Chunk>, stop: &AtomicBool, mut chunk: Chunk) -> bool {
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        match sender.try_send(chunk) {
            Ok(()) => return true,
            Err(TrySendError::Full(back)) => {
                chunk = back;
                std::thread::sleep(Duration::from_millis(4));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn decode_loop(
    buffer: SharedBuffer,
    stop: Arc<AtomicBool>,
    sender: SyncSender<Chunk>,
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
            send_chunk(&sender, &stop, Chunk::Failed { generation, error });
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
        // The latest Seek wins; older ones are superseded.
        let mut seek = None;
        while let Ok(DecoderCommand::Seek { generation: g, seconds }) = commands.try_recv() {
            seek = Some((g, seconds));
        }
        if let Some((g, seconds)) = seek {
            generation = g;
            at_end = false;
            converter.reset();
            if let Err(error) = decoder.seek(seconds) {
                if error == Error::Cancelled {
                    return;
                }
                send_chunk(&sender, &stop, Chunk::Failed { generation, error });
                return;
            }
        }
        if at_end {
            // Wait for a Seek or for the session to be dropped.
            std::thread::sleep(Duration::from_millis(10));
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
                at_end = true;
                if !send_chunk(&sender, &stop, Chunk::End { generation }) {
                    return;
                }
            }
            Err(Error::Cancelled) => return,
            Err(error) => {
                send_chunk(&sender, &stop, Chunk::Failed { generation, error });
                return;
            }
        }
    }
}
