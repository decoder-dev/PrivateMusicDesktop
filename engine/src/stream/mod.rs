//! Getting a track from a VK stream URL to decoded audio.
//!
//! `fetch` downloads (HLS playlist, keys, segments, or a plain file), decrypts
//! and demuxes into one elementary stream (ADTS AAC or MP3) inside a
//! [`SharedBuffer`]; `decode` reads that buffer with symphonia while the
//! download is still running, so playback starts after the first segment.

pub mod buffer;
pub mod cmaf;
pub mod decode;
pub mod fetch;

pub use buffer::{BufferReader, ElementaryKind, SharedBuffer};
pub use decode::Decoder;
pub use fetch::{spawn_fetch, FetchConfig};
