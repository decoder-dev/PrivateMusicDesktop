//! VK иногда отвечает на `audio.get` замаскированным потоком URL (`…audio_api_unavailable…?extra=…#…`).
//! Декодирование находится в общем собственном ядре; это безопасная оболочка для его C ABI.

use pm_native::media::{pm_vk_unmask, PM_VK_UNMASK_NOT_MASKED};

/// Возвращает воспроизводимый URL, ввод без изменений, если он никогда не был замаскирован, или `None`,
/// когда он замаскирован и не может быть декодирован (например, без идентификатора пользователя).
pub fn resolve(url: &str, user_id: Option<i64>) -> Option<String> {
    let bytes = url.as_bytes();
    if bytes.is_empty() || bytes.len() > i32::MAX as usize / 2 {
        return None;
    }
    let mut out = vec![0u8; bytes.len().max(16)];
    // SAFETY: оба фрагмента действительны для переданной длины; встроенная функция никогда не
    // записывает более `out_capacity` байт.
    let written = unsafe {
        pm_vk_unmask(
            bytes.as_ptr(),
            bytes.len() as i32,
            user_id.unwrap_or(0).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            out.as_mut_ptr(),
            out.len() as i32,
        )
    };
    if written == PM_VK_UNMASK_NOT_MASKED {
        return Some(url.to_string());
    }
    if written <= 0 || user_id.is_none() {
        return None;
    }
    out.truncate(written as usize);
    String::from_utf8(out).ok().and_then(|s| secure_remote_url(&s))
}

/// Только https: `//host/…` получает схему, простой http обновляется, все остальное (файл:,
/// javascript:, пользовательские схемы) отклоняется.
pub fn secure_remote_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let schemed = if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_string()
    };
    let mut url = url::Url::parse(&schemed).ok()?;
    url.host_str()?;
    match url.scheme() {
        "https" => {}
        // Loopback остается в покое, поэтому тесты могут выполняться с адреса 127.0.0.1. Он никогда не
        // извлекается, если сборщик явно не переведен в тестовый режим.
        "http" if matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) => {}
        "http" => url.set_scheme("https").ok()?,
        _ => return None,
    }
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmasked_url_passes_through() {
        let url = "https://cs1.vkuseraudio.net/s/v1/acmp/abc/index.m3u8?siren=1";
        assert_eq!(resolve(url, Some(1)).as_deref(), Some(url));
    }

    #[test]
    fn masked_url_without_user_is_rejected() {
        let masked = "https://vk.com/mp3/audio_api_unavailable.mp3?extra=aaaa#bbbb";
        assert_eq!(resolve(masked, None), None);
    }

    #[test]
    fn secure_remote_url_rules() {
        assert_eq!(secure_remote_url("//a.example/x").as_deref(), Some("https://a.example/x"));
        assert_eq!(secure_remote_url("http://a.example/x").as_deref(), Some("https://a.example/x"));
        assert_eq!(secure_remote_url("http://127.0.0.1:8/x").as_deref(), Some("http://127.0.0.1:8/x"));
        assert_eq!(secure_remote_url("file:///etc/passwd"), None);
        assert_eq!(secure_remote_url("javascript:alert(1)"), None);
        assert_eq!(secure_remote_url("  "), None);
        assert_eq!(secure_remote_url("https://"), None);
    }
}
