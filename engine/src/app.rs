//! Everything the window needs, in one object: sign-in state, VK data with
//! automatic token refresh, and the playback controller.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

use crate::audio::Sink;
use crate::controller::{Controller, UiEvent};
use crate::history::{History, HistoryEntry, StatsView};
use crate::lyrics::{from_plain, GeniusLyrics, LrcLib, Lyrics};
use crate::offline::{OfflineStore, OfflineTrack};
use crate::stream::fetch::fetch_to_end;
use crate::store::SecretStore;
use crate::stream::FetchConfig;
use crate::vk::auth::{exchange_at, now_unix};
use crate::vk::{model, Album, AlbumTracks, Artist, ArtistAlbums, MusicMix, Page, Playlist, Profile, Session, Track, VkClient};
use crate::{Error, Result};

const TOKEN_URL: &str = "https://login.vk.ru/?act=web_token";

/// Furthest offset the window may ask for in an artist or album list; VK's
/// own lists end long before.
const MAX_OFFSET: u32 = 100_000;

/// Upper bound for a search query from the window.
const MAX_QUERY: usize = 500;

fn checked_offset(offset: u32) -> Result<u32> {
    (offset <= MAX_OFFSET).then_some(offset).ok_or(Error::InvalidRequest)
}

fn checked_person(owner_id: i64) -> Result<i64> {
    (owner_id > 0).then_some(owner_id).ok_or(Error::InvalidRequest)
}

fn checked_artist_id(id: &str) -> Result<&str> {
    let id = id.trim();
    model::valid_artist_id(id).then_some(id).ok_or(Error::InvalidRequest)
}

fn checked_mix_id(id: &str) -> Result<&str> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .then_some(id).ok_or(Error::InvalidRequest)
}

fn checked_query(query: &str) -> Result<Option<&str>> {
    if query.len() > MAX_QUERY {
        return Err(Error::InvalidRequest);
    }
    let query = query.trim();
    Ok((!query.is_empty()).then_some(query))
}

fn checked_mix_options(options: Option<&str>) -> Result<Option<String>> {
    let Some(options) = options else { return Ok(None) };
    if options.len() > 8_192 {
        return Err(Error::InvalidRequest);
    }
    let value: Value = serde_json::from_str(options).map_err(|_| Error::InvalidRequest)?;
    if !value.is_object() {
        return Err(Error::InvalidRequest);
    }
    Ok(Some(value.to_string()))
}

