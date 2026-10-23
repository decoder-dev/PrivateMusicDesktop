//! Playlist cover writes use VK's upload result directly, without adding a photo to an album.
use super::{
    upload::{accepted_upload_url, CancelUpload},
    Playlist, Session, VkClient,
};
use crate::{Error, Result};
use reqwest::blocking::{multipart, Client};
use serde_json::Value;
use std::{
    io::{Cursor, Read},
    time::Duration,
};
use url::Url;

pub const MAX_COVER_BYTES: usize = 10_000_000;
pub fn checked_jpeg(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 5
        || bytes.len() > MAX_COVER_BYTES
        || !bytes.starts_with(&[0xff, 0xd8, 0xff])
        || !bytes.ends_with(&[0xff, 0xd9])
    {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}
fn fields(uploaded: &Value) -> Result<(String, String)> {
    let field = |name: &str| -> Result<String> {
        let value = match uploaded.get(name) {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => return Err(Error::Format("playlist cover upload".into())),
        };
        if value.is_empty() || value.len() > 64 * 1024 {
            return Err(Error::Format("playlist cover upload".into()));
        }
        Ok(value)
    };
    Ok((field("photo")?, field("hash")?))
}
struct ImageReader {
    bytes: Cursor<Vec<u8>>,
    cancelled: CancelUpload,
}
impl Read for ImageReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "cover upload cancelled",
            ));
        }
        self.bytes.read(buffer)
    }
}
fn post_cover(url: Url, jpeg: Vec<u8>, cancelled: CancelUpload) -> Result<Value> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let length = jpeg.len() as u64;
    let part = multipart::Part::reader_with_length(
        ImageReader {
            bytes: Cursor::new(jpeg),
            cancelled: cancelled.clone(),
        },
        length,
    )
    .file_name("cover.jpg")
    .mime_str("image/jpeg")
    .map_err(|_| Error::InvalidRequest)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(Error::from)?;
    let response = client
        .post(url)
        .multipart(multipart::Form::new().part("photo", part))
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
    let mut bytes = Vec::new();
    response
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Transport("cover upload response".into()))?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if bytes.len() > 256 * 1024 {
        return Err(Error::Format("cover upload response".into()));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Format("cover upload response".into()))?;
    fields(&value)?;
    Ok(value)
}
impl VkClient {
    pub fn playlist_cover_server(&self, session: &Session, playlist: &Playlist) -> Result<Url> {
        self.own_playlist(session, playlist)?;
        let response = self.call(
            session,
            "photos.getAudioPlaylistCoverUploadServer",
            &[
                ("owner_id", playlist.owner_id.to_string()),
                ("playlist_id", playlist.playlist_id.to_string()),
            ],
        )?;
        response
            .get("upload_url")
            .and_then(Value::as_str)
            .and_then(|raw| Url::parse(raw).ok())
            .filter(accepted_upload_url)
            .ok_or_else(|| Error::Format("playlist cover server".into()))
    }
    pub fn upload_playlist_cover(
        &self,
        url: Url,
        jpeg: Vec<u8>,
        cancelled: CancelUpload,
    ) -> Result<Value> {
        if !accepted_upload_url(&url) {
            return Err(Error::InvalidRequest);
        }
        checked_jpeg(&jpeg)?;
        post_cover(url, jpeg, cancelled)
    }
    pub fn save_playlist_cover(
        &self,
        session: &Session,
        playlist: &Playlist,
        uploaded: &Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<()> {
        self.own_playlist(session, playlist)?;
        let (photo, hash) = fields(uploaded)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        self.call_once(
            session,
            "audio.setPlaylistCoverPhoto",
            &[
                ("playlist_owner_id", playlist.owner_id.to_string()),
                ("playlist_id", playlist.playlist_id.to_string()),
                ("photo", photo),
                ("hash", hash),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn accepts_bounded_jpeg_and_opaque_upload_scalars_only() {
        assert!(checked_jpeg(&[0xff, 0xd8, 0xff, 0xff, 0xd9]).is_ok());
        for bytes in [
            vec![],
            b"not a jpeg".to_vec(),
            vec![0xff; MAX_COVER_BYTES + 1],
        ] {
            assert!(checked_jpeg(&bytes).is_err());
        }
        assert_eq!(
            fields(&serde_json::json!({"photo":"opaque-photo","hash":123})).unwrap(),
            ("opaque-photo".into(), "123".into())
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"photo":[],"hash":"x"}),
            serde_json::json!({"photo":"x","hash":""}),
        ] {
            assert!(fields(&value).is_err());
        }
    }
    #[test]
    fn multipart_uses_the_photo_field_and_does_not_forward_login_headers() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap();
        let worker = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
            let mut body = Vec::new();
            request.as_reader().read_to_end(&mut body).unwrap();
            let body = String::from_utf8_lossy(&body);
            assert!(body.contains("name=\"photo\"; filename=\"cover.jpg\""));
            assert!(
                body.contains("Content-Type: image/jpeg")
                    || body.contains("content-type: image/jpeg")
            );
            assert!(!request
                .headers()
                .iter()
                .any(|header| header.field.equiv("Cookie") || header.field.equiv("Authorization")));
            request
                .respond(tiny_http::Response::from_string(
                    r#"{"photo":"opaque","hash":"hash"}"#,
                ))
                .unwrap();
        });
        let uploaded = post_cover(
            Url::parse(&format!("http://{address}/upload")).unwrap(),
            vec![0xff, 0xd8, 0xff, 0xff, 0xd9],
            Arc::new(|| false),
        )
        .unwrap();
        assert_eq!(uploaded["photo"], "opaque");
        worker.join().unwrap();
    }
}
