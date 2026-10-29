use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Секретное хранилище операционной системы.
///
/// Записи Windows представляют собой ограниченные фрагменты UTF-16. Два банка допускают полную запись с
/// проверкой контрольной суммы до фиксации манифеста; предыдущий снимок можно восстановить только для
/// той же известной учетной записи. Устаревшие записи остаются читаемыми.
pub struct KeyringStore {
    service: String,
    logout_marker: Option<PathBuf>,
}

const ACCOUNT: &str = "vk-session";
/// UTF-16 единиц на запись: под Windows 1280, с запасом места.
const PART_LEN: usize = 1000;
const PARTS_PREFIX: &str = "parts:";

impl KeyringStore {
    pub fn with_logout_marker(mut self, path: PathBuf) -> Self {
        self.logout_marker = Some(path);
        self
    }

    fn mark_logout(&self) -> Result<()> {
        if let Some(path) = &self.logout_marker {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::Transport(e.to_string()))?;
            }
            std::fs::write(path, b"signed-out").map_err(|e| Error::Transport(e.to_string()))?;
        }
        Ok(())
    }

    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), logout_marker: None }
    }
}

/// Именные текстовые слоты в хранилище секретов; поверх них записывается формат разделения.
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

/// Отдельные учетные данные, которые никогда не смешиваются с учетной записью VK и не записываются на
/// диск.
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

/// На сколько частей указывает основной вход (0, если он содержит сам текст).
fn part_count(head: &str) -> usize {
    head.strip_prefix(PARTS_PREFIX).and_then(|n| n.parse().ok()).unwrap_or(0)
}

