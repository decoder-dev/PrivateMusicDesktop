use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};

use crate::Error;

/// What the elementary stream in the buffer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementaryKind {
    AdtsAac,
    Mp3,
}

#[derive(Default)]
struct State {
    data: Vec<u8>,
    kind: Option<ElementaryKind>,
    complete: bool,
    failure: Option<Error>,
    cancelled: bool,
}

struct Inner {
    state: Mutex<State>,
    changed: Condvar,
}

/// A byte buffer one thread appends to while another reads it, blocking
/// until the bytes it wants arrive.
#[derive(Clone)]
pub struct SharedBuffer {
    inner: Arc<Inner>,
    limit: usize,
}

impl SharedBuffer {
    /// `limit` caps the total size: a hostile playlist cannot grow memory
    /// without bound.
    pub fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new(Inner { state: Mutex::new(State::default()), changed: Condvar::new() }),
            limit,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panicking writer must not wedge the audio thread: the state is
        // plain data, so the poisoned guard is still usable.
        self.inner.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Appends bytes. Returns `false` when the buffer was cancelled or the
    /// size limit was hit (the buffer is then failed).
    pub fn push(&self, bytes: &[u8]) -> bool {
        let mut state = self.lock();
        if state.cancelled || state.failure.is_some() {
            return false;
        }
        if state.data.len().saturating_add(bytes.len()) > self.limit {
            state.failure = Some(Error::Stream("stream is larger than the size limit".into()));
            self.inner.changed.notify_all();
            return false;
        }
        state.data.extend_from_slice(bytes);
        self.inner.changed.notify_all();
        true
    }

    pub fn set_kind(&self, kind: ElementaryKind) {
        let mut state = self.lock();
        state.kind = Some(kind);
        self.inner.changed.notify_all();
    }

    pub fn finish(&self) {
        let mut state = self.lock();
        state.complete = true;
        self.inner.changed.notify_all();
    }

    pub fn fail(&self, error: Error) {
        let mut state = self.lock();
        if state.failure.is_none() && !state.complete {
            state.failure = Some(error);
        }
        self.inner.changed.notify_all();
    }

    pub fn cancel(&self) {
        let mut state = self.lock();
        state.cancelled = true;
        self.inner.changed.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    pub fn len(&self) -> usize {
        self.lock().data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_complete(&self) -> bool {
        self.lock().complete
    }

    pub fn failure(&self) -> Option<Error> {
        self.lock().failure.clone()
    }

    /// Blocks until the kind is known (first segment demuxed), the stream
    /// failed, or it was cancelled.
    pub fn wait_for_kind(&self) -> Result<ElementaryKind, Error> {
        let mut state = self.lock();
        loop {
            if let Some(kind) = state.kind {
                return Ok(kind);
            }
            if let Some(error) = &state.failure {
                return Err(error.clone());
            }
            if state.cancelled {
                return Err(Error::Cancelled);
            }
            if state.complete {
                return Err(Error::Stream("stream ended before any audio".into()));
            }
            state = self.inner.changed.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub fn reader(&self) -> BufferReader {
        BufferReader { shared: self.clone(), position: 0 }
    }
}

/// `Read + Seek` over a [`SharedBuffer`]. A read at the end of what has been
/// downloaded so far waits for more; end of stream is only reported once the
/// download completed.
pub struct BufferReader {
    shared: SharedBuffer,
    position: u64,
}

impl BufferReader {
    pub fn total_len(&self) -> Option<u64> {
        let state = self.shared.lock();
        state.complete.then_some(state.data.len() as u64)
    }
}

/// Carried inside the `io::Error` of a cancelled read.
///
/// Not `ErrorKind::Interrupted`: `Read::read_to_end` and most readers retry
/// that kind forever, which would turn a cancel into a hang.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled_io(error: &io::Error) -> bool {
    error.get_ref().map(|inner| inner.is::<Cancelled>()).unwrap_or(false)
}

fn io_error(error: &Error) -> io::Error {
    match error {
        Error::Cancelled => io::Error::new(io::ErrorKind::Other, Cancelled),
        other => io::Error::new(io::ErrorKind::Other, other.to_string()),
    }
}

impl Read for BufferReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut state = self.shared.lock();
        loop {
            let available = state.data.len() as u64;
            if self.position < available {
                let start = self.position as usize;
                let count = out.len().min(state.data.len() - start);
                out[..count].copy_from_slice(&state.data[start..start + count]);
                self.position += count as u64;
                return Ok(count);
            }
            if state.cancelled {
                return Err(io_error(&Error::Cancelled));
            }
            if let Some(error) = &state.failure {
                return Err(io_error(error));
            }
            if state.complete {
                return Ok(0);
            }
            state = self.shared.inner.changed.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }
}

impl Seek for BufferReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => match self.total_len() {
                Some(len) => len.checked_add_signed(delta),
                None => {
                    return Err(io::Error::new(io::ErrorKind::Unsupported, "length unknown until download completes"))
                }
            },
        };
        match target {
            Some(n) => {
                self.position = n;
                Ok(n)
            }
            None => Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start")),
        }
    }
}

