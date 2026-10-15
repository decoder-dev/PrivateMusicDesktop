//! The window's entire view of the engine: thin, validated wrappers.

use pm_engine::app::SessionState;
use pm_engine::controller::State;
use pm_engine::queue::Repeat;
use pm_engine::vk::{Page, Playlist, Profile, Track};
use pm_engine::Error;
use serde::Serialize;
use tauri::{AppHandle, State as TauriState};

use crate::AppState;

#[derive(Serialize)]
pub struct CommandError {
    kind: String,
    message: String,
}

impl From<Error> for CommandError {
    fn from(error: Error) -> Self {
        Self { kind: error.kind().to_string(), message: error.to_string() }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Network calls block, so they run off the UI/IPC thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, Error> + Send + 'static) -> CommandResult<T> {
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(result) => result.map_err(CommandError::from),
        Err(_) => Err(CommandError { kind: "internal".into(), message: "the task was interrupted".into() }),
    }
}

/// Upper bound for a queue handed over by the window; a runaway caller cannot
/// make the engine hold millions of tracks.
const MAX_QUEUE: usize = 20_000;

fn checked_queue(tracks: Vec<Track>) -> CommandResult<Vec<Track>> {
    if tracks.len() > MAX_QUEUE {
        return Err(Error::InvalidRequest.into());
    }
    Ok(tracks)
}

#[tauri::command]
pub fn session_state(state: TauriState<'_, AppState>) -> SessionState {
    state.app.session_state()
}

#[tauri::command]
pub async fn profile(state: TauriState<'_, AppState>) -> CommandResult<Profile> {
    let app = state.app.clone();
    blocking(move || app.profile()).await
}

#[tauri::command]
pub fn login_start(handle: AppHandle, state: TauriState<'_, AppState>) -> CommandResult<()> {
    crate::login::start(&handle, &state).map_err(|message| CommandError { kind: "login".into(), message })
}

#[tauri::command]
pub fn logout(state: TauriState<'_, AppState>) {
    state.app.sign_out();
}

#[tauri::command]
pub async fn library(state: TauriState<'_, AppState>, offset: u32, count: u32) -> CommandResult<Page<Track>> {
    let app = state.app.clone();
    blocking(move || app.library(offset, count)).await
}

#[tauri::command]
pub async fn playlists(state: TauriState<'_, AppState>, offset: u32, count: u32) -> CommandResult<Page<Playlist>> {
    let app = state.app.clone();
    blocking(move || app.playlists(offset, count)).await
}

#[tauri::command]
pub async fn playlist_tracks(
    state: TauriState<'_, AppState>,
    playlist: Playlist,
    offset: u32,
    count: u32,
) -> CommandResult<Page<Track>> {
    let app = state.app.clone();
    blocking(move || app.playlist_tracks(&playlist, offset, count)).await
}

#[tauri::command]
pub async fn search(state: TauriState<'_, AppState>, query: String, offset: u32, count: u32) -> CommandResult<Page<Track>> {
    if query.len() > 500 {
        return Err(Error::InvalidRequest.into());
    }
    let app = state.app.clone();
    blocking(move || app.search(&query, offset, count)).await
}

#[tauri::command]
pub fn player_state(state: TauriState<'_, AppState>) -> State {
    state.app.controller().state()
}

#[tauri::command]
pub fn play_tracks(state: TauriState<'_, AppState>, tracks: Vec<Track>, index: usize) -> CommandResult<()> {
    state.app.controller().play_tracks(checked_queue(tracks)?, index);
    Ok(())
}

#[tauri::command]
pub fn load_queue(state: TauriState<'_, AppState>, tracks: Vec<Track>, index: usize, position: f64) -> CommandResult<()> {
    state.app.controller().load_queue(checked_queue(tracks)?, index, position);
    Ok(())
}

#[tauri::command]
pub fn toggle(state: TauriState<'_, AppState>) {
    state.app.controller().toggle();
}

#[tauri::command]
pub fn pause(state: TauriState<'_, AppState>) {
    state.app.controller().pause();
}

#[tauri::command]
pub fn resume(state: TauriState<'_, AppState>) {
    state.app.controller().resume();
}

#[tauri::command]
pub fn next(state: TauriState<'_, AppState>) {
    state.app.controller().next();
}

