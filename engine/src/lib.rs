//! Настольный движок Private Music.
//!
//! Здесь все синхронно и не содержит типов GUI, поэтому его можно тестировать без головы: клиент VK,
//! сборщик HLS (использует то же ядро `pm_native`, что и приложение iOS для анализа плейлистов,
//! демультиплексирования AES-128 и MPEG-TS), декодер, очередь и проигрыватель. нить.

pub mod app;
pub mod artwork;
pub mod audio;
pub mod controller;
pub mod error;
pub mod history;
pub mod lyrics;
pub mod media_sync;
pub mod net;
pub mod offline;
pub mod queue;
pub mod radio;
pub mod store;
pub mod stream;
pub mod unmask;
pub mod update;
pub mod vk;

pub use error::{Error, Result};
