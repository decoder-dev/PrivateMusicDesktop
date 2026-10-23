//! Native share/copy accepts VK resource links, not arbitrary text, media URLs or credentials.
use pm_engine::{Error, Result};
use tauri::AppHandle;
use url::Url;

fn locator(value: &str) -> bool {
    let Some((owner, id)) = value.split_once('_') else {
        return false;
    };
    owner.parse::<i64>().is_ok_and(|id| id != 0)
        && id.parse::<i64>().is_ok_and(|id| id > 0)
        && owner.bytes().all(|ch| ch.is_ascii_digit() || ch == b'-')
        && id.bytes().all(|ch| ch.is_ascii_digit())
}
fn checked_link(raw: &str) -> Result<Url> {
    if raw.len() > 2048 {
        return Err(Error::InvalidRequest);
    }
    let url = Url::parse(raw).map_err(|_| Error::InvalidRequest)?;
    if url.scheme() != "https"
        || url.host_str() != Some("vk.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidRequest);
    }
    if let Some(value) = url.path().strip_prefix("/audio") {
        if locator(value) && url.query().is_none() {
            return Ok(url);
        }
    }
    for prefix in ["/music/playlist/", "/music/album/"] {
        if let Some(value) = url.path().strip_prefix(prefix) {
            if !locator(value) {
                return Err(Error::InvalidRequest);
            }
            let query: Vec<_> = url.query_pairs().collect();
            if query.is_empty()
                || query.len() == 1
                    && query[0].0 == "access_key"
                    && !query[0].1.is_empty()
                    && query[0].1.len() <= 256
                    && query[0].1.bytes().all(|ch| ch.is_ascii_alphanumeric())
            {
                return Ok(url);
            }
        }
    }
    Err(Error::InvalidRequest)
}

pub fn share(handle: &AppHandle, raw: &str, title: &str) -> Result<()> {
    let url = checked_link(raw)?;
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_vk_login::VkLoginExt;
        let title: String = title
            .chars()
            .filter(|ch| !ch.is_control())
            .take(200)
            .collect();
        handle
            .vk_login()
            .share_link(url.as_str(), &title)
            .map_err(|_| Error::InvalidRequest)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (handle, url, title);
        Err(Error::InvalidRequest)
    }
}

pub fn copy(handle: &AppHandle, raw: &str) -> Result<()> {
    let url = checked_link(raw)?;
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_vk_login::VkLoginExt;
        handle
            .vk_login()
            .copy_link(url.as_str())
            .map_err(|_| Error::InvalidRequest)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (handle, url);
        Err(Error::InvalidRequest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_vk_resource_locators_and_optional_collection_access_key() {
        for link in [
            "https://vk.com/audio-7_4",
            "https://vk.com/music/playlist/7_1?access_key=abc123",
            "https://vk.com/music/album/-1_2",
        ] {
            assert!(checked_link(link).is_ok(), "{link}");
        }
        for link in [
            "https://evil.invalid/audio7_2",
            "http://vk.com/audio7_2",
            "https://vk.com/audio0_2",
            "https://vk.com/audio7_0",
            "https://vk.com/audio7_2?access_token=secret",
            "https://user@vk.com/audio7_2",
            "https://vk.com:123/audio7_2",
            "https://vk.com/music/album/7_1?access_key=abc&cookie=secret",
            "https://vk.com/music/playlist/7_1?access_key=https://evil.invalid",
            "https://vk.com/audio7_2#fragment",
        ] {
            assert!(checked_link(link).is_err(), "{link}");
        }
    }
}
