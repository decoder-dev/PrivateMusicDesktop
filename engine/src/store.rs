//! Сохранение входа между запусками приложения.
//!
//! Токен и cookie VK хранятся только в защищённом хранилище операционной системы:
//! Keychain, Credential Manager или Secret Service. При временной недоступности
//! хранилища текущая сессия продолжает работать в памяти, а запись можно повторить.

use std::sync::Mutex;

use crate::vk::Session;
use crate::{Error, Result};

pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<Session>>;
    fn save(&self, session: &Session) -> Result<()>;
    fn clear(&self) -> Result<()>;
    /// Записывает явную замену учетной записи перед попыткой записи учетных данных.
    fn begin_login(&self) -> Result<()> {
        Ok(())
    }
    /// False для магазинов, которые забывают все на выходе.
    fn is_durable(&self) -> bool {
        true
    }
}

/// Сохраняет сеанс только в памяти. Используется в тестах и ​​в качестве запасного варианта, когда у OS
/// нет секретного хранилища.
#[derive(Default)]
pub struct MemoryStore(Mutex<Option<Session>>);

impl SecretStore for MemoryStore {
    fn is_durable(&self) -> bool {
        false
    }

    fn load(&self) -> Result<Option<Session>> {
        Ok(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }

    fn save(&self, session: &Session) -> Result<()> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(session.clone());
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

/// Хранилище, которое держит всю сессию одним текстом за платформенным механизмом
/// (на Android это плагин `secure-store` поверх Android Keystore). Лимита на размер
/// записи там нет, поэтому ни разбиения, ни банков, ни контрольных сумм не нужно:
/// запись атомарна на стороне платформы.
pub trait BlobBackend: Send + Sync {
    fn read(&self) -> Result<Option<String>>;
    fn write(&self, text: &str) -> Result<()>;
    fn remove(&self) -> Result<()>;
}

pub struct BackendStore<B: BlobBackend>(B);

impl<B: BlobBackend> BackendStore<B> {
    pub fn new(backend: B) -> Self {
        Self(backend)
    }
}

impl<B: BlobBackend> SecretStore for BackendStore<B> {
    fn load(&self) -> Result<Option<Session>> {
        // Нечитаемый текст считается выходом из аккаунта, как и у хранилища ОС.
        Ok(self.0.read()?.and_then(|json| serde_json::from_str(&json).ok()))
    }

    fn save(&self, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session).map_err(|e| Error::Format(e.to_string()))?;
        self.0.write(&json)
    }

    fn clear(&self) -> Result<()> {
        self.0.remove()
    }
}

// У системной связки ключей нет бэкенда под Android: там работает `BackendStore`.
#[cfg(not(target_os = "android"))]
mod os_keyring;
#[cfg(not(target_os = "android"))]
pub use os_keyring::{load_update_token, update_token, KeyringStore};

// Android updates use the public release endpoint; a separate GitHub token is not stored.
#[cfg(target_os = "android")]
pub fn update_token(_token: Option<&str>) -> Result<()> {
    Err(Error::Transport("secret store: not available on Android".into()))
}

#[cfg(target_os = "android")]
pub fn load_update_token() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Session {
        Session {
            access_token: "0123456789abcdef0123".into(),
            user_id: Some(5),
            expires_at: Some(1_900_000_000),
            api_user_agent: "ua".into(),
            refresh_cookie: "remixsid=abc".into(),
            web_user_agent: "web".into(),
        }
    }

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        assert!(store.load().unwrap().is_none());
        store.save(&sample()).unwrap();
        assert_eq!(store.load().unwrap().unwrap().user_id, Some(5));
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn session_survives_json() {
        let json = serde_json::to_string(&sample()).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.access_token, "0123456789abcdef0123");
        assert_eq!(back.refresh_cookie, "remixsid=abc");
    }

    struct MemoryBlob(Mutex<Option<String>>);

    impl BlobBackend for MemoryBlob {
        fn read(&self) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn write(&self, text: &str) -> Result<()> {
            *self.0.lock().unwrap() = Some(text.to_string());
            Ok(())
        }
        fn remove(&self) -> Result<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn backend_store_keeps_a_long_session_whole() {
        let store = BackendStore::new(MemoryBlob(Mutex::new(None)));
        assert!(store.load().unwrap().is_none());
        let mut session = sample();
        session.refresh_cookie = "remixsid=ё😀".repeat(3000);
        store.save(&session).unwrap();
        assert_eq!(store.load().unwrap().unwrap().refresh_cookie, session.refresh_cookie);
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn backend_store_treats_garbage_as_signed_out() {
        let store = BackendStore::new(MemoryBlob(Mutex::new(Some("not json".into()))));
        assert!(store.load().unwrap().is_none());
    }
}