const MAX_PARTS: usize = 64;
const V2: &str = "session-v2:";
const LOGGED_OUT: &str = "session-v2:logout";
#[derive(Clone, Serialize, Deserialize)]
struct Blob {
    bank: u8,
    parts: usize,
    hash: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    active: Blob,
    previous: Option<Blob>,
}
fn bank_part(blob: &Blob, index: usize) -> String {
    format!("{ACCOUNT}.v2.{}.{}", blob.bank, index)
}
fn checksum(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn manifest(head: &str) -> Option<Manifest> {
    serde_json::from_str(head.strip_prefix(V2)?).ok()
}
fn read_blob(slots: &impl Slots, blob: &Blob) -> Result<Option<String>> {
    if blob.bank > 1 || blob.parts == 0 || blob.parts > MAX_PARTS {
        return Ok(None);
    }
    let mut text = String::new();
    for i in 0..blob.parts {
        let Some(part) = slots.get(&bank_part(blob, i))? else {
            return Ok(None);
        };
        if part.encode_utf16().count() > PART_LEN {
            return Ok(None);
        }
        text.push_str(&part);
    }
    Ok((checksum(&text) == blob.hash).then_some(text))
}
fn load_text(slots: &impl Slots) -> Result<Option<String>> {
    let Some(head) = slots.get(ACCOUNT)? else {
        return Ok(None);
    };
    if head == LOGGED_OUT {
        return Ok(None);
    }
    if head.starts_with(V2) {
        let Some(m) = manifest(&head) else {
            return Ok(None);
        };
        if let Some(text) = read_blob(slots, &m.active)? {
            return Ok(Some(text));
        }
        return match m.previous {
            Some(blob) => read_blob(slots, &blob),
            None => Ok(None),
        };
    }
    let count = part_count(&head);
    if count > MAX_PARTS {
        return Ok(None);
    }
    if count == 0 {
        return Ok(Some(head));
    }
    let mut text = String::new();
    for index in 1..=count {
        match slots.get(&part_name(index))? {
            Some(part) if part.encode_utf16().count() <= PART_LEN => text.push_str(&part),
            _ => return Ok(None),
        }
    }
    Ok(Some(text))
}
fn save_text(slots: &impl Slots, text: &str) -> Result<()> {
    let head = slots.get(ACCOUNT)?;
    let old_manifest = head.as_deref().and_then(manifest);
    let mut previous = None;
    let mut old_bank_parts = [0usize; 2];
    if let Some(m) = old_manifest {
        for blob in std::iter::once(m.active).chain(m.previous) {
            if (blob.bank as usize) < old_bank_parts.len() {
                old_bank_parts[blob.bank as usize] = old_bank_parts[blob.bank as usize].max(blob.parts.min(MAX_PARTS));
            }
            if previous.is_none() && read_blob(slots, &blob)?.is_some() {
                previous = Some(blob);
            }
        }
    }
    let parts = split_utf16(text, PART_LEN);
    if parts.len() > MAX_PARTS {
        return Err(Error::Format("session exceeds credential store limit".into()));
    }
    let active = Blob { bank: previous.as_ref().map_or(0, |b| 1 - b.bank), parts: parts.len(), hash: checksum(text) };
    for (i, part) in parts.iter().enumerate() {
        slots.set(&bank_part(&active, i), part)?;
    }
    if read_blob(slots, &active)?.as_deref() != Some(text) {
        return Err(Error::Format("credential verification failed".into()));
    }
    // Восстановление никогда не должно автоматически восстанавливать другую учетную запись.
    let mut discarded_previous: Option<Blob> = None;
    if let Some(blob) = &previous {
        let old = read_blob(slots, blob)?.and_then(|s| serde_json::from_str::<Session>(&s).ok());
        let new = serde_json::from_str::<Session>(text).ok();
        if old.as_ref().and_then(|s| s.user_id).is_none() || old.and_then(|s| s.user_id) != new.and_then(|s| s.user_id)
        {
            discarded_previous = previous.take();
        }
    }
    let active_bank = active.bank;
    let active_parts = active.parts;
    let m = Manifest { active, previous };
    let json = serde_json::to_string(&m).map_err(|e| Error::Format(e.to_string()))?;
    // Единственный момент фиксации: существующие активные части до сих пор остаются нетронутыми.
    slots.set(ACCOUNT, &format!("{V2}{json}"))?;
    for i in active_parts..old_bank_parts[active_bank as usize] {
        let _ = slots.delete(&format!("{ACCOUNT}.v2.{active_bank}.{i}"));
    }
    if let Some(discarded) = discarded_previous {
        let count = old_bank_parts[discarded.bank as usize].max(discarded.parts.min(MAX_PARTS));
        for i in 0..count {
            let _ = slots.delete(&format!("{ACCOUNT}.v2.{}.{i}", discarded.bank));
        }
    }
    let old = head.as_deref().map_or(0, part_count).min(MAX_PARTS);
    for index in 1..=old {
        let _ = slots.delete(&part_name(index));
    }
    Ok(())
}

/// Разбивает текст на фрагменты длиной не более `max` UTF-16, но не внутри символа.
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
    // Перед удалением резервной копии создается надежное захоронение.
    slots.set(ACCOUNT, LOGGED_OUT)?;
    for i in 0..MAX_PARTS {
        for bank in 0..=1 {
            let _ = slots.delete(&format!("{ACCOUNT}.v2.{bank}.{i}"));
        }
        let _ = slots.delete(&part_name(i + 1));
    }
    Ok(())
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<Session>> {
        if let Some(path) = &self.logout_marker {
            match std::fs::metadata(path) {
                Ok(_) => return Ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Transport(e.to_string())),
            }
        }
        Ok(load_text(self)?.and_then(|json| serde_json::from_str(&json).ok()))
    }

    fn save(&self, session: &Session) -> Result<()> {
        let json = serde_json::to_string(session).map_err(|e| Error::Format(e.to_string()))?;
        save_text(self, &json)?;
        if let Some(path) = &self.logout_marker {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Transport(e.to_string())),
            }
        }
        Ok(())
    }

    fn begin_login(&self) -> Result<()> {
        self.mark_logout()
    }

    fn clear(&self) -> Result<()> {
        let guard = self.mark_logout();
        // По-прежнему очищайте записи OS, если запись несекретного маркера не удалась.
        let cleared = clear_text(self);
        cleared.and(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::sample;

    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MapSlots(Mutex<BTreeMap<String, String>>);

    impl Slots for MapSlots {
        fn get(&self, name: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(name).cloned())
        }
        fn set(&self, name: &str, value: &str) -> Result<()> {
            // Ограничение Windows, в UTF-16 байт.
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
        session.refresh_cookie = "remixsid=ё😀".repeat(600); // длинный, из двухзначных символов
        let json = serde_json::to_string(&session).unwrap();
        save_text(&slots, &json).unwrap();
        assert!(slots.0.lock().unwrap().len() > 2);
        assert_eq!(load_text(&slots).unwrap().as_deref(), Some(json.as_str()));
    }

    #[test]
    fn migration_and_logout_do_not_revive_old_parts() {
        let slots = MapSlots::default();
        slots.set(ACCOUNT, "parts:2").unwrap();
        slots.set(&part_name(1), "legacy").unwrap();
        slots.set(&part_name(2), " session").unwrap();
        assert_eq!(load_text(&slots).unwrap().as_deref(), Some("legacy session"));
        save_text(&slots, "new").unwrap();
        assert_eq!(load_text(&slots).unwrap().as_deref(), Some("new"));
        assert!(slots.get(&part_name(1)).unwrap().is_none());
        clear_text(&slots).unwrap();
        assert_eq!(load_text(&slots).unwrap(), None);
    }
    #[test]
    fn corrupt_active_snapshot_recovers_only_the_same_account() {
        let slots = MapSlots::default();
        let mut session = sample();
        let old = serde_json::to_string(&session).unwrap();
        save_text(&slots, &old).unwrap();
        session.access_token = "renewed".into();
        save_text(&slots, &serde_json::to_string(&session).unwrap()).unwrap();
        let m = manifest(&slots.get(ACCOUNT).unwrap().unwrap()).unwrap();
        slots.set(&bank_part(&m.active, 0), "damaged").unwrap();
        assert_eq!(load_text(&slots).unwrap().as_deref(), Some(old.as_str()));
        session.user_id = Some(99);
        save_text(&slots, &serde_json::to_string(&session).unwrap()).unwrap();
        let m = manifest(&slots.get(ACCOUNT).unwrap().unwrap()).unwrap();
        assert!(m.previous.is_none());
    }
    #[test]
    fn failed_staging_or_commit_keeps_the_previous_session() {
        struct Failing {
            slots: MapSlots,
            remaining: Mutex<usize>,
        }
        impl Slots for Failing {
            fn get(&self, n: &str) -> Result<Option<String>> {
                self.slots.get(n)
            }
            fn delete(&self, n: &str) -> Result<()> {
                self.slots.delete(n)
            }
            fn set(&self, n: &str, v: &str) -> Result<()> {
                let mut left = self.remaining.lock().unwrap();
                if *left == 0 {
                    return Err(Error::Transport("unavailable".into()));
                }
                *left -= 1;
                self.slots.set(n, v)
            }
        }
        for writes in [0, 1, 2, 3] {
            let slots = Failing { slots: MapSlots::default(), remaining: Mutex::new(100) };
            save_text(&slots, &"old".repeat(900)).unwrap();
            *slots.remaining.lock().unwrap() = writes;
            assert!(save_text(&slots, &"new".repeat(900)).is_err());
            assert_eq!(load_text(&slots).unwrap().unwrap(), "old".repeat(900));
        }
    }

    #[test]
    fn explicit_login_and_logout_markers_contain_no_credentials_and_block_restore() {
        let path = std::env::temp_dir().join(format!("pm-session-marker-{}", std::process::id()));
        let store = KeyringStore::new("pm-test-guard").with_logout_marker(path.clone());
        store.begin_login().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"signed-out");
        assert!(store.load().unwrap().is_none());
        std::fs::remove_file(path).unwrap();
    }

    /// Настоящий диспетчер учетных данных, именно он и отказывался от длительных сессий.
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
