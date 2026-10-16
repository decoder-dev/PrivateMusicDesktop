import { invoke } from "./bridge.js";

// Every call resolves to data or rejects with { kind, message }.
export const api = {
  sessionState: () => invoke("session_state"),
  profile: () => invoke("profile"),
  loginStart: () => invoke("login_start"),
  logout: () => invoke("logout"),
  library: (offset, count) => invoke("library", { offset, count }),
  playlists: (offset, count) => invoke("playlists", { offset, count }),
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
  createPlaylist: (title, description) => invoke("create_playlist", { title, description }),
  editPlaylist: (playlist, title, description) => invoke("edit_playlist", { playlist, title, description }),
  deletePlaylist: (playlist) => invoke("delete_playlist", { playlist }),
  addToPlaylist: (playlist, track) => invoke("add_to_playlist", { playlist, track }),
  removeFromPlaylist: (playlist, track) => invoke("remove_from_playlist", { playlist, track }),
  downloads: () => invoke("downloads"),
  download: (track) => invoke("download", { track }),
  removeDownload: (trackId) => invoke("remove_download", { trackId }),
  mix: () => invoke("mix"),
  recommendations: () => invoke("recommendations"),
  similar: (track) => invoke("similar", { track }),
  addToLibrary: (track) => invoke("add_to_library", { track }),
  removeFromLibrary: (track) => invoke("remove_from_library", { track }),
  lyrics: (track) => invoke("lyrics", { track }),
  outputDevices: () => invoke("output_devices"),
  setOutputDevice: (name) => invoke("set_output_device", { name }),
  setCloseToTray: (on) => invoke("set_close_to_tray", { on }),
  appInfo: () => invoke("app_info"),
  setEqualizer: (gains) => invoke("set_equalizer", { gains }),
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
};

export const trackKey = (track) => `${track.owner_id}_${track.track_id}`;

/** Pulls every page of a paged call, reporting progress as it goes. */
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
