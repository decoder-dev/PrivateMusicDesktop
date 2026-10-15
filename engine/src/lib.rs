//! Private Music desktop engine.
//!
//! Everything here is synchronous and free of GUI types so it can be tested
//! headless: the VK client, the HLS fetcher (using the same `pm_native` core as
//! the iOS app for playlist parsing, AES-128 and MPEG-TS demuxing), the
//! decoder, the queue and the player thread.

pub mod audio;
pub mod controller;
pub mod error;
pub mod queue;
pub mod stream;
pub mod unmask;
pub mod vk;

pub use error::{Error, Result};
