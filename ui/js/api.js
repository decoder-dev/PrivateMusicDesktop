import { invoke } from "./bridge.js";

// Каждый вызов преобразуется в данные или отклоняется с помощью { kind, message }.
export const api = {
  sessionState: () => invoke("session_state"),
  profile: () => invoke("profile"),
  loginStart: (reset = false) => invoke("login_start", { reset }),
  logout: () => invoke("logout"),
  library: (offset, count) => invoke("library", { offset, count }),
  playlists: (offset, count) => invoke("playlists", { offset, count }),
  playlistById: (album) => invoke("playlist_by_id", { album }),
  playlistTracks: (playlist, offset, count) => invoke("playlist_tracks", { playlist, offset, count }),
  search: (query, offset, count) => invoke("search", { query, offset, count }),
  playerState: () => invoke("player_state"),
  playTracks: (tracks, index) => invoke("play_tracks", { tracks, index }),
  loadQueue: (tracks, index, position) => invoke("load_queue", { tracks, index, position }),
  toggle: () => invoke("toggle"),
  next: () => invoke("next"),
  previous: () => invoke("previous"),
  seek: (seconds) => invoke("seek", { seconds }),
  setVolume: (volume) => invoke("set_volume", { volume }),
  setShuffle: (on) => invoke("set_shuffle", { on }),
  setRepeat: (repeat) => invoke("set_repeat", { repeat }),
  jump: (index) => invoke("jump", { index }),
  playNext: (track) => invoke("play_next", { track }),
  playLast: (track) => invoke("play_last", { track }),
  removeFromQueue: (index) => invoke("remove_from_queue", { index }),
  stop: () => invoke("stop"),
  exportTrack: (track) => invoke("export_track", { track }),
  setSleepTimer: (seconds, endOfTrack = false) => invoke("set_sleep_timer", { seconds, endOfTrack }),
  sleepTimer: () => invoke("sleep_timer"),
  clearUpcoming: () => invoke("clear_upcoming"),
  moveUpcoming: (from, to) => invoke("move_upcoming", { from, to }),
  shareLink: (url, title) => invoke("share_link", { url, title }),
  shareAudio: (track, allowDownload = false) => invoke("share_audio", {track,allowDownload}),
  networkUsage: () => invoke("network_usage"),
  copyLink: (url) => invoke("copy_link", { url }),
  chooseCover: () => invoke("choose_cover"),
  setPlaylistCover: (fileId,playlist) => invoke("set_playlist_cover",{fileId,playlist}),
  discardCover: fileId => invoke("discard_cover",{fileId}),
  chooseAudio: () => invoke("choose_audio"),
  uploadAudio: (fileId, artist, title) => invoke("upload_audio", { fileId, artist, title }),
  discardAudio: (fileId) => invoke("discard_audio", { fileId }),
  createPlaylist: (title, description, hidden = null) => invoke("create_playlist", { title, description, hidden }),
  editPlaylist: (playlist, title, description, hidden = null) => invoke("edit_playlist", { playlist, title, description, hidden }),
  deletePlaylist: (playlist) => invoke("delete_playlist", { playlist }),
  addToPlaylist: (playlist, track) => invoke("add_to_playlist", { playlist, track }),
  removeFromPlaylist: (playlist, track) => invoke("remove_from_playlist", { playlist, track }),
  downloads: () => invoke("downloads"),
  download: (track) => invoke("download", { track }),
  removeDownload: (trackId) => invoke("remove_download", { trackId }),
  mix: (options = null, offset = 0) => invoke("mix", { options, offset }),
  mixSettings: () => invoke("mix_settings"),
  catalogMixes: () => invoke("catalog_mixes"),
  streamMix: (mixId, options = null, offset = 0) => invoke("stream_mix", { mixId, options, offset }),
  newReleases: () => invoke("new_releases"),
  /**
   * Воспроизводит очередь, которая продолжает пополняться из `source`: { kind:
   * "personal"|"catalog"|"similar", … }.
   */
  restoreRadio: (context, tracks, index, position) => invoke("restore_radio", {source:context.source,tracks,index,position,nextOffset:context.next_offset}),
  playRadio: (source, tracks, index) => { const { next_offset, ...radio } = source; return invoke("play_radio", { source:radio, tracks, index, nextOffset:next_offset ?? null }); },
  recommendations: () => invoke("recommendations"),
  similar: (track) => invoke("similar", { track }),
  addToLibrary: (track) => invoke("add_to_library", { track }),
  removeFromLibrary: (track) => invoke("remove_from_library", { track }),
  lyrics: (track) => invoke("lyrics", { track }),
  lyricsVersions: (track) => invoke("lyrics_versions", { track }),
  outputDevices: () => invoke("output_devices"),
  setOutputDevice: (name) => invoke("set_output_device", { name }),
  setCloseToTray: (on) => invoke("set_close_to_tray", { on }),
  appInfo: () => invoke("app_info"),
  updateState: () => invoke("update_state"),
  updateCheck: () => invoke("update_check"),
  updateDownload: () => invoke("update_download"),
  updateCancel: () => invoke("update_cancel"),
  updateAccess: (token) => invoke("update_access", { token }),
  updateOpen: () => invoke("update_open"),
  setWindowMode: (mode) => invoke("set_window_mode", { mode }),
  setEqualizer: (gains) => invoke("set_equalizer", { gains }),
  setTaste: (taste,userId) => invoke("set_taste", { taste,userId }),
  setTransitions: (prefetch,gapless,crossfade) => invoke("set_transitions", { prefetch,gapless,crossfade }),
  setNormalization: (on) => invoke("set_normalization", { on }),
  setVisualizer: (on) => invoke("set_visualizer", { on }),
  artworkPalette: (url) => invoke("artwork_palette", { url }),
  setFullscreen: (on) => invoke("set_fullscreen", { on }),
  searchArtists: (query, offset, count) => invoke("search_artists", { query, offset, count }),
  findArtist: (name) => invoke("find_artist", { name }),
  artistTracks: (artistId, offset, count) => invoke("artist_tracks", { artistId, offset, count }),
  artistAlbums: (artistId, offset, count) => invoke("artist_albums", { artistId, offset, count }),
  searchAlbums: (query, offset, count) => invoke("search_albums", { query, offset, count }),
  followedAlbums: (offset, count) => invoke("followed_albums", { offset, count }),
  albumTracks: (album, offset, count) => invoke("album_tracks", { album, offset, count }),
  followAlbum: (album, follow) => invoke("follow_album", { album, follow }),
  friends: (offset, count) => invoke("friends", { offset, count }),
  friendTracks: (ownerId, offset, count) => invoke("friend_tracks", { ownerId, offset, count }),
  friendPlaylists: (ownerId, offset, count) => invoke("friend_playlists", { ownerId, offset, count }),
  history: () => invoke("history"),
  monthPlays: () => invoke("month_plays"),
  historyRemove: (trackId) => invoke("history_remove", { trackId }),
  historyClear: () => invoke("history_clear"),
  listeningStats: (scope, choice = null) => invoke("listening_stats", { scope, choice }),
  resetListeningStats: () => invoke("reset_listening_stats"),
  setUtcOffset: (minutes) => invoke("set_utc_offset", { minutes }),
  /**
   * Сообщает движку, привлекает ли окно внимание пользователя (он наблюдает за сетью только тогда или
   * во время игры).
   */
  networkFocus: (focused) => invoke("network_focus", { focused }),
  /** «Повторить» после сбоя соединения: механизм отбрасывает соединения из пула и просматривает сеть. */
  networkCheck: () => invoke("network_check"),
  networkProbe: () => invoke("network_probe"),
  /** «Повторить» для воспроизведения, остановленного из-за проблемы с подключением. */
  retryPlayback: () => invoke("retry_playback"),
};

export const trackKey = (track) => `${track.owner_id}_${track.track_id}`;

/** Извлекает каждую страницу пейджингового вызова, сообщая о ходе выполнения. */
export async function loadAllPages(fetchPage, { pageSize = 200, onProgress, isCancelled = () => false } = {}) {
  const items = [];
  let offset = 0;
  for (;;) {
    const page = await fetchPage(offset, pageSize);
    if (isCancelled()) return null;
    items.push(...page.items);
    onProgress?.(items.length, page.total, items);
    if (page.next_offset == null || page.next_offset <= offset) return items;
    offset = page.next_offset;
  }
}
