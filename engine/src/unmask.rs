//! VK sometimes answers `audio.get` with a masked stream URL
//! (`…audio_api_unavailable…?extra=…#…`). The decoding lives in the shared
//! native core; this is the safe wrapper around its C ABI.

use pm_native::media::{pm_vk_unmask, PM_VK_UNMASK_NOT_MASKED};

/// Returns the playable URL, the input unchanged when it was never masked,
/// or `None` when it is masked and cannot be decoded (for example without a
/// user id).
pub fn resolve(url: &str, user_id: Option<i64>) -> Option<String> {
    let bytes = url.as_bytes();
    if bytes.is_empty() || bytes.len() > i32::MAX as usize / 2 {
        return None;
    }
    let mut out = vec![0u8; bytes.len().max(16)];
    // SAFETY: both slices are valid for the lengths passed; the native
    // function never writes more than `out_capacity` bytes.
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

/// https only: `//host/…` gets a scheme, plain http is upgraded, anything
/// else (file:, javascript:, custom schemes) is rejected.
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
        assert_eq!(secure_remote_url("file:///etc/passwd"), None);
        assert_eq!(secure_remote_url("javascript:alert(1)"), None);
        assert_eq!(secure_remote_url("  "), None);
        assert_eq!(secure_remote_url("https://"), None);
    }
}