/// An album handed over by the window: a real locator, and an access key or
/// follow hash that is a plain token, since both are sent back to VK.
fn checked_album(album: &Album) -> Result<()> {
    let token = |value: &Option<String>| {
        value.as_deref().is_none_or(|v| v.len() <= 256 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    };
    let artists_ok = album.artists.len() <= 50 && album.artists.iter().all(|a| a.name.len() <= 1024);
    if album.album_id <= 0
        || album.owner_id == 0
        || album.title.len() > 1024
        || !token(&album.access_key)
        || !token(&album.follow_hash)
        || !artists_ok
    {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionState {
    pub signed_in: bool,
    pub profile: Option<Profile>,
    /// False when the OS secret store failed, so the sign-in will not survive a restart.
    pub persistent: bool,
}

/// The app object the shell holds. It is cheap to share: everything lives in
/// [`Core`], which the controller can also reach to renew an expired token.
pub struct App {
    core: Arc<Core>,
}

impl std::ops::Deref for App {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

pub struct Core {
    controller: Controller,
    store: Box<dyn SecretStore>,
    emit: Arc<dyn Fn(UiEvent) + Send + Sync>,
    profile: Mutex<Option<Profile>>,
    persistent: Mutex<bool>,
    /// One refresh at a time: two screens hitting 401 together must not both
    /// re-issue the token.
    refresh_lock: Mutex<()>,
    /// Bumped by every sign-out, under this lock. A refresh that started
    /// before a sign-out must not sign the user back in when it finishes.
    generation: Mutex<u64>,
    token_url: String,
    lrclib: Option<LrcLib>,
    genius: Option<GeniusLyrics>,
    offline: Option<Arc<OfflineStore>>,
    history: Arc<History>,
}

pub struct AppOptions {
    /// Where downloads are kept; `None` disables them.
    pub offline_dir: Option<std::path::PathBuf>,
    /// Where the play history and statistics are kept; `None` keeps them in
    /// memory only.
    pub history_dir: Option<std::path::PathBuf>,
    pub lyrics_base: String,
    pub fetch: FetchConfig,
    pub token_url: String,
    pub volume: f32,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            fetch: FetchConfig::new("PrivateMusicDesktop"),
            token_url: TOKEN_URL.to_string(),
            volume: 1.0,
            lyrics_base: crate::lyrics::LRCLIB_BASE.to_string(),
            offline_dir: None,
            history_dir: None,
        }
    }
}

impl App {
    pub fn new<F>(
        client: VkClient,
        make_sink: F,
        store: Box<dyn SecretStore>,
        emit: impl Fn(UiEvent) + Send + Sync + 'static,
        options: AppOptions,
    ) -> Self
    where
        F: FnMut() -> Result<Box<dyn Sink>> + Send + 'static,
    {
        let emit: Arc<dyn Fn(UiEvent) + Send + Sync> = Arc::new(emit);
        let controller_emit = emit.clone();
        let controller = Controller::with_fetch(
            client,
            make_sink,
            move |event| controller_emit(event),
            options.volume,
            options.fetch,
        );
        let durable = store.is_durable();
        let app = Core {
            controller,
            store,
            emit,
            profile: Mutex::new(None),
            persistent: Mutex::new(durable),
            refresh_lock: Mutex::new(()),
            generation: Mutex::new(0),
            token_url: options.token_url,
            lrclib: LrcLib::with_base(&options.lyrics_base).ok(),
            genius: GeniusLyrics::new().ok(),
            offline: options.offline_dir.and_then(|dir| {
                OfflineStore::open(dir).map_err(|e| log::warn!("downloads unavailable: {e}")).ok().map(Arc::new)
            }),
            history: History::open(options.history_dir),
        };
        app.controller.set_history(app.history.clone());
        if let Some(store) = &app.offline {
            app.controller.set_offline(store.clone());
        }
        match app.store.load() {
            Ok(Some(session)) => {
                if session.user_id.is_some() {
                    app.history.set_account(session.user_id);
                }
                app.controller.set_session(Some(session));
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!("secret store unavailable: {error}");
                *app.persistent.lock().unwrap_or_else(|p| p.into_inner()) = false;
            }
        }
        let core = Arc::new(app);
        let weak = Arc::downgrade(&core);
        core.controller.set_refresher(Arc::new(move |stale: &Session| {
            weak.upgrade().ok_or(Error::Unauthorized)?.refresh(stale)
        }));
        App { core }
    }
}

impl Core {
    pub fn controller(&self) -> &Controller {
        &self.controller
    }

    fn lock<'a, T>(mutex: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
        mutex.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Adopts a fresh session (just signed in, or just refreshed).
    pub fn sign_in(&self, session: Session) {
        let _generation = Self::lock(&self.generation);
        self.adopt(session);
    }

    fn adopt(&self, session: Session) {
        let saved = match self.store.save(&session) {
            Ok(()) => self.store.is_durable(),
            Err(error) => {
                log::warn!("could not save the session: {error}");
                false
            }
        };
        *Self::lock(&self.persistent) = saved;
        *Self::lock(&self.profile) = None;
        // A refreshed token may not name its user: keep the account we know.
        if session.user_id.is_some() {
            self.history.set_account(session.user_id);
        }
        self.controller.set_session(Some(session));
    }

    pub fn sign_out(&self) {
        let mut generation = Self::lock(&self.generation);
        *generation += 1;
        if let Err(error) = self.store.clear() {
            log::warn!("could not clear the session: {error}");
        }
        *Self::lock(&self.profile) = None;
        self.history.set_account(None);
        self.controller.set_session(None);
    }

    pub fn session_state(&self) -> SessionState {
        SessionState {
            signed_in: self.controller.session().is_some(),
            profile: Self::lock(&self.profile).clone(),
            persistent: *Self::lock(&self.persistent),
        }
    }

    /// Re-issues the token from the stored web cookie.
    fn refresh(&self, stale: &Session) -> Result<Session> {
        let _one_at_a_time = Self::lock(&self.refresh_lock);
        let started = *Self::lock(&self.generation);
        // Someone else may have refreshed while we waited for the lock.
        if let Some(current) = self.controller.session() {
            if current.access_token != stale.access_token {
                return Ok(current);
            }
        }
        match exchange_at(&self.token_url, &stale.refresh_cookie, &stale.web_user_agent, now_unix()) {
            Ok(fresh) => {
                let generation = Self::lock(&self.generation);
                if *generation != started || self.controller.session().is_none() {
                    // Signed out while VK was answering: stay signed out.
                    return Err(Error::Unauthorized);
                }
                self.adopt(fresh.clone());
                drop(generation);
                Ok(fresh)
            }
            Err(Error::Unauthorized) => {
                if *Self::lock(&self.generation) != started {
                    return Err(Error::Unauthorized);
                }
                self.sign_out();
                (self.emit)(UiEvent::SessionExpired);
                Err(Error::Unauthorized)
            }
            Err(other) => Err(other),
        }
    }

    /// Runs a VK call with a valid token: refreshed ahead of expiry, and once
    /// more if VK answers 401 anyway.
    pub fn api<T>(&self, call: impl Fn(&VkClient, &Session) -> Result<T>) -> Result<T> {
        let mut session = self.controller.session().ok_or(Error::Unauthorized)?;
        if session.needs_refresh(now_unix()) {
            session = self.refresh(&session).unwrap_or(session);
        }
        match call(self.controller.client(), &session) {
            Err(Error::Unauthorized) => {
                let fresh = self.refresh(&session)?;
                call(self.controller.client(), &fresh)
            }
            other => other,
        }
    }

    pub fn profile(&self) -> Result<Profile> {
        if let Some(profile) = Self::lock(&self.profile).clone() {
            return Ok(profile);
        }
        let profile = self.api(|client, session| client.profile(session))?;
        // A session that did not name its user: the history starts now.
        if self.history.account().is_none() && self.controller.session().is_some() {
            self.history.set_account(Some(profile.id));
        }
        *Self::lock(&self.profile) = Some(profile.clone());
        Ok(profile)
    }

    pub fn library(&self, offset: u32, count: u32) -> Result<Page<Track>> {
        self.api(|client, session| client.library(session, offset, count.clamp(1, 300)))
    }

    pub fn search(&self, query: &str, offset: u32, count: u32) -> Result<Page<Track>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Page { items: Vec::new(), total: 0, next_offset: None });
        }
        self.api(|client, session| client.search(session, query, offset, count.clamp(1, 300)))
    }

    fn offline(&self) -> Result<&OfflineStore> {
        self.offline.as_deref().ok_or_else(|| Error::Stream("downloads are not available".into()))
    }

    pub fn downloads(&self) -> Vec<OfflineTrack> {
        self.offline.as_ref().map(|o| o.list()).unwrap_or_default()
    }

    pub fn downloads_size(&self) -> u64 {
        self.offline.as_ref().map(|o| o.total_bytes()).unwrap_or(0)
    }

    /// Downloads a track for listening without a connection. A fresh stream
    /// address is asked for first: the one the window holds may have expired.
    pub fn download(&self, track: &Track) -> Result<OfflineTrack> {
        let store = self.offline()?;
        if let Some(existing) = store.list().into_iter().find(|e| e.track.id() == track.id()) {
            return Ok(existing);
        }
        let fresh = self.api(|client, session| client.refreshed_track(session, track))?;
        let url = fresh.stream_url.as_deref().ok_or_else(|| Error::Format("no stream address".into()))?;
        let (kind, audio) = fetch_to_end(url, &self.controller.fetch_config())?;
        // Keep the metadata the window showed (title, cover) over VK's echo.
        store.save(&Track { stream_url: None, ..track.clone() }, kind, &audio)
    }

    pub fn remove_download(&self, track_id: &str) -> Result<bool> {
        self.offline()?.remove(track_id)
    }

    /// The personal mix: two pages, so a session lasts a while.
    pub fn mix(&self) -> Result<Vec<Track>> {
        self.mix_with_options(None)
    }

    pub fn mix_with_options(&self, options: Option<&str>) -> Result<Vec<Track>> {
        let options = checked_mix_options(options)?;
        self.api(|client, session| {
            let mut tracks = client.mix_with_options(session, 0, 50, options.as_deref())?;
            if let Ok(more) = client.mix_with_options(session, 50, 50, options.as_deref()) {
                let known: std::collections::HashSet<String> = tracks.iter().map(Track::id).collect();
                tracks.extend(more.into_iter().filter(|t| !known.contains(&t.id())));
            }
            Ok(tracks)
        })
    }

    pub fn mix_settings(&self) -> Result<Value> {
        self.api(|client, session| client.mix_settings(session))
    }

    pub fn catalog_mixes(&self) -> Result<Vec<MusicMix>> {
        self.api(|client, session| client.catalog_mixes(session))
    }

    pub fn stream_mix(&self, mix_id: &str, options: Option<&str>) -> Result<Vec<Track>> {
        let mix_id = checked_mix_id(mix_id)?.to_string();
        let options = checked_mix_options(options)?;
        self.api(|client, session| {
            let mut tracks = client.stream_mix(session, &mix_id, 0, 50, options.as_deref())?;
            if let Ok(more) = client.stream_mix(session, &mix_id, 50, 50, options.as_deref()) {
                let mut seen: std::collections::HashSet<String> = tracks.iter().map(Track::id).collect();
                tracks.extend(more.into_iter().filter(|track| seen.insert(track.id())));
            }
            Ok(tracks)
        })
    }

    pub fn recommendations(&self) -> Result<Vec<Track>> {
        self.api(|client, session| client.recommendations(session, None))
    }

    /// "More like this": VK's radio seeded with one track.
    pub fn similar(&self, track: &Track) -> Result<Vec<Track>> {
        self.api(|client, session| client.recommendations(session, Some(track)))
    }

    pub fn create_playlist(&self, title: &str, description: &str) -> Result<Playlist> {
        self.api(|client, session| client.create_playlist(session, title, description))
    }

    pub fn edit_playlist(&self, playlist: &Playlist, title: &str, description: &str) -> Result<Playlist> {
        self.api(|client, session| client.edit_playlist(session, playlist, title, description))
    }

    pub fn delete_playlist(&self, playlist: &Playlist) -> Result<()> {
        self.api(|client, session| client.delete_playlist(session, playlist))
    }

    pub fn add_to_playlist(&self, playlist: &Playlist, track: &Track) -> Result<()> {
        self.api(|client, session| client.change_playlist(session, playlist, track, true))
    }

    pub fn remove_from_playlist(&self, playlist: &Playlist, track: &Track) -> Result<()> {
        self.api(|client, session| client.change_playlist(session, playlist, track, false))
    }

    pub fn add_to_library(&self, track: &Track) -> Result<Track> {
        self.api(|client, session| client.add_track(session, track))
    }

    pub fn remove_from_library(&self, track: &Track) -> Result<()> {
        self.api(|client, session| client.delete_track(session, track))
    }

    /// Lyrics for a track: timed LRCLIB first, then the exact VK lyric id,
    /// Genius when its search result matches, then plain LRCLIB text.
    pub fn lyrics(&self, track: &Track) -> Result<Option<Lyrics>> {
        let mut failure = None;
        let lrc = match self.lrclib.as_ref().map(|l| l.lookup(track)) {
            Some(Ok(found)) => found,
            Some(Err(error)) => {
                log::debug!("lrclib: {error}");
                failure = Some(error);
                None
            }
            None => None,
        };
        if lrc.as_ref().is_some_and(|l| !l.lines.is_empty()) {
            return Ok(lrc);
        }
        if let Some(id) = track.lyrics_id {
            match self.api(|client, session| client.lyrics_text(session, id)) {
                Ok(Some(text)) => {
                    if let Some(found) = from_plain("VK", &text) {
                        return Ok(Some(found));
                    }
                }
                Ok(None) => {}
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => failure = failure.or(Some(error)),
            }
        }
        if let Some(genius) = &self.genius {
            match genius.lookup(track) {
                Ok(Some(found)) => return Ok(Some(found)),
                Ok(None) => {}
                Err(error) => {
                    // Genius is an optional last-resort source. Its public
                    // endpoints can reject automated requests; that must not
                    // turn "no lyrics found" into a playback-facing error.
                    log::debug!("Genius lyrics unavailable: {error}");
                }
            }
        }
        match (lrc, failure) {
            (Some(found), _) => Ok(Some(found)),
            (None, Some(error)) => Err(error),
            (None, None) => Ok(None),
        }
    }

    // The play history and statistics live on this computer only.

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.history.entries()
    }

    pub fn history_remove(&self, track_id: &str) -> Result<bool> {
        if track_id.is_empty() || track_id.len() > 64 || !track_id.chars().all(|c| c.is_ascii_digit() || c == '_' || c == '-') {
            return Err(Error::InvalidRequest);
        }
        Ok(self.history.remove(track_id))
    }

    pub fn history_clear(&self) {
        self.history.clear();
    }

    pub fn listening_stats(&self, scope: &str, choice: Option<&str>) -> Result<StatsView> {
        self.history.stats(scope, choice)
    }

    pub fn reset_listening_stats(&self) {
        self.history.reset_stats();
    }

    /// Minutes east of UTC, reported by the window, for hours and weekdays.
    pub fn set_utc_offset(&self, minutes: i32) -> Result<()> {
        self.history.set_utc_offset(minutes)
    }

    /// Writes the history to disk now (the app is closing).
    pub fn flush_history(&self) {
        self.history.flush();
    }

    pub fn playlists(&self, offset: u32, count: u32) -> Result<Page<Playlist>> {
        self.api(|client, session| client.playlists(session, offset, count.clamp(1, 200)))
    }

    /// The signed-in user's friends.
    pub fn friends(&self, offset: u32, count: u32) -> Result<Page<Profile>> {
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.friends(session, offset, count.clamp(1, 200)))
    }

    /// A friend's tracks. `owner_id` must be a person (VK ids of people are
    /// positive), never a community and never "0".
    pub fn friend_tracks(&self, owner_id: i64, offset: u32, count: u32) -> Result<Page<Track>> {
        let owner_id = checked_person(owner_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.friend_tracks(session, owner_id, offset, count.clamp(1, 300)))
    }

    pub fn friend_playlists(&self, owner_id: i64, offset: u32, count: u32) -> Result<Page<Playlist>> {
        let owner_id = checked_person(owner_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.friend_playlists(session, owner_id, offset, count.clamp(1, 200)))
    }

    pub fn playlist_tracks(&self, playlist: &Playlist, offset: u32, count: u32) -> Result<Page<Track>> {
        self.api(|client, session| client.playlist_tracks(session, playlist, offset, count.clamp(1, 300)))
    }

    pub fn search_artists(&self, query: &str, offset: u32, count: u32) -> Result<Vec<Artist>> {
        let offset = checked_offset(offset)?;
        let Some(query) = checked_query(query)? else { return Ok(Vec::new()) };
        self.api(|client, session| client.search_artists(session, query, offset, count.clamp(1, 100)))
    }

    /// The artist meant by a name, for tracks that do not link one.
    pub fn find_artist(&self, name: &str) -> Result<Option<Artist>> {
        let Some(name) = checked_query(name)? else { return Ok(None) };
        self.api(|client, session| client.find_artist(session, name))
    }

    pub fn artist_tracks(&self, artist_id: &str, offset: u32, count: u32) -> Result<Page<Track>> {
        let artist_id = checked_artist_id(artist_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.artist_tracks(session, artist_id, offset, count.clamp(1, 100)))
    }

    pub fn artist_albums(&self, artist_id: &str, offset: u32, count: u32) -> Result<ArtistAlbums> {
        let artist_id = checked_artist_id(artist_id)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.artist_albums(session, artist_id, offset, count.clamp(1, 100)))
    }

    pub fn search_albums(&self, query: &str, offset: u32, count: u32) -> Result<Page<Album>> {
        let offset = checked_offset(offset)?;
        let Some(query) = checked_query(query)? else {
            return Ok(Page { items: Vec::new(), total: 0, next_offset: None });
        };
        self.api(|client, session| client.search_albums(session, query, offset, count.clamp(1, 100)))
    }

    pub fn followed_albums(&self, offset: u32, count: u32) -> Result<Page<Album>> {
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.followed_albums(session, offset, count.clamp(1, 100)))
    }

    pub fn album_tracks(&self, album: &Album, offset: u32, count: u32) -> Result<AlbumTracks> {
        checked_album(album)?;
        let offset = checked_offset(offset)?;
        self.api(|client, session| client.album_tracks(session, album, offset, count.clamp(1, 100)))
    }

    pub fn follow_album(&self, album: &Album, follow: bool) -> Result<()> {
        checked_album(album)?;
        self.api(|client, session| client.follow_album(session, album, follow))
    }
}
