//! Release discovery and bounded, checksum-verified downloads. No webview navigation.
use reqwest::{
    blocking::{Client, Response},
    header::{ACCEPT, AUTHORIZATION},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

const API: &str = "https://api.github.com/repos/decoder-dev/PrivateMusicDesktop/releases/latest";
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    pub phase: String,
    pub current: String,
    pub version: String,
    pub notes: String,
    pub name: String,
    pub total: u64,
    pub downloaded: u64,
    pub error: String,
    pub access_saved: bool,
}
#[derive(Clone, Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone)]
struct Plan {
    installer: Asset,
    sums: Asset,
}
#[derive(Default)]
struct Inner {
    snapshot: Snapshot,
    plan: Option<Plan>,
    ready: Option<(PathBuf, String)>,
}

pub struct Updater {
    inner: Mutex<Inner>,
    busy: AtomicBool,
    cancelled: AtomicBool,
    directory: PathBuf,
}
impl Updater {
    pub fn new(directory: PathBuf, current: &str) -> Self {
        Self {
            inner: Mutex::new(Inner {
                snapshot: Snapshot { phase: "idle".into(), current: current.into(), ..Default::default() },
                ..Default::default()
            }),
            busy: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            directory,
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).snapshot.clone()
    }
    fn change(&self, f: impl FnOnce(&mut Inner)) {
        f(&mut self.inner.lock().unwrap_or_else(|p| p.into_inner()));
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    fn running(&self) -> Result<Running<'_>> {
        self.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).map_err(|_| "busy")?;
        self.cancelled.store(false, Ordering::SeqCst);
        Ok(Running(self))
    }
    fn active(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err("cancelled")
        } else {
            Ok(())
        }
    }
    fn finish(&self, result: Result<()>, emit: &impl Fn(Snapshot)) -> Snapshot {
        if let Err(code) = result {
            self.change(|i| {
                i.snapshot.phase = if code == "cancelled" { "cancelled" } else { "error" }.into();
                i.snapshot.error = code.into();
            });
        }
        let snapshot = self.snapshot();
        emit(snapshot.clone());
        snapshot
    }
    pub fn set_token(&self, token: &str) -> Result<Snapshot> {
        let _guard = self.running()?;
        let token = token.trim();
        if token.len() > 512 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("token");
        }
        crate::store::update_token(Some(token)).map_err(|_| "secret")?;
        self.change(|i| i.snapshot.access_saved = !token.is_empty());
        Ok(self.snapshot())
    }
    pub fn refresh_access(&self) -> Snapshot {
        let saved = crate::store::load_update_token().ok().flatten().is_some();
        self.change(|i| i.snapshot.access_saved = saved);
        self.snapshot()
    }
    pub fn check(&self, emit: impl Fn(Snapshot)) -> Result<Snapshot> {
        let _guard = self.running()?;
        self.change(|i| {
            i.plan = None;
            i.ready = None;
            let current = i.snapshot.current.clone();
            let access_saved = i.snapshot.access_saved;
            i.snapshot = Snapshot { phase: "checking".into(), current, access_saved, ..Default::default() };
        });
        emit(self.snapshot());
        let result = (|| {
            let token = crate::store::load_update_token().ok().flatten();
            self.change(|i| i.snapshot.access_saved = token.is_some());
            let client = client()?;
            let bytes = read_small(response(&client, API, token.as_deref(), false)?, 2 * 1024 * 1024)?;
            self.active()?;
            let release: Release = serde_json::from_slice(&bytes).map_err(|_| "release")?;
            let current = self.snapshot().current;
            let version = release.tag_name.trim_start_matches('v');
            if release.draft || release.prerelease || !newer(version, &current)? {
                self.change(|i| i.snapshot.phase = "current".into());
                return Ok(());
            }
            let plan = select(&release, platform())?;
            self.change(|i| {
                i.snapshot.phase = "available".into();
                i.snapshot.version = version.into();
                i.snapshot.notes = release.body.unwrap_or_default().chars().take(12000).collect();
                i.snapshot.name = plan.installer.name.clone();
                i.snapshot.total = plan.installer.size;
                i.plan = Some(plan);
            });
            Ok(())
        })();
        Ok(self.finish(result, &emit))
    }
    pub fn download(&self, emit: impl Fn(Snapshot)) -> Result<Snapshot> {
        let _guard = self.running()?;
        let plan = self.inner.lock().unwrap_or_else(|p| p.into_inner()).plan.clone().ok_or("release")?;
        self.change(|i| {
            i.ready = None;
            i.snapshot.phase = "downloading".into();
            i.snapshot.error.clear();
            i.snapshot.downloaded = 0;
        });
        emit(self.snapshot());
        let part = self.directory.join(format!("{}.part", plan.installer.name));
        let result = (|| {
            self.active()?;
            fs::create_dir_all(&self.directory).map_err(|_| "disk")?;
            let token = crate::store::load_update_token().ok().flatten();
            let client = client()?;
            let sums = read_small(response(&client, &asset_url(plan.sums.id), token.as_deref(), true)?, 64 * 1024)?;
            let hash = checksum(&sums, &plan.installer.name)?;
            self.active()?;
            let mut response = response(&client, &asset_url(plan.installer.id), token.as_deref(), true)?;
            if response.content_length().is_some_and(|n| n != plan.installer.size) {
                return Err("size");
            }
            let mut file = File::create(&part).map_err(|_| "disk")?;
            let mut digest = Sha256::new();
            let mut received = 0u64;
            let mut last = Instant::now();
            let mut buffer = [0; 64 * 1024];
            loop {
                self.active()?;
                let count = response.read(&mut buffer).map_err(|_| "network")?;
                if count == 0 {
                    break;
                }
                received += count as u64;
                if received > plan.installer.size || received > MAX_INSTALLER {
                    return Err("size");
                }
                file.write_all(&buffer[..count]).map_err(|_| "disk")?;
                digest.update(&buffer[..count]);
                self.change(|i| i.snapshot.downloaded = received);
                if last.elapsed() >= Duration::from_millis(100) {
                    emit(self.snapshot());
                    last = Instant::now();
                }
            }
            self.active()?;
            self.change(|i| i.snapshot.phase = "verifying".into());
            emit(self.snapshot());
            if received != plan.installer.size {
                return Err("size");
            }
            if format!("{:x}", digest.finalize()) != hash {
                return Err("checksum");
            }
            file.sync_all().map_err(|_| "disk")?;
            drop(file);
            let target = self.directory.join(&plan.installer.name);
            // A previous verified download may be replaced; partial files never appear ready.
            if target.exists() {
                fs::remove_file(&target).map_err(|_| "disk")?;
            }
            fs::rename(&part, &target).map_err(|_| "disk")?;
            self.change(|i| {
                i.ready = Some((target, hash));
                i.snapshot.phase = "ready".into();
            });
            Ok(())
        })();
        let _ = fs::remove_file(part);
        Ok(self.finish(result, &emit))
    }
    /// Revalidate immediately before the native installer is opened.
    pub fn verified_file(&self) -> Result<PathBuf> {
        let _guard = self.running()?;
        let (path, hash) = self.inner.lock().unwrap_or_else(|p| p.into_inner()).ready.clone().ok_or("release")?;
        let mut file = File::open(&path).map_err(|_| "disk")?;
        if file.metadata().map_err(|_| "disk")?.len() > MAX_INSTALLER {
            return Err("size");
        }
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(|_| "disk")?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
        }
        if format!("{:x}", digest.finalize()) != hash {
            return Err("checksum");
        }
        Ok(path)
    }
}
struct Running<'a>(&'a Updater);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}
fn platform() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x64-setup.exe"
    } else if cfg!(target_os = "macos") {
        "macos-universal.dmg"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-amd64.AppImage"
    } else {
        "unsupported"
    }
}
fn newer(candidate: &str, current: &str) -> Result<bool> {
    let next = semver::Version::parse(candidate).map_err(|_| "release")?;
    let current = semver::Version::parse(current).map_err(|_| "release")?;
    Ok(next.pre.is_empty() && next > current)
}
fn select(release: &Release, platform: &str) -> Result<Plan> {
    let name = format!("PrivateMusic_{}_{}", release.tag_name.trim_start_matches('v'), platform);
    let installer = release
        .assets
        .iter()
        .find(|a| a.name == name && a.id > 0 && a.size > 0 && a.size <= MAX_INSTALLER)
        .cloned()
        .ok_or("platform")?;
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt" && a.id > 0 && a.size > 0 && a.size <= 64 * 1024)
        .cloned()
        .ok_or("checksum")?;
    Ok(Plan { installer, sums })
}
fn checksum(bytes: &[u8], filename: &str) -> Result<String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "checksum")?;
    let mut found = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(hash), Some(name)) = (fields.next(), fields.next()) else {
            continue;
        };
        if name.trim_start_matches('*') == filename {
            if found.is_some()
                || fields.next().is_some()
                || hash.len() != 64
                || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("checksum");
            }
            found = Some(hash.to_ascii_lowercase());
        }
    }
    found.ok_or("checksum")
}
fn trusted(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}
fn client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!("PrivateMusic/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !trusted(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| "network")
}
fn asset_url(id: u64) -> String {
    format!("https://api.github.com/repos/decoder-dev/PrivateMusicDesktop/releases/assets/{id}")
}
fn response(client: &Client, url: &str, token: Option<&str>, binary: bool) -> Result<Response> {
    let mut request =
        client.get(url).header(ACCEPT, if binary { "application/octet-stream" } else { "application/vnd.github+json" });
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = request.send().map_err(|_| "network")?;
    if response.status().as_u16() == 403
        && response.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()) == Some("0")
    {
        return Err("rate");
    }
    match response.status().as_u16() {
        200 => Ok(response),
        401 | 403 | 404 => Err("access"),
        429 => Err("rate"),
        _ => Err("network"),
    }
}
fn read_small(response: Response, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response.take(limit + 1).read_to_end(&mut bytes).map_err(|_| "network")?;
    if bytes.len() as u64 > limit {
        return Err("release");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "downloads the latest public installer from GitHub"]
    fn public_release_downloads_and_verifies_without_browser_or_token() {
        let dir = tempfile::tempdir().unwrap();
        let updater = Updater::new(dir.path().into(), "0.0.1");
        let found = updater.check(|_| {}).unwrap();
        assert_eq!(found.phase, "available", "{}", found.error);
        let ready = updater.download(|_| {}).unwrap();
        assert_eq!(ready.phase, "ready", "{}", ready.error);
        assert!(updater.verified_file().unwrap().is_file());
        assert!(!dir.path().join(format!("{}.part", ready.name)).exists());
    }
    #[test]
    fn cancel_before_download_never_requests_or_leaves_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let updater = Updater::new(dir.path().into(), "0.3.0");
        updater.change(|i| {
            i.plan = Some(Plan {
                installer: Asset { id: 1, name: "installer.exe".into(), size: 10 },
                sums: Asset { id: 2, name: "SHA256SUMS.txt".into(), size: 70 },
            })
        });
        let result = updater
            .download(|snapshot| {
                if snapshot.phase == "downloading" {
                    updater.cancel();
                }
            })
            .unwrap();
        assert_eq!(result.phase, "cancelled");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(updater.verified_file().is_err());
    }
    #[test]
    fn versions_never_downgrade_or_install_previews() {
        assert!(newer("0.4.0", "0.3.0").unwrap());
        for v in ["0.2.9", "0.3.0", "0.4.0-beta.1"] {
            assert!(!newer(v, "0.3.0").unwrap());
        }
        assert!(newer("../../bad", "0.3.0").is_err());
    }
    #[test]
    fn exact_platform_asset_and_bounded_size_required() {
        let mut release = Release {
            tag_name: "v0.4.0".into(),
            body: None,
            draft: false,
            prerelease: false,
            assets: vec![
                Asset { id: 1, name: "PrivateMusic_0.4.0_windows-x64-setup.exe".into(), size: 100 },
                Asset { id: 2, name: "SHA256SUMS.txt".into(), size: 200 },
            ],
        };
        assert!(select(&release, "windows-x64-setup.exe").is_ok());
        assert!(select(&release, "macos-universal.dmg").is_err());
        release.assets[0].size = MAX_INSTALLER + 1;
        assert!(select(&release, "windows-x64-setup.exe").is_err());
    }
    #[test]
    fn hash_manifest_requires_one_exact_filename() {
        let hash = "a".repeat(64);
        assert_eq!(checksum(format!("{hash}  installer.exe\n").as_bytes(), "installer.exe"), Ok(hash.clone()));
        assert!(checksum(format!("{hash}  ../installer.exe\n").as_bytes(), "installer.exe").is_err());
        assert!(checksum(format!("{hash}  installer.exe\n{hash}  installer.exe").as_bytes(), "installer.exe").is_err());
        assert!(checksum(b"wrong installer.exe", "installer.exe").is_err());
    }
    #[test]
    fn download_redirects_stay_on_github_https() {
        for value in ["https://api.github.com/x", "https://release-assets.githubusercontent.com/x"] {
            assert!(trusted(&url::Url::parse(value).unwrap()));
        }
        for value in [
            "http://github.com/x",
            "https://github.com.evil.test/x",
            "https://github.com:444/x",
            "https://user@github.com/x",
            "http://127.0.0.1/x",
        ] {
            assert!(!trusted(&url::Url::parse(value).unwrap()));
        }
    }
    #[test]
    fn downloaded_file_is_rechecked_and_operations_are_serialized() {
        let dir = tempfile::tempdir().unwrap();
        let updater = Updater::new(dir.path().into(), "0.3.0");
        let guard = updater.running().unwrap();
        assert!(updater.running().is_err());
        updater.cancel();
        assert_eq!(updater.active(), Err("cancelled"));
        drop(guard);
        assert!(updater.running().is_ok());
        let path = dir.path().join("installer.exe");
        fs::write(&path, b"verified").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"verified"));
        updater.change(|i| i.ready = Some((path.clone(), hash)));
        assert_eq!(updater.verified_file().unwrap(), path);
        fs::write(path, b"tampered").unwrap();
        assert_eq!(updater.verified_file(), Err("checksum"));
    }
}
