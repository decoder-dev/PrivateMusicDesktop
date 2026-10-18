//! Родное ядро Private Music.
//!
//! Каждая функция `pm_*` здесь сохраняет точный C ABI заголовков в `PrivateMusic/Native/`, который
//! Swift импортирует через мостовой заголовок. Заголовки остаются в рамках контракта; этот ящик
//! является реализацией.

#![allow(clippy::missing_safety_doc)]
#![allow(non_camel_case_types, non_upper_case_globals)]

pub mod core;
pub mod dsp;
pub mod hls;
pub mod media;
pub mod offline;
