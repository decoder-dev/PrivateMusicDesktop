//! MP3 uploads use only VK's upload hosts; requests never follow redirects or replay a write.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde_json::Value;
use url::Url;

use super::{model, Session, Track, VkClient};
use crate::{Error, Result};

pub const MAX_AUDIO_BYTES: u64 = 200_000_000;
pub type CancelUpload = Arc<dyn Fn() -> bool + Send + Sync>;

pub fn accepted_upload_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && url.fragment().is_none()
        && url.host_str().is_some_and(|host| {
            [
                "vk.com",
                "vk.ru",
                "vkuseraudio.net",
                "vkuseraudio.com",
                "userapi.com",
                "vkuserphoto.ru",
                "vk-cdn.net",
            ]
            .iter()
            .any(|root| host == *root || host.ends_with(&format!(".{root}")))
        })
}

pub fn checked_mp3(path: &Path) -> Result<(File, u64)> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::InvalidRequest)?;
    if !metadata.is_file() || metadata.len() < 3 || metadata.len() > MAX_AUDIO_BYTES {
        return Err(Error::InvalidRequest);
    }
    let mut file = File::open(path).map_err(|_| Error::InvalidRequest)?;
    let mut header = [0u8; 3];
    file.read_exact(&mut header)
        .map_err(|_| Error::InvalidRequest)?;
    if header != *b"ID3" && !(header[0] == 0xff && header[1] & 0xe0 == 0xe0) {
        return Err(Error::InvalidRequest);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Error::InvalidRequest)?;
    Ok((file, metadata.len()))
}

struct UploadReader {
    file: File,
    remaining: u64,
    cancelled: CancelUpload,
}
impl Read for UploadReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "audio upload cancelled",
            ));
        }
        let limit = buffer.len().min(self.remaining as usize);
        let count = self.file.read(&mut buffer[..limit])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

fn scalar(value: &Value, name: &str) -> Result<String> {
    let value = match value.get(name) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => return Err(Error::Format("audio.upload".into())),
    };
    if value.is_empty() || value.len() > 64 * 1024 {
        return Err(Error::Format("audio.upload".into()));
    }
    Ok(value)
}

impl VkClient {
    pub fn audio_upload_server(&self, session: &Session) -> Result<Url> {
        let response = self.call(session, "audio.getUploadServer", &[])?;
        let url = response
            .get("upload_url")
            .and_then(Value::as_str)
            .and_then(|value| Url::parse(value).ok())
            .filter(accepted_upload_url)
            .ok_or_else(|| Error::Format("audio.getUploadServer".into()))?;
        Ok(url)
    }

    pub fn upload_audio_file(
        &self,
        url: Url,
        path: &Path,
        cancelled: CancelUpload,
    ) -> Result<Value> {
        if !accepted_upload_url(&url) {
            return Err(Error::InvalidRequest);
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let (file, bytes) = checked_mp3(path)?;
        let reader = UploadReader {
            file,
            remaining: bytes,
            cancelled: cancelled.clone(),
        };
        let part = multipart::Part::reader_with_length(reader, bytes)
            .file_name("audio.mp3")
            .mime_str("audio/mpeg")
            .map_err(|_| Error::InvalidRequest)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(Error::from)?;
        let response = client
            .post(url)
            .multipart(multipart::Form::new().part("file", part))
            .send()
            .map_err(|error| {
                if cancelled() {
                    Error::Cancelled
                } else {
                    Error::from(error.without_url())
                }
            })?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if !response.status().is_success() {
            return Err(Error::Http(response.status().as_u16()));
        }
        let mut body = Vec::new();
        response
            .take(256 * 1024 + 1)
            .read_to_end(&mut body)
            .map_err(|_| Error::Transport("audio upload response".into()))?;
        if body.len() > 256 * 1024 {
            return Err(Error::Format("audio.upload".into()));
        }
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| Error::Format("audio.upload".into()))?;
        for name in ["server", "audio", "hash"] {
            scalar(&value, name)?;
        }
        Ok(value)
    }

