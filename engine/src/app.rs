//! Everything the window needs, in one object: sign-in state, VK data with
//! automatic token refresh, and the playback controller.

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::audio::Sink;
use crate::controller::{Controller, UiEvent};
use crate::lyrics::{from_plain, LrcLib, Lyrics};
use crate::store::SecretStore;
use crate::stream::FetchConfig;
use crate::vk::auth::{exchange_at, now_unix};
use crate::vk::{Page, Playlist, Profile, Session, Track, VkClient};
use crate::{Error, Result};

const TOKEN_URL: &str = "https://login.vk.ru/?act=web_token";

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
}

pub struct AppOptions {
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
        };
        match app.store.load() {
            Ok(Some(session)) => app.controller.set_session(Some(session)),
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
        self.controller.set_session(Some(session));
    }

    pub fn sign_out(&self) {
        let mut generation = Self::lock(&self.generation);
        *generation += 1;
        if let Err(error) = self.store.clear() {
            log::warn!("could not clear the session: {error}");
        }
        *Self::lock(&self.profile) = None;
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

    /// The personal mix: two pages, so a session lasts a while.
    pub fn mix(&self) -> Result<Vec<Track>> {
        self.api(|client, session| {
            let mut tracks = client.mix(session, 0, 50)?;
            if let Ok(more) = client.mix(session, 50, 50) {
                let known: std::collections::HashSet<String> = tracks.iter().map(Track::id).collect();
                tracks.extend(more.into_iter().filter(|t| !known.contains(&t.id())));
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

    pub fn add_to_library(&self, track: &Track) -> Result<Track> {
        self.api(|client, session| client.add_track(session, track))
    }

    pub fn remove_from_library(&self, track: &Track) -> Result<()> {
        self.api(|client, session| client.delete_track(session, track))
    }

    /// Lyrics for a track: timed from LRCLIB when its length agrees, else VK's
    /// own text, else whatever plain text LRCLIB had. `Ok(None)`: none exists.
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
        match (lrc, failure) {
            (Some(found), _) => Ok(Some(found)),
            (None, Some(error)) => Err(error),
            (None, None) => Ok(None),
        }
    }

    pub fn playlists(&self, offset: u32, count: u32) -> Result<Page<Playlist>> {
        self.api(|client, session| client.playlists(session, offset, count.clamp(1, 200)))
    }

    pub fn playlist_tracks(&self, playlist: &Playlist, offset: u32, count: u32) -> Result<Page<Track>> {
        self.api(|client, session| client.playlist_tracks(session, playlist, offset, count.clamp(1, 300)))
    }
}
