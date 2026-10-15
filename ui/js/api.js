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
  addToLibrary: (track) => invoke("add_to_library", { track }),
  removeFromLibrary: (track) => invoke("remove_from_library", { track }),
  lyrics: (track) => invoke("lyrics", { track }),
  setVisualizer: (on) => invoke("set_visualizer", { on }),
  artworkPalette: (url) => invoke("artwork_palette", { url }),
  setFullscreen: (on) => invoke("set_fullscreen", { on }),
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
