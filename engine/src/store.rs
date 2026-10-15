//! Where the signed-in session lives between launches.
//!
//! The VK token and cookie are credentials: they go into the operating
//! system's secret store (Keychain, Credential Manager, Secret Service) and
//! nowhere else. If no secret store is available the app still works for the
//! current run; it just cannot remember the sign-in.

use std::sync::Mutex;

use crate::vk::Session;
use crate::{Error, Result};

pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<Session>>;
    fn save(&self, session: &Session) -> Result<()>;
    fn clear(&self) -> Result<()>;
    /// False for stores that forget everything at exit.
    fn is_durable(&self) -> bool {
        true
    }
}

/// Keeps the session in memory only. Used in tests and as the fallback when
/// the OS has no secret store.
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

/// The operating system's secret store.
pub struct KeyringStore {
    service: String,
}

const ACCOUNT: &str = "vk-session";

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    fn entry(&self) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, ACCOUNT).map_err(|e| Error::Transport(format!("secret store: {e}")))
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<Session>> {
        match self.entry()?.get_password() {
            Ok(json) => Ok(serde_json::from_str(&json).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Transport(format!("secret store: {e}"))),
        }
    }

    fn save(&self, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session).map_err(|e| Error::Format(e.to_string()))?;
        self.entry()?.set_password(&json).map_err(|e| Error::Transport(format!("secret store: {e}")))
    }

    fn clear(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Transport(format!("secret store: {e}"))),
        }
    }
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
}