    pub fn save_audio_upload(
        &self,
        session: &Session,
        uploaded: &Value,
        artist: &str,
        title: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Track> {
        let artist = artist.trim();
        let title = title.trim();
        if artist.is_empty()
            || title.is_empty()
            || artist.chars().count() > 200
            || title.chars().count() > 200
        {
            return Err(Error::InvalidRequest);
        }
        // Resolve ownership before the write: a later read failure must not make a saved track look unsaved.
        let owner = self.user_id(session)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let response = self.call_once(
            session,
            "audio.save",
            &[
                ("server", scalar(uploaded, "server")?),
                ("audio", scalar(uploaded, "audio")?),
                ("hash", scalar(uploaded, "hash")?),
                ("artist", artist.into()),
                ("title", title.into()),
            ],
        )?;
        model::parse_track(&response, Some(owner))
            .filter(|track| track.track_id > 0)
            .ok_or_else(|| Error::Format("audio.save".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn upload_hosts_are_https_exact_hosts_or_subdomains_without_credentials() {
        for value in [
            "https://pu.vk.com/audio",
            "https://pu.vk.ru/audio",
            "https://cs.vkuseraudio.net/upload?key=secret",
            "https://userapi.com/a",
        ] {
            assert!(accepted_upload_url(&Url::parse(value).unwrap()), "{value}");
        }
        for value in [
            "http://vk.com/a",
            "https://vk.com.evil.invalid/a",
            "https://evilvk.com/a",
            "https://u:p@vk.com/a",
            "https://vk.com:123/a",
            "https://127.0.0.1/a",
            "https://vk.com/a#fragment",
        ] {
            assert!(!accepted_upload_url(&Url::parse(value).unwrap()), "{value}");
        }
    }
    #[test]
    fn staged_mp3_is_bounded_and_reader_observes_cancellation() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"ID3some audio bytes").unwrap();
        let (source, bytes) = checked_mp3(file.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let mut reader = UploadReader {
            file: source,
            remaining: bytes,
            cancelled: Arc::new(move || flag.load(Ordering::Acquire)),
        };
        let mut buffer = [0u8; 3];
        assert_eq!(reader.read(&mut buffer).unwrap(), 3);
        cancel.store(true, Ordering::Release);
        assert_eq!(
            reader.read(&mut buffer).unwrap_err().kind(),
            std::io::ErrorKind::ConnectionAborted
        );
        file.as_file().set_len(MAX_AUDIO_BYTES + 1).unwrap();
        assert!(checked_mp3(file.path()).is_err());
    }
    #[test]
    fn upload_fields_require_scalars_and_mp3_header_is_checked() {
        assert_eq!(
            scalar(&serde_json::json!({"server":12}), "server").unwrap(),
            "12"
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"hash":[]}),
            serde_json::json!({"hash":""}),
        ] {
            assert!(scalar(&value, "hash").is_err());
        }
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"not an mp3").unwrap();
        assert!(checked_mp3(file.path()).is_err());
    }
    #[test]
    fn cancelling_during_owner_lookup_prevents_the_final_save_request() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let worker = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert_eq!(request.url(), "/method/users.get");
            flag.store(true, Ordering::Release);
            request
                .respond(tiny_http::Response::from_string(
                    r#"{"response":[{"id":7,"first_name":"A","last_name":"B"}]}"#,
                ))
                .unwrap();
            assert!(server
                .recv_timeout(Duration::from_millis(100))
                .unwrap()
                .is_none());
        });
        let client = VkClient::with_base(&format!("http://{address}")).unwrap();
        let session = Session {
            access_token: "test-token".into(),
            user_id: None,
            expires_at: None,
            api_user_agent: "test".into(),
            refresh_cookie: String::new(),
            web_user_agent: "test".into(),
        };
        let result = client.save_audio_upload(
            &session,
            &serde_json::json!({"server":1,"audio":"audio","hash":"hash"}),
            "Artist",
            "Title",
            &|| cancelled.load(Ordering::Acquire),
        );
        assert_eq!(result.unwrap_err(), Error::Cancelled);
        worker.join().unwrap();
    }
}
