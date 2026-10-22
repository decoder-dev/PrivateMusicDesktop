pub mod auth;
pub mod client;
pub mod model;

pub use auth::{exchange_web_session, session_from_web_token, API_USER_AGENT};
pub use client::{AlbumTracks, ArtistAlbums, Session, VkClient};
pub use model::{Album, AlbumRef, Artist, ArtistRef, MusicMix, Page, Playlist, Profile, Track};
