//! Explicit bounded text copies; no paths, network requests or relaxed VK-link validation.
use pm_engine::{Error, Result};
use tauri::AppHandle;

fn checked(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > 150_000 {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}
pub fn copy(handle: &AppHandle, text: &str) -> Result<()> {
    checked(text)?;
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_vk_login::VkLoginExt;
        handle.vk_login().copy_text(text).map_err(|_| Error::Unavailable)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = handle;
        Err(Error::InvalidRequest)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_copies_are_nonempty_and_utf8_bounded() {
        assert!(checked("Первая строка\nВторая строка").is_ok());
        assert!(checked(&"я".repeat(75_000)).is_ok());
        for text in [String::new(), " \n\t".into(), "я".repeat(75_001)] {
            assert!(checked(&text).is_err());
        }
    }
}
