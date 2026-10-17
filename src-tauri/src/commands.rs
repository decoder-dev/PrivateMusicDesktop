//! The window's entire view of the engine: thin, validated wrappers.

use pm_engine::app::{RadioSource, SessionState};
use pm_engine::controller::State;
use pm_engine::history::{HistoryEntry, StatsView};
use pm_engine::queue::Repeat;
use pm_engine::vk::{Album, AlbumTracks, Artist, ArtistAlbums, MusicMix, Page, Playlist, Profile, Track};
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

// Controller operations can briefly wait on engine locks. Keep them off the
// native window thread, including Pause/Next during recovery.
async fn control(state: TauriState<'_, AppState>, work: impl FnOnce(&pm_engine::controller::Controller) + Send + 'static) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { work(app.controller()); Ok(()) }).await
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
pub async fn session_state(state: TauriState<'_, AppState>) -> CommandResult<SessionState> {
    let app = state.app.clone();
    blocking(move || { Ok(app.session_state()) }).await
}

#[tauri::command]
pub async fn profile(state: TauriState<'_, AppState>) -> CommandResult<Profile> {
    let app = state.app.clone();
    blocking(move || app.profile()).await
}

#[tauri::command]
// Async on purpose: on Windows, creating a window from a synchronous command
// deadlocks WebView2 (the window opens but its page never loads).
pub async fn login_start(handle: AppHandle, state: TauriState<'_, AppState>) -> CommandResult<()> {
    crate::login::start(&handle, &state).map_err(|message| CommandError { kind: "login".into(), message })
}

#[tauri::command]
pub async fn logout(state: TauriState<'_, AppState>) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { app.sign_out(); Ok(()) }).await
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
pub async fn player_state(state: TauriState<'_, AppState>) -> CommandResult<State> {
    let app = state.app.clone();
    blocking(move || { Ok(app.controller().state()) }).await
}

#[tauri::command]
pub async fn play_tracks(state: TauriState<'_, AppState>, tracks: Vec<Track>, index: usize) -> CommandResult<()> {
    let tracks = checked_queue(tracks)?;
    control(state, move |controller| controller.play_tracks(tracks, index)).await
}

#[tauri::command]
pub async fn load_queue(state: TauriState<'_, AppState>, tracks: Vec<Track>, index: usize, position: f64) -> CommandResult<()> {
    let tracks = checked_queue(tracks)?;
    control(state, move |controller| controller.load_queue(tracks, index, position)).await
}

#[tauri::command]
pub async fn toggle(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, move |controller| controller.toggle()).await
}

#[tauri::command]
pub async fn pause(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, move |controller| controller.pause()).await
}

#[tauri::command]
pub async fn resume(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, move |controller| controller.resume()).await
}

#[tauri::command]
pub async fn next(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, move |controller| controller.next()).await
}

#[tauri::command]
pub async fn previous(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, move |controller| controller.previous()).await
}

#[tauri::command]
pub async fn seek(state: TauriState<'_, AppState>, seconds: f64) -> CommandResult<()> {
    control(state, move |controller| controller.seek(seconds)).await
}

#[tauri::command]
pub async fn set_volume(state: TauriState<'_, AppState>, volume: f32) -> CommandResult<()> {
    control(state, move |controller| controller.set_volume(volume)).await
}

#[tauri::command]
pub async fn set_shuffle(state: TauriState<'_, AppState>, on: bool) -> CommandResult<()> {
    control(state, move |controller| controller.set_shuffle(on)).await
}

#[tauri::command]
pub async fn set_repeat(state: TauriState<'_, AppState>, repeat: Repeat) -> CommandResult<()> {
    control(state, move |controller| controller.set_repeat(repeat)).await
}

#[tauri::command]
pub async fn jump(state: TauriState<'_, AppState>, index: usize) -> CommandResult<()> {
    control(state, move |controller| controller.jump(index)).await
}

#[tauri::command]
pub async fn play_next(state: TauriState<'_, AppState>, track: Track) -> CommandResult<()> {
    control(state, move |controller| controller.play_next(track)).await
}

#[tauri::command]
pub async fn play_last(state: TauriState<'_, AppState>, track: Track) -> CommandResult<()> {
    control(state, move |controller| controller.play_last(track)).await
}