#[tauri::command]
pub fn previous(state: TauriState<'_, AppState>) {
    state.app.controller().previous();
}

#[tauri::command]
pub fn seek(state: TauriState<'_, AppState>, seconds: f64) {
    state.app.controller().seek(seconds);
}

#[tauri::command]
pub fn set_volume(state: TauriState<'_, AppState>, volume: f32) {
    state.app.controller().set_volume(volume);
}

#[tauri::command]
pub fn set_shuffle(state: TauriState<'_, AppState>, on: bool) {
    state.app.controller().set_shuffle(on);
}

#[tauri::command]
pub fn set_repeat(state: TauriState<'_, AppState>, repeat: Repeat) {
    state.app.controller().set_repeat(repeat);
}

#[tauri::command]
pub fn jump(state: TauriState<'_, AppState>, index: usize) {
    state.app.controller().jump(index);
}

#[tauri::command]
pub fn play_next(state: TauriState<'_, AppState>, track: Track) {
    state.app.controller().play_next(track);
}

#[tauri::command]
pub fn play_last(state: TauriState<'_, AppState>, track: Track) {
    state.app.controller().play_last(track);
}

#[tauri::command]
pub fn remove_from_queue(state: TauriState<'_, AppState>, index: usize) {
    state.app.controller().remove_from_queue(index);
}

#[tauri::command]
pub fn stop(state: TauriState<'_, AppState>) {
    state.app.controller().stop();
}

#[tauri::command]
pub fn set_visualizer(state: TauriState<'_, AppState>, on: bool) {
    state.app.controller().set_visualizer(on);
}

#[tauri::command]
pub async fn artwork_palette(url: String) -> CommandResult<pm_engine::artwork::Palette> {
    if url.len() > 2048 {
        return Err(Error::InvalidRequest.into());
    }
    blocking(move || pm_engine::artwork::palette_from_url(&url)).await
}

/// Full-screen for the whole window (the UI's own "full screen" view is separate).
#[tauri::command]
pub fn set_fullscreen(window: tauri::WebviewWindow, on: bool) -> CommandResult<()> {
    window.set_fullscreen(on).map_err(|e| CommandError { kind: "window".into(), message: e.to_string() })
}

#[tauri::command]
pub async fn lyrics(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Option<pm_engine::lyrics::Lyrics>> {
    let app = state.app.clone();
    blocking(move || app.lyrics(&track)).await
}

#[tauri::command]
pub async fn add_to_library(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Track> {
    let app = state.app.clone();
    blocking(move || app.add_to_library(&track)).await
}

#[tauri::command]
pub async fn remove_from_library(state: TauriState<'_, AppState>, track: Track) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.remove_from_library(&track)).await
}

#[tauri::command]
pub fn set_close_to_tray(state: TauriState<'_, AppState>, on: bool) {
    state.close_to_tray.store(on, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    /// False when the desktop has no tray: "close to tray" cannot work there.
    has_tray: bool,
}

#[tauri::command]
pub fn app_info(handle: AppHandle, state: TauriState<'_, AppState>) -> AppInfo {
    AppInfo {
        version: handle.package_info().version.to_string(),
        has_tray: state.has_tray.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Sound outputs, default first. Asking the audio system can take a moment.
#[tauri::command]
pub async fn output_devices() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(pm_engine::audio::output_devices).await.unwrap_or_default()
}

/// `None` follows the system default. Playback continues on the new output.
#[tauri::command]
pub fn set_output_device(state: TauriState<'_, AppState>, name: Option<String>) -> CommandResult<()> {
    if name.as_ref().is_some_and(|n| n.is_empty() || n.len() > 512) {
        return Err(Error::InvalidRequest.into());
    }
    {
        let mut current = state.output_device.lock().unwrap_or_else(|p| p.into_inner());
        if *current == name {
            return Ok(());
        }
        *current = name;
    }
    state.app.controller().reopen_output();
    Ok(())
}

#[tauri::command]
pub async fn mix(state: TauriState<'_, AppState>) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.mix()).await
}

#[tauri::command]
pub async fn recommendations(state: TauriState<'_, AppState>) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.recommendations()).await
}

#[tauri::command]
pub async fn similar(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.similar(&track)).await
}