impl symphonia::core::io::MediaSource for BufferReader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.total_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn reader_waits_for_data_and_sees_end_only_after_finish() {
        let buffer = SharedBuffer::new(1 << 20);
        let mut reader = buffer.reader();
        let writer = buffer.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            writer.push(b"hello ");
            thread::sleep(Duration::from_millis(30));
            writer.push(b"world");
            writer.finish();
        });
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        handle.join().unwrap();
        assert_eq!(all, b"hello world");
        assert_eq!(reader.total_len(), Some(11));
    }

    #[test]
    fn failure_and_cancel_reach_the_reader() {
        let buffer = SharedBuffer::new(1 << 20);
        let mut reader = buffer.reader();
        buffer.push(b"ab");
        buffer.fail(Error::Stream("boom".into()));
        let mut chunk = [0u8; 2];
        assert_eq!(reader.read(&mut chunk).unwrap(), 2);
        assert!(reader.read(&mut chunk).is_err());

        let cancelled = SharedBuffer::new(1 << 20);
        let mut reader = cancelled.reader();
        cancelled.cancel();
        let error = reader.read(&mut chunk).unwrap_err();
        assert!(is_cancelled_io(&error));
        assert_ne!(error.kind(), io::ErrorKind::Interrupted, "would be retried forever");
        // And the standard retrying reader terminates.
        assert!(cancelled.reader().read_to_end(&mut Vec::new()).is_err());
        assert!(!cancelled.push(b"x"));
    }

    #[test]
    fn size_limit_fails_the_stream() {
        let buffer = SharedBuffer::new(8);
        assert!(buffer.push(b"12345678"));
        assert!(!buffer.push(b"9"));
        assert!(buffer.failure().is_some());
    }

    #[test]
    fn seek_from_end_needs_a_complete_download() {
        let buffer = SharedBuffer::new(1 << 20);
        buffer.push(b"0123456789");
        let mut reader = buffer.reader();
        assert!(reader.seek(SeekFrom::End(0)).is_err());
        buffer.finish();
        assert_eq!(reader.seek(SeekFrom::End(-2)).unwrap(), 8);
        assert_eq!(reader.seek(SeekFrom::Current(-3)).unwrap(), 5);
        assert!(reader.seek(SeekFrom::Current(-100)).is_err());
    }

    #[test]
    fn wait_for_kind_unblocks_on_set_and_on_failure() {
        let buffer = SharedBuffer::new(1 << 10);
        let writer = buffer.clone();
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            writer.set_kind(ElementaryKind::Mp3);
        });
        assert_eq!(buffer.wait_for_kind().unwrap(), ElementaryKind::Mp3);
        t.join().unwrap();

        let failing = SharedBuffer::new(1 << 10);
        failing.fail(Error::Offline);
        assert_eq!(failing.wait_for_kind().unwrap_err(), Error::Offline);
    }
}