#[tauri::command]
pub async fn remove_from_queue(state: TauriState<'_, AppState>, index: usize) -> CommandResult<()> {
    control(state, move |controller| controller.remove_from_queue(index)).await
}

#[tauri::command]
pub async fn stop(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, move |controller| controller.stop()).await
}

#[tauri::command]
pub async fn set_visualizer(state: TauriState<'_, AppState>, on: bool) -> CommandResult<()> {
    control(state, move |controller| controller.set_visualizer(on)).await
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

/// The window gained or lost the user's attention: the engine watches the
/// network (cheaply) only while it has it or while audio plays.
#[tauri::command]
pub fn network_focus(state: TauriState<'_, AppState>, focused: bool) {
    state.app.set_window_focused(focused);
}

/// "Retry" after a connection failure: drop pooled connections and look at
/// the network now. The window then repeats the failed load itself.
#[tauri::command]
pub fn network_check(state: TauriState<'_, AppState>) {
    state.app.check_network();
}

/// "Retry" for playback that stopped on a connection problem.
#[tauri::command]
pub async fn retry_playback(state: TauriState<'_, AppState>) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { app.retry_playback(); Ok(()) }).await
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
pub async fn set_output_device(state: TauriState<'_, AppState>, name: Option<String>) -> CommandResult<()> {
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
    control(state, |controller| controller.reopen_output()).await
}

#[tauri::command]
pub async fn mix(state: TauriState<'_, AppState>, options: Option<String>) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.mix_with_options(options.as_deref())).await
}

#[tauri::command]
pub async fn mix_settings(state: TauriState<'_, AppState>) -> CommandResult<serde_json::Value> {
    let app = state.app.clone();
    blocking(move || app.mix_settings()).await
}

#[tauri::command]
pub async fn catalog_mixes(state: TauriState<'_, AppState>) -> CommandResult<Vec<MusicMix>> {
    let app = state.app.clone();
    blocking(move || app.catalog_mixes()).await
}

#[tauri::command]
pub async fn stream_mix(state: TauriState<'_, AppState>, mix_id: String, options: Option<String>) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.stream_mix(&mix_id, options.as_deref())).await
}

#[tauri::command]
pub async fn new_releases(state: TauriState<'_, AppState>) -> CommandResult<Vec<Album>> {
    let app = state.app.clone();
    blocking(move || app.new_releases()).await
}

/// Plays a queue that keeps refilling from the mix, stream or seed it came from.
#[tauri::command]
pub async fn play_radio(state: TauriState<'_, AppState>, source: RadioSource, tracks: Vec<Track>, index: usize) -> CommandResult<()> {
    let tracks = checked_queue(tracks)?;
    let app = state.app.clone();
    blocking(move || app.play_radio(source, tracks, index)).await
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

/// Equaliser gains in dB for the ten bands; `None` turns it off.
#[tauri::command]
pub fn set_equalizer(state: TauriState<'_, AppState>, gains: Option<[f32; 10]>) {
    state.app.controller().set_equalizer(gains);
}

#[derive(Serialize)]
pub struct Downloads {
    tracks: Vec<Track>,
    bytes: u64,
}

#[tauri::command]
pub async fn downloads(state: TauriState<'_, AppState>) -> CommandResult<Downloads> {
    let app = state.app.clone();
    blocking(move || Ok(Downloads { tracks: app.downloads().into_iter().map(|d| d.track).collect(), bytes: app.downloads_size() })).await
}

#[tauri::command]
pub async fn download(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Track> {
    let app = state.app.clone();
    blocking(move || app.download(&track).map(|d| d.track)).await
}

/// `track_id` is "owner_trackid", as the window knows it.
#[tauri::command]
pub async fn remove_download(state: TauriState<'_, AppState>, track_id: String) -> CommandResult<bool> {
    if track_id.len() > 64 || !track_id.chars().all(|c| c.is_ascii_digit() || c == '_' || c == '-') { return Err(Error::InvalidRequest.into()); }
    let app = state.app.clone();
    blocking(move || { app.remove_download(&track_id) }).await
}

#[tauri::command]
pub async fn create_playlist(state: TauriState<'_, AppState>, title: String, description: String) -> CommandResult<Playlist> {
    let app = state.app.clone();
    blocking(move || app.create_playlist(&title, &description)).await
}

#[tauri::command]
pub async fn edit_playlist(
    state: TauriState<'_, AppState>,
    playlist: Playlist,
    title: String,
    description: String,
) -> CommandResult<Playlist> {
    let app = state.app.clone();
    blocking(move || app.edit_playlist(&playlist, &title, &description)).await
}

#[tauri::command]
pub async fn delete_playlist(state: TauriState<'_, AppState>, playlist: Playlist) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.delete_playlist(&playlist)).await
}

#[tauri::command]
pub async fn add_to_playlist(state: TauriState<'_, AppState>, playlist: Playlist, track: Track) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.add_to_playlist(&playlist, &track)).await
}

#[tauri::command]
pub async fn remove_from_playlist(state: TauriState<'_, AppState>, playlist: Playlist, track: Track) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.remove_from_playlist(&playlist, &track)).await
}

// Artist and album pages. Ids, offsets and the album locator are checked by
// the engine before anything reaches VK.

#[tauri::command]
pub async fn search_artists(state: TauriState<'_, AppState>, query: String, offset: u32, count: u32) -> CommandResult<Vec<Artist>> {
    let app = state.app.clone();
    blocking(move || app.search_artists(&query, offset, count)).await
}

#[tauri::command]
pub async fn find_artist(state: TauriState<'_, AppState>, name: String) -> CommandResult<Option<Artist>> {
    let app = state.app.clone();
    blocking(move || app.find_artist(&name)).await
}

#[tauri::command]
pub async fn artist_tracks(
    state: TauriState<'_, AppState>,
    artist_id: String,
    offset: u32,
    count: u32,
) -> CommandResult<Page<Track>> {
    let app = state.app.clone();
    blocking(move || app.artist_tracks(&artist_id, offset, count)).await
}

#[tauri::command]
pub async fn artist_albums(
    state: TauriState<'_, AppState>,
    artist_id: String,
    offset: u32,
    count: u32,
) -> CommandResult<ArtistAlbums> {
    let app = state.app.clone();
    blocking(move || app.artist_albums(&artist_id, offset, count)).await
}

#[tauri::command]
pub async fn search_albums(state: TauriState<'_, AppState>, query: String, offset: u32, count: u32) -> CommandResult<Page<Album>> {
    let app = state.app.clone();
    blocking(move || app.search_albums(&query, offset, count)).await
}

#[tauri::command]
pub async fn followed_albums(state: TauriState<'_, AppState>, offset: u32, count: u32) -> CommandResult<Page<Album>> {
    let app = state.app.clone();
    blocking(move || app.followed_albums(offset, count)).await
}

#[tauri::command]
pub async fn album_tracks(state: TauriState<'_, AppState>, album: Album, offset: u32, count: u32) -> CommandResult<AlbumTracks> {
    let app = state.app.clone();
    blocking(move || app.album_tracks(&album, offset, count)).await
}

#[tauri::command]
pub async fn follow_album(state: TauriState<'_, AppState>, album: Album, follow: bool) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.follow_album(&album, follow)).await
}

// Friends' music.

#[tauri::command]
pub async fn friends(state: TauriState<'_, AppState>, offset: u32, count: u32) -> CommandResult<Page<Profile>> {
    let app = state.app.clone();
    blocking(move || app.friends(offset, count)).await
}

#[tauri::command]
pub async fn friend_tracks(state: TauriState<'_, AppState>, owner_id: i64, offset: u32, count: u32) -> CommandResult<Page<Track>> {
    let app = state.app.clone();
    blocking(move || app.friend_tracks(owner_id, offset, count)).await
}

#[tauri::command]
pub async fn friend_playlists(state: TauriState<'_, AppState>, owner_id: i64, offset: u32, count: u32) -> CommandResult<Page<Playlist>> {
    let app = state.app.clone();
    blocking(move || app.friend_playlists(owner_id, offset, count)).await
}

// Listening history and statistics: local to this computer.

#[tauri::command]
pub async fn history(state: TauriState<'_, AppState>) -> CommandResult<Vec<HistoryEntry>> {
    let app = state.app.clone();
    blocking(move || { Ok(app.history()) }).await
}

#[tauri::command]
pub async fn history_remove(state: TauriState<'_, AppState>, track_id: String) -> CommandResult<bool> {
    let app = state.app.clone();
    blocking(move || { app.history_remove(&track_id) }).await
}

#[tauri::command]
pub async fn history_clear(state: TauriState<'_, AppState>) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { app.history_clear(); Ok(()) }).await
}

/// `scope` is "month", "year" or "all"; `choice` is "YYYY-MM" or "YYYY".
#[tauri::command]
pub async fn listening_stats(state: TauriState<'_, AppState>, scope: String, choice: Option<String>) -> CommandResult<StatsView> {
    let app = state.app.clone();
    blocking(move || { app.listening_stats(&scope, choice.as_deref()) }).await
}

#[tauri::command]
pub async fn reset_listening_stats(state: TauriState<'_, AppState>) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { app.reset_listening_stats(); Ok(()) }).await
}

/// The window's UTC offset in minutes east, so hours follow the user's clock.
#[tauri::command]
pub async fn set_utc_offset(state: TauriState<'_, AppState>, minutes: i32) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { app.set_utc_offset(minutes) }).await
}

#[tauri::command]
pub async fn clear_upcoming(state: TauriState<'_, AppState>) -> CommandResult<()> {
    control(state, |controller| controller.clear_upcoming()).await
}

#[tauri::command]
pub async fn move_upcoming(state: TauriState<'_, AppState>, from: usize, to: usize) -> CommandResult<()> {
    control(state, move |controller| controller.move_upcoming(from, to)).await
}

#[tauri::command]
pub async fn set_sleep_timer(state: TauriState<'_, AppState>, seconds: Option<u64>, end_of_track: bool) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.controller().set_sleep_timer(seconds, end_of_track)).await
}

#[tauri::command]
pub async fn sleep_timer(state: TauriState<'_, AppState>) -> CommandResult<(Option<u64>, bool)> {
    let app = state.app.clone();
    blocking(move || Ok(app.controller().sleep_timer())).await
}

#[tauri::command]
pub async fn export_track(handle: AppHandle, state: TauriState<'_, AppState>, track: Track) -> CommandResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let app = state.app.clone();
    blocking(move || {
        let entry = app.download(&track)?;
        let source = app.controller().fetch_config().offline_dir.ok_or(Error::InvalidRequest)?.join(&entry.file);
        let extension = std::path::Path::new(&entry.file).extension().and_then(|e| e.to_str()).ok_or(Error::InvalidRequest)?;
        let title: String = format!("{} - {}", track.artist, track.title).chars()
            .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).take(160).collect();
        let Some(target) = handle.dialog().file().set_file_name(format!("{title}.{extension}"))
            .add_filter("Audio", &[extension]).blocking_save_file() else { return Ok(false) };
        let target = target.into_path().map_err(|_| Error::InvalidRequest)?;
        // The destination comes only from the native save dialog.
        if target != source {
            std::fs::copy(source, target).map_err(|e| Error::Stream(format!("saving audio: {e}")))?;
        }
        Ok(true)
    }).await
}

#[tauri::command]
pub async fn playlist_by_id(state: TauriState<'_, AppState>, album: Album) -> CommandResult<Album> {
    let app = state.app.clone();
    blocking(move || app.playlist_by_id(&album)).await
}

/// Fixed normal preset, desktop maximization, or borderless fullscreen.
#[tauri::command]
pub fn set_window_mode(window: tauri::WebviewWindow, mode: String) -> CommandResult<()> {
    let apply = || -> tauri::Result<()> {
        window.set_fullscreen(false)?;
        if mode == "fullscreen" {
            return window.set_fullscreen(true);
        }
        if mode == "maximized" {
            return window.maximize();
        }
        window.unmaximize()?;
        let (mut width, mut height) = (1120.0f64, 740.0f64);
        if let Some(monitor) = window.current_monitor()? {
            width = width.min(f64::from(monitor.size().width) / monitor.scale_factor() * 0.85);
            height = height.min(f64::from(monitor.size().height) / monitor.scale_factor() * 0.85);
        }
        window.set_size(tauri::LogicalSize::new(width, height))?;
        window.center()
    };
    if !matches!(mode.as_str(), "medium" | "maximized" | "fullscreen") {
        return Err(CommandError { kind: "window".into(), message: "unknown window mode".into() });
    }
    apply().map_err(|e| CommandError { kind: "window".into(), message: e.to_string() })
}
