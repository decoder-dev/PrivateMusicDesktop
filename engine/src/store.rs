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
///
/// Windows Credential Manager holds at most 2560 bytes (1280 UTF-16
/// characters) per entry and VK's cookies alone can be longer, so a long
/// session is split: the main entry then holds `parts:N` and the text sits in
/// `vk-session.1` … `vk-session.N`. A short one stays in the main entry as is.
pub struct KeyringStore {
    service: String,
}

const ACCOUNT: &str = "vk-session";
/// UTF-16 units per entry: under Windows' 1280, with room to spare.
const PART_LEN: usize = 1000;
const PARTS_PREFIX: &str = "parts:";

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }
}

/// Named text slots in a secret store; the split format is written over these.
trait Slots {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
}

impl Slots for KeyringStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        match keyring_entry(&self.service, name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(secret_error(e)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        keyring_entry(&self.service, name)?.set_password(value).map_err(secret_error)
    }

    fn delete(&self, name: &str) -> Result<()> {
        match keyring_entry(&self.service, name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(secret_error(e)),
        }
    }
}

fn keyring_entry(service: &str, name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(service, name).map_err(secret_error)
}

/// A separate credential, never mixed with the VK account or written to disk.
pub fn update_token(token: Option<&str>) -> Result<()> {
    let store = KeyringStore::new("com.dec.privatemusic2.desktop.updater");
    match token.filter(|s| !s.is_empty()) {
        Some(value) => store.set("github-read", value),
        None => store.delete("github-read"),
    }
}

pub fn load_update_token() -> Result<Option<String>> {
    KeyringStore::new("com.dec.privatemusic2.desktop.updater").get("github-read")
}

fn secret_error(e: keyring::Error) -> Error {
    Error::Transport(format!("secret store: {e}"))
}

fn part_name(index: usize) -> String {
    format!("{ACCOUNT}.{index}")
}

/// How many parts the main entry points to (0 when it holds the text itself).
fn part_count(head: &str) -> usize {
    head.strip_prefix(PARTS_PREFIX).and_then(|n| n.parse().ok()).unwrap_or(0)
}

fn load_text(slots: &impl Slots) -> Result<Option<String>> {
    let Some(head) = slots.get(ACCOUNT)? else {
        return Ok(None);
    };
    let count = part_count(&head);
    if count == 0 {
        return Ok(Some(head));
    }
    let mut text = String::new();
    for index in 1..=count {
        match slots.get(&part_name(index))? {
            Some(part) => text.push_str(&part),
            None => return Ok(None), // a part went missing: treat as signed out
        }
    }
    Ok(Some(text))
}

fn save_text(slots: &impl Slots, text: &str) -> Result<()> {
    let old = slots.get(ACCOUNT).ok().flatten().map_or(0, |head| part_count(&head));
    let parts = split_utf16(text, PART_LEN);
    let count = if parts.len() > 1 { parts.len() } else { 0 };
    if count == 0 {
        slots.set(ACCOUNT, text)?;
    } else {
        // Parts first, so the main entry never points at parts not yet written.
        for (index, part) in parts.iter().enumerate() {
            slots.set(&part_name(index + 1), part)?;
        }
        slots.set(ACCOUNT, &format!("{PARTS_PREFIX}{count}"))?;
    }
    for index in count + 1..=old {
        let _ = slots.delete(&part_name(index));
    }
    Ok(())
}

/// Splits text into pieces of at most `max` UTF-16 units, never inside a character.
fn split_utf16(text: &str, max: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut units) = (0, 0);
    for (at, c) in text.char_indices() {
        if units + c.len_utf16() > max {
            parts.push(&text[start..at]);
            (start, units) = (at, 0);
        }
        units += c.len_utf16();
    }
    parts.push(&text[start..]);
    parts
}

fn clear_text(slots: &impl Slots) -> Result<()> {
    let old = slots.get(ACCOUNT).ok().flatten().map_or(0, |head| part_count(&head));
    slots.delete(ACCOUNT)?;
    for index in 1..=old {
        let _ = slots.delete(&part_name(index));
    }
    Ok(())
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<Session>> {
        Ok(load_text(self)?.and_then(|json| serde_json::from_str(&json).ok()))
    }

    fn save(&self, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session).map_err(|e| Error::Format(e.to_string()))?;
        save_text(self, &json)
    }

    fn clear(&self) -> Result<()> {
        clear_text(self)
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

    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MapSlots(Mutex<BTreeMap<String, String>>);

    impl Slots for MapSlots {
        fn get(&self, name: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(name).cloned())
        }
        fn set(&self, name: &str, value: &str) -> Result<()> {
            // Windows' limit, in UTF-16 bytes.
            assert!(value.encode_utf16().count() * 2 <= 2560, "{name} too long for Windows");
            self.0.lock().unwrap().insert(name.into(), value.into());
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<()> {
            self.0.lock().unwrap().remove(name);
            Ok(())
        }
    }

    #[test]
    fn a_long_session_is_split_and_read_back() {
        let slots = MapSlots::default();
        let mut session = sample();
        session.refresh_cookie = "remixsid=ё😀".repeat(600); // long, with 2-unit characters
        let json = serde_json::to_string(&session).unwrap();
        save_text(&slots, &json).unwrap();
        assert!(slots.0.lock().unwrap().len() > 2);
        assert_eq!(load_text(&slots).unwrap().as_deref(), Some(json.as_str()));
    }

    #[test]
    fn a_short_session_stays_in_one_entry_and_old_parts_are_removed() {
        let slots = MapSlots::default();
        save_text(&slots, &"x".repeat(3500)).unwrap();
        assert_eq!(slots.0.lock().unwrap().len(), 5); // main entry + 4 parts
        save_text(&slots, "short").unwrap();
        assert_eq!(slots.0.lock().unwrap().keys().collect::<Vec<_>>(), [ACCOUNT]);
        assert_eq!(load_text(&slots).unwrap().as_deref(), Some("short"));
        save_text(&slots, &"y".repeat(2500)).unwrap();
        clear_text(&slots).unwrap();
        assert!(slots.0.lock().unwrap().is_empty());
        assert_eq!(load_text(&slots).unwrap(), None);
    }

    #[test]
    fn a_missing_part_reads_as_signed_out() {
        let slots = MapSlots::default();
        save_text(&slots, &"z".repeat(2500)).unwrap();
        slots.delete(&part_name(2)).unwrap();
        assert_eq!(load_text(&slots).unwrap(), None);
    }

    /// The real Credential Manager, which is what refused long sessions.
    #[cfg(windows)]
    #[test]
    fn windows_credential_manager_keeps_a_long_session() {
        let store = KeyringStore::new(format!("private-music-test-{}", std::process::id()));
        let mut session = sample();
        session.refresh_cookie = "remixsid=0123456789; ".repeat(300);
        store.save(&session).unwrap();
        assert_eq!(store.load().unwrap().unwrap().refresh_cookie, session.refresh_cookie);
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
