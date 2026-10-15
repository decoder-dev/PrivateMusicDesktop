//! Native core of Private Music.
//!
//! Every `pm_*` function here keeps the exact C ABI of the headers in
//! `PrivateMusic/Native/`, which Swift imports through the bridging header.
//! The headers stay the contract; this crate is the implementation.

#![allow(clippy::missing_safety_doc)]
#![allow(non_camel_case_types, non_upper_case_globals)]

pub mod core;
pub mod dsp;
pub mod hls;
pub mod media;
pub mod offline;
