//! Everything the window needs, in one object: sign-in state, VK data with
//! automatic token refresh, and the playback controller.

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::audio::Sink;
use crate::controller::{Controller, UiEvent};
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

pub struct App {
    controller: Controller,
    store: Box<dyn SecretStore>,
    emit: Arc<dyn Fn(UiEvent) + Send + Sync>,
    profile: Mutex<Option<Profile>>,
    persistent: Mutex<bool>,
    /// One refresh at a time: two screens hitting 401 together must not both
    /// re-issue the token.
    refresh_lock: Mutex<()>,
    token_url: String,
}

pub struct AppOptions {
    pub fetch: FetchConfig,
    pub token_url: String,
    pub volume: f32,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self { fetch: FetchConfig::new("PrivateMusicDesktop"), token_url: TOKEN_URL.to_string(), volume: 1.0 }
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
        let app = Self {
            controller,
            store,
            emit,
            profile: Mutex::new(None),
            persistent: Mutex::new(true),
            refresh_lock: Mutex::new(()),
            token_url: options.token_url,
        };
        match app.store.load() {
            Ok(Some(session)) => app.controller.set_session(Some(session)),
            Ok(None) => {}
            Err(error) => {
                log::warn!("secret store unavailable: {error}");
                *app.persistent.lock().unwrap_or_else(|p| p.into_inner()) = false;
            }
        }
        app
    }

    pub fn controller(&self) -> &Controller {
        &self.controller
    }

    fn lock<'a, T>(mutex: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
        mutex.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Adopts a fresh session (just signed in, or just refreshed).
    pub fn sign_in(&self, session: Session) {
        if let Err(error) = self.store.save(&session) {
            log::warn!("could not save the session: {error}");
            *Self::lock(&self.persistent) = false;
        } else {
            *Self::lock(&self.persistent) = true;
        }
        *Self::lock(&self.profile) = None;
        self.controller.set_session(Some(session));
    }

    pub fn sign_out(&self) {
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
        // Someone else may have refreshed while we waited for the lock.
        if let Some(current) = self.controller.session() {
            if current.access_token != stale.access_token {
                return Ok(current);
            }
        }
        match exchange_at(&self.token_url, &stale.refresh_cookie, &stale.web_user_agent, now_unix()) {
            Ok(fresh) => {
                self.sign_in(fresh.clone());
                Ok(fresh)
            }
            Err(Error::Unauthorized) => {
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

    pub fn playlists(&self, offset: u32, count: u32) -> Result<Page<Playlist>> {
        self.api(|client, session| client.playlists(session, offset, count.clamp(1, 200)))
    }

    pub fn playlist_tracks(&self, playlist: &Playlist, offset: u32, count: u32) -> Result<Page<Track>> {
        self.api(|client, session| client.playlist_tracks(session, playlist, offset, count.clamp(1, 300)))
    }
}
