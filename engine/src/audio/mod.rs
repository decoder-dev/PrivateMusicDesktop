//! Output side: converting decoded audio to the device format and handing it
//! to the sound card.

pub mod convert;
pub mod player;
pub mod sink;

pub use player::{Command, PlayerEvent, PlayerHandle};
pub use sink::{CpalSink, SimulatedSink, Sink};
