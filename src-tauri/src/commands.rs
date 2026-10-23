//! Весь вид движка из окна: тонкие, проверенные обертки.

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
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after: Option<u64>,
}

impl From<Error> for CommandError {
    fn from(error: Error) -> Self {
        let retry_after = match error { Error::RetryAfter { seconds, .. } => Some(seconds), _ => None };
        Self { kind: error.kind().to_string(), message: error.to_string(), retry_after }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Сетевые вызовы блокируются, поэтому они выполняются из потока UI/IPC.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, Error> + Send + 'static) -> CommandResult<T> {
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(result) => result.map_err(CommandError::from),
        Err(_) => Err(CommandError { kind: "internal".into(), message: "the task was interrupted".into(), retry_after:None }),
    }
}

// Операции контроллера могут ненадолго подождать блокировки двигателя. Держите их подальше от
// собственного потока окна, включая Pause/Next во время восстановления.
async fn control(state: TauriState<'_, AppState>, work: impl FnOnce(&pm_engine::controller::Controller) + Send + 'static) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { work(app.controller()); Ok(()) }).await
}

/// Верхняя граница очереди, переданной окном; неуправляемый абонент не может заставить движок хранить
/// миллионы треков.
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
// Специальная асинхронность: в Windows создание окна из синхронной команды приводит к взаимоблокировке
// WebView2 (окно открывается, но его страница никогда не загружается).
pub async fn login_start(handle: AppHandle, state: TauriState<'_, AppState>, reset: Option<bool>) -> CommandResult<()> {
    let reset = reset.unwrap_or(false);
    #[cfg(desktop)]
    let started = crate::login::start(&handle, &state, reset);
    #[cfg(mobile)]
    let started = crate::mobile_login::start(&handle, &state, reset);
    started.map_err(|message| CommandError { kind: "login".into(), message, retry_after:None })
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

/// Полноэкранный режим для всего окна (собственный вид "full screen" у UI отдельный).
#[cfg(desktop)]
#[tauri::command]
pub fn set_fullscreen(window: tauri::WebviewWindow, on: bool) -> CommandResult<()> {
    set_window_mode(window,if on{"fullscreen"}else{"medium"}.into())
}

/// Mobile: the app is always full screen already; the commands exist so the
/// UI never meets "command not found".
#[cfg(mobile)]
#[tauri::command]
pub fn set_fullscreen(on: bool) -> CommandResult<()> {
    let _ = on;
    Ok(())
}

#[tauri::command]
pub async fn lyrics(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Option<pm_engine::lyrics::Lyrics>> {
    let app = state.app.clone();
    blocking(move || app.lyrics(&track)).await
}

#[tauri::command]
pub async fn lyrics_versions(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Vec<pm_engine::lyrics::Lyrics>> {
    let app = state.app.clone();
    blocking(move || app.lyrics_versions(&track)).await
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

/// Окно привлекало или теряло внимание пользователя: движок наблюдает за сетью (дешево) только пока она
/// у нее есть или пока воспроизводится звук.
#[tauri::command]
pub fn network_focus(state: TauriState<'_, AppState>, focused: bool) {
    state.app.set_window_focused(focused);
}

/// «Повторить» после сбоя соединения: удалите соединения из пула и посмотрите на сеть сейчас. Затем окно
/// повторяет саму неудачную загрузку.
#[tauri::command]
pub fn network_check(state: TauriState<'_, AppState>) {
    state.app.check_network();
}

#[tauri::command]
pub async fn network_probe(state: TauriState<'_, AppState>) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || app.probe_network()).await
}

/// «Повторить» для воспроизведения, остановленного из-за проблемы с подключением.
#[tauri::command]
pub async fn retry_playback(state: TauriState<'_, AppState>) -> CommandResult<()> {
    let app = state.app.clone();
    blocking(move || { app.retry_playback(); Ok(()) }).await
}

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    /// Ложь, если на рабочем столе нет лотка: "close to tray" не может там работать.
    has_tray: bool,
    /// `std::env::consts::OS`: "windows", "macos", "linux", "android" …
    platform: &'static str,
}

#[tauri::command]
pub fn app_info(handle: AppHandle, state: TauriState<'_, AppState>) -> AppInfo {
    AppInfo {
        version: handle.package_info().version.to_string(),
        has_tray: state.has_tray.load(std::sync::atomic::Ordering::Relaxed),
        platform: std::env::consts::OS,
    }
}

/// CI на эмуляторе Android: цикл «записать, прочитать, удалить» через Keystore. Существует только в отладочной
/// сборке Android (в остальных возвращает ошибку `unsupported`); сохранённую сессию не трогает.
#[tauri::command]
pub async fn debug_secure_selftest(handle: AppHandle, mode: Option<String>) -> CommandResult<String> {
    #[cfg(all(target_os = "android", debug_assertions))]
    {
        blocking(move || crate::secure_store::selftest(&handle, mode.as_deref().unwrap_or("")).map(str::to_string)).await
    }
    #[cfg(not(all(target_os = "android", debug_assertions)))]
    {
        let _ = (handle, mode);
        Err(CommandError { kind: "unsupported".into(), message: "available in Android debug builds only".into(), retry_after: None })
    }
}

/// Звуковые выходы, по умолчанию в первую очередь. Запрос аудиосистемы может занять некоторое время.
#[tauri::command]
pub async fn output_devices() -> Vec<String> {
    #[cfg(desktop)]
    {
        tauri::async_runtime::spawn_blocking(pm_engine::audio::output_devices).await.unwrap_or_default()
    }
    // Mobile: the system routes audio (speaker, headphones, Bluetooth) itself.
    #[cfg(mobile)]
    {
        Vec::new()
    }
}

/// `None` соответствует системному значению по умолчанию. Воспроизведение продолжится на новом выходе.
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
pub async fn mix(state: TauriState<'_, AppState>, options: Option<String>, offset: Option<u32>) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.stream_mix_at("common", options.as_deref(), offset.unwrap_or(0))).await
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
pub async fn stream_mix(state: TauriState<'_, AppState>, mix_id: String, options: Option<String>, offset: Option<u32>) -> CommandResult<Vec<Track>> {
    let app = state.app.clone();
    blocking(move || app.stream_mix_at(&mix_id, options.as_deref(), offset.unwrap_or(0))).await
}

#[tauri::command]
pub async fn new_releases(state: TauriState<'_, AppState>) -> CommandResult<Vec<Album>> {
    let app = state.app.clone();
    blocking(move || app.new_releases()).await
}

/// Воспроизводит очередь, которая продолжает пополняться из микса, потока или начального числа, из
/// которого она пришла.
#[tauri::command]
pub async fn play_radio(state: TauriState<'_, AppState>, source: RadioSource, tracks: Vec<Track>, index: usize, next_offset: Option<u32>) -> CommandResult<()> {
    let tracks = checked_queue(tracks)?;
    let app = state.app.clone();
    blocking(move || app.play_radio_from(source, tracks, index, next_offset)).await
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

/// Усиление эквалайзера dB для десяти полос; `None` выключает его.
#[tauri::command]
pub fn set_equalizer(state: TauriState<'_, AppState>, gains: Option<[f32; 10]>) {
    state.app.controller().set_equalizer(gains);
}

#[tauri::command]
pub fn set_taste(state:TauriState<'_,AppState>,taste:pm_engine::taste::Taste,user_id:i64)->CommandResult<()> {
    if state.app.controller().session().and_then(|session|session.user_id)!=Some(user_id){return Err(CommandError::from(pm_engine::Error::Unauthorized));}
    state.app.controller().set_taste(taste).map_err(CommandError::from)
}

#[tauri::command]
pub fn set_transitions(state:TauriState<'_,AppState>,prefetch:bool,gapless:bool,crossfade:f64)->CommandResult<()> {
    if !crossfade.is_finite() || !(0.0..=8.0).contains(&crossfade){return Err(CommandError::from(pm_engine::Error::InvalidRequest));}
    state.app.controller().set_transitions(prefetch,gapless,crossfade);Ok(())
}

#[tauri::command]
pub fn set_normalization(state:TauriState<'_,AppState>,on:bool){state.app.controller().set_normalization(on);}

#[tauri::command]
pub fn month_plays(state:TauriState<'_,AppState>)->std::collections::HashMap<String,u32>{state.app.month_plays()}

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
pub async fn download_details(state: TauriState<'_, AppState>, track_id: String) -> CommandResult<Option<pm_engine::offline::DownloadDetails>> {
    if track_id.len() > 64 || !track_id.chars().all(|c| c.is_ascii_digit() || c == '_' || c == '-') { return Err(Error::InvalidRequest.into()); }
    let app = state.app.clone();
    blocking(move || Ok(app.download_details(&track_id))).await
}

#[tauri::command]
pub async fn download(state: TauriState<'_, AppState>, track: Track) -> CommandResult<Track> {
    let app = state.app.clone();
    blocking(move || app.download(&track).map(|d| d.track)).await
}

/// `track_id` — это "owner_trackid", как его знает окно.
#[tauri::command]
pub async fn remove_download(state: TauriState<'_, AppState>, track_id: String) -> CommandResult<bool> {
    if track_id.len() > 64 || !track_id.chars().all(|c| c.is_ascii_digit() || c == '_' || c == '-') { return Err(Error::InvalidRequest.into()); }
    let app = state.app.clone();
    blocking(move || { app.remove_download(&track_id) }).await
}

#[tauri::command]
pub async fn create_playlist(state: TauriState<'_, AppState>, title: String, description: String, hidden: Option<bool>) -> CommandResult<Playlist> {
    let app = state.app.clone();
    blocking(move || app.create_playlist_with_discovery(&title, &description, hidden)).await
}

#[tauri::command]
pub async fn edit_playlist(
    state: TauriState<'_, AppState>,
    playlist: Playlist,
    title: String,
    description: String,
    hidden: Option<bool>,
) -> CommandResult<Playlist> {
    let app = state.app.clone();
    blocking(move || app.edit_playlist_with_discovery(&playlist, &title, &description, hidden)).await
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

// Страницы исполнителя и альбома. Идентификаторы, смещения и локатор альбома проверяются движком,
// прежде чем что-либо достигнет VK.

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

// Музыка друзей.

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

// История прослушивания и статистика: локально для этого компьютера.

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

/// Смещение UTC окна в минутах на восток, поэтому часы следуют за часами пользователя.
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

#[cfg(desktop)]
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
        // Пункт назначения указывается только в собственном диалоговом окне сохранения.
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

/// Исправлены обычные настройки, максимизация рабочего стола или полноэкранный режим без полей.
#[cfg(desktop)]
#[tauri::command]
pub fn set_window_mode(window: tauri::WebviewWindow, mode: String) -> CommandResult<()> {
    let apply = || -> tauri::Result<()> {
        window.set_fullscreen(false)?;
        window.set_always_on_top(mode == "mini")?;
        window.set_min_size(Some(tauri::LogicalSize::new(if mode == "mini" { 420.0 } else { 780.0 },if mode == "mini" { 190.0 } else { 540.0 })))?;
        if mode == "mini" {
            window.unmaximize()?;
            window.set_size(tauri::LogicalSize::new(420.0,190.0))?;
            return window.center();
        }
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
    if !matches!(mode.as_str(), "medium" | "maximized" | "fullscreen" | "mini") {
        return Err(CommandError { kind: "window".into(), message: "unknown window mode".into(), retry_after:None });
    }
    apply().map_err(|e| CommandError { kind: "window".into(), message: e.to_string(), retry_after:None })
}

/// Mobile: режимов окна нет (приложение всегда на весь экран); команда существует, чтобы UI не
/// встречал «command not found».
#[cfg(mobile)]
#[tauri::command]
pub fn set_window_mode(mode: String) -> CommandResult<()> {
    let _ = mode;
    Ok(())
}

/// Mobile: диалога сохранения файла в этой сборке нет; команда возвращает понятную ошибку.
#[cfg(mobile)]
#[tauri::command]
pub async fn export_track(track: Track) -> CommandResult<bool> {
    let _ = track;
    Err(CommandError { kind: "unsupported".into(), message: "Export is not available on this device".into(), retry_after: None })
}


#[tauri::command]
pub async fn choose_audio(handle: AppHandle, state: TauriState<'_, AppState>) -> CommandResult<Option<crate::audio_upload::AudioFileInfo>> {
    let app = state.app.clone();
    blocking(move || crate::audio_upload::choose(&handle, &app)).await
}

#[tauri::command]
pub async fn upload_audio(handle: AppHandle, state: TauriState<'_, AppState>, file_id: String, artist: String, title: String) -> CommandResult<Track> {
    let app = state.app.clone();
    blocking(move || crate::audio_upload::upload(&handle, &app, &file_id, &artist, &title)).await
}

#[tauri::command]
pub async fn discard_audio(handle: AppHandle, file_id: String) -> CommandResult<()> {
    blocking(move || { crate::audio_upload::discard(&handle, &file_id); Ok(()) }).await
}


#[tauri::command]
pub async fn share_link(handle: AppHandle, url: String, title: String) -> CommandResult<()> {
    blocking(move || crate::share::share(&handle, &url, &title)).await
}

#[tauri::command]
pub async fn copy_link(handle: AppHandle, url: String) -> CommandResult<()> {
    blocking(move || crate::share::copy(&handle, &url)).await
}


/// Local Android network cost hint. No request to a server is made.
#[tauri::command]
pub async fn network_usage(handle: AppHandle) -> CommandResult<serde_json::Value> {
    blocking(move || {
        #[cfg(target_os = "android")]
        {
            use tauri_plugin_vk_login::VkLoginExt;
            handle.vk_login().network_usage().map_err(|_| Error::Offline)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = handle;
            Ok(serde_json::json!({"connected":true,"metered":false}))
        }
    }).await
}


#[tauri::command]
pub async fn restore_radio(state: TauriState<'_, AppState>, source: RadioSource, tracks: Vec<Track>, index: usize, position: f64, next_offset: Option<u32>) -> CommandResult<()> {
    let tracks = checked_queue(tracks)?;
    let app = state.app.clone();
    blocking(move || app.restore_radio(source, tracks, index, position, next_offset)).await
}


#[tauri::command]
pub async fn share_audio(handle: AppHandle, state: TauriState<'_, AppState>, track: Track, allow_download: Option<bool>) -> CommandResult<bool> {
    let track=checked_queue(vec![track])?.pop().ok_or(Error::InvalidRequest)?;
    let app=state.app.clone();
    blocking(move || crate::audio_share::share(&handle,&app,&track,allow_download.unwrap_or(false))).await
}


#[tauri::command]
pub async fn choose_cover(handle: AppHandle, state: TauriState<'_, AppState>) -> CommandResult<Option<crate::playlist_cover::CoverInfo>> {
    let app=state.app.clone();blocking(move ||crate::playlist_cover::choose(&handle,&app)).await
}
#[tauri::command]
pub async fn set_playlist_cover(handle: AppHandle, state: TauriState<'_, AppState>, file_id:String, playlist:Playlist) -> CommandResult<()> {
    let app=state.app.clone();blocking(move ||crate::playlist_cover::upload(&handle,&app,&file_id,&playlist)).await
}
#[tauri::command]
pub async fn discard_cover(handle:AppHandle,file_id:String)->CommandResult<()> {
    blocking(move ||{crate::playlist_cover::discard(&handle,&file_id);Ok(())}).await
}


/// Generated image fixtures only; no user image or account is needed by the emulator check.
#[tauri::command]
pub async fn debug_cover_selftest(handle: AppHandle) -> CommandResult<String> {
    #[cfg(all(target_os="android",debug_assertions))]
    {
        use tauri_plugin_vk_login::VkLoginExt;
        tauri::async_runtime::spawn_blocking(move ||handle.vk_login().cover_selftest())
            .await.map_err(|_|CommandError {kind:"cover_selftest".into(),message:"image check interrupted".into(),retry_after:None})?
            .map_err(|error|CommandError {kind:"cover_selftest".into(),message:error.to_string(),retry_after:None})
    }
    #[cfg(not(all(target_os="android",debug_assertions)))]
    {
        let _=handle;
        Err(CommandError {kind:"unsupported".into(),message:"available in Android debug builds only".into(),retry_after:None})
    }
}
