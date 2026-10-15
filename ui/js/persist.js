// What survives a restart: volume, shuffle, repeat and the last queue.
// Stored in the window's own localStorage; no credentials are ever kept here.

const SETTINGS = "pm.settings.v1";
const QUEUE = "pm.queue.v1";
const MAX_SAVED = 500;

function read(key) {
  try {
    return JSON.parse(localStorage.getItem(key) ?? "null");
  } catch {
    return null;
  }
}

function write(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage full or unavailable: forgetting is acceptable.
  }
}

export function loadSettings() {
  const saved = read(SETTINGS) ?? {};
  const volume = Number.isFinite(saved.volume) ? Math.min(1, Math.max(0, saved.volume)) : 1;
  return {
    volume,
    shuffle: saved.shuffle === true,
    repeat: ["off", "all", "one"].includes(saved.repeat) ? saved.repeat : "off",
    visualizer: ["bars", "mirror", "wave", "radial", "off"].includes(saved.visualizer) ? saved.visualizer : null,
  };
}

export function saveSettings(settings) {
  write(SETTINGS, settings);
}

/** Stream URLs expire, so a saved track never carries one. */
function trim(track) {
  return {
    track_id: track.track_id,
    owner_id: track.owner_id,
    title: track.title,
    artist: track.artist,
    album_title: track.album_title ?? null,
    duration: track.duration,
    stream_url: null,
    artwork_url: track.artwork_url ?? null,
    access_key: track.access_key ?? null,
    lyrics_id: track.lyrics_id ?? null,
    is_hq: !!track.is_hq,
    track_code: track.track_code ?? null,
  };
}

export function saveQueue(tracks, index, position) {
  if (!tracks.length || index == null || index < 0 || index >= tracks.length) {
    localStorage.removeItem(QUEUE);
    return;
  }
  let start = 0;
  let end = tracks.length;
  if (tracks.length > MAX_SAVED) {
    start = Math.max(0, Math.min(index - 50, tracks.length - MAX_SAVED));
    end = start + MAX_SAVED;
  }
  write(QUEUE, { tracks: tracks.slice(start, end).map(trim), index: index - start, position });
}

export function loadQueue() {
  const saved = read(QUEUE);
  if (!saved || !Array.isArray(saved.tracks) || !saved.tracks.length) return null;
  const index = Number.isInteger(saved.index) ? saved.index : 0;
  if (index < 0 || index >= saved.tracks.length) return null;
  return { tracks: saved.tracks, index, position: Number.isFinite(saved.position) ? saved.position : 0 };
}

export function clearQueue() {
  localStorage.removeItem(QUEUE);
}
