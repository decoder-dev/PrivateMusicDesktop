//! Загрузка, расшифровка и разбор потока до элементарного аудио.
//!
//! HLS, AES-128 и контейнеры обрабатываются общей библиотекой `pm_native`.
//! Результат поступает в общий буфер, из которого читает декодер.
//! Сетевые ошибки и отмена передаются потребителю без раскрытия секретов.

pub mod buffer;
pub mod cmaf;
pub mod decode;
pub mod fetch;

pub use buffer::{BufferReader, ElementaryKind, SharedBuffer};
pub use decode::Decoder;
pub use fetch::{spawn_fetch, FetchConfig};
