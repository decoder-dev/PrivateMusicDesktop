//! Output side: converting decoded audio to the device format and handing it
//! to the sound card.

pub mod convert;
pub mod player;
pub mod sink;
pub mod spectrum;

pub use player::{Command, PlayerEvent, PlayerHandle, Remote};
pub use sink::{output_devices, CpalSink, SimulatedSink, Sink};
