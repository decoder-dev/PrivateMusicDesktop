use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Ошибки, на которые может реагировать UI. `Unauthorized` означает "sign in again".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unauthorized,
    Offline,
    Timeout,
    /// VK ответил ошибкой API. `code` — это `error_code` VK.
    Vk { code: i64, message: String },
    Http(u16),
    /// Ответ не имел той формы, на которую мы рассчитываем.
    Format(String),
    Transport(String),
    /// Поток не удалось получить, расшифровать или декодировать.
    Stream(String),
    /// VK знает трек, но не дает по нему стрим (заблокировано правообладателем или в этом регионе, или
    /// только для подписчиков).
    Unavailable,
    Cancelled,
    InvalidRequest,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "The VK session has expired. Sign in again."),
            Error::Offline => write!(f, "No internet connection."),
            Error::Timeout => write!(f, "The request timed out."),
            Error::Vk { code, message } => write!(f, "VK error {code}: {message}"),
            Error::Http(status) => write!(f, "HTTP status {status}."),
            Error::Format(what) => write!(f, "Unexpected response: {what}"),
            Error::Transport(what) => write!(f, "Network error: {what}"),
            Error::Stream(what) => write!(f, "Stream error: {what}"),
            Error::Unavailable => write!(f, "This track is not available from VK."),
            Error::Cancelled => write!(f, "Cancelled."),
            Error::InvalidRequest => write!(f, "Invalid request."),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Error::Timeout
        } else if err.is_connect() {
            Error::Offline
        } else if let Some(status) = err.status() {
            Error::Http(status.as_u16())
        } else {
            // `reqwest::Error` Display может встроить запрос URL, который для хостов потока содержит
            // подписанный токен. Оставляйте только вид.
            Error::Transport(err.without_url().to_string())
        }
    }
}

impl Error {
    /// Стабильная машиночитаемая категория для UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Unauthorized => "unauthorized",
            Error::Offline => "offline",
            Error::Timeout => "timeout",
            Error::Vk { .. } => "vk",
            Error::Http(_) => "http",
            Error::Format(_) => "format",
            Error::Transport(_) => "network",
            Error::Stream(_) => "stream",
            Error::Unavailable => "unavailable",
            Error::Cancelled => "cancelled",
            Error::InvalidRequest => "invalid",
        }
    }
}
