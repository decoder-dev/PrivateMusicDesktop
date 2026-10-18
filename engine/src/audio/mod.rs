//! Выходная сторона: преобразование декодированного звука в формат устройства и передача его на
//! звуковую карту.

pub mod convert;
pub mod eq;
mod health;
pub mod player;
pub mod sink;
pub mod spectrum;

pub use player::{Command, PlayerEvent, PlayerHandle, Remote};
pub use sink::{output_devices, CpalSink, SimulatedSink, Sink};
