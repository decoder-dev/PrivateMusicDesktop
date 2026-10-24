import { PALETTES } from "./themes.js";
// Что сохраняется при перезапуске: громкость, перемешивание, повтор и последняя очередь. Хранится в
// собственном localStorage окна; здесь никогда не хранятся никакие учетные данные.

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
    // Память заполнена или недоступна: забывание допустимо.
  }
}

function readEq(saved) {
  const gains = Array.isArray(saved?.gains) && saved.gains.length === 10 && saved.gains.every(Number.isFinite)
    ? saved.gains.map((g) => Math.min(12, Math.max(-12, g)))
    : Array(10).fill(0);
  const custom=Array.isArray(saved?.custom) && saved.custom.length===10 && saved.custom.every(Number.isFinite) ? saved.custom.map(g=>Math.min(12,Math.max(-12,g))) : gains.slice();
  return { on: saved?.on === true, gains, custom, preset: typeof saved?.preset === "string" ? saved.preset : "flat" };
}

export const ACCENTS = ["blue", "indigo", "violet", "pink", "red", "orange", "green", "teal", "banana"];
// В более старых версиях два акцента назывались другими именами.
const RENAMED = { purple: "violet", yellow: "banana" };

export function loadSettings() {
  const saved = read(SETTINGS) ?? {};
  const volume = Number.isFinite(saved.volume) ? Math.min(1, Math.max(0, saved.volume)) : 1;
  return {
    volume,
    prefetch:saved.prefetch!==false,
    gapless:saved.gapless!==false,
    crossfade:Number.isFinite(saved.crossfade)?Math.max(0,Math.min(10,saved.crossfade)):0,
    resumeBluetooth:saved.resumeBluetooth!==false,
    pauseAtZero:saved.pauseAtZero===true,
    highQuality:saved.highQuality!==false,
    advanceOnError:saved.advanceOnError!==false,
    textScale:["compact","system","large","extraLarge"].includes(saved.textScale)?saved.textScale:"system",
    homeStage:saved.homeStage!==false,
    normalization:saved.normalization===true,
    audio: {
      preamp:Number.isFinite(saved.audio?.preamp)?Math.max(-12,Math.min(6,saved.audio.preamp)):0,
      compression:saved.audio?.compression===true,
      spatial:saved.audio?.spatial===true,
      spatialIntensity:Number.isFinite(saved.audio?.spatialIntensity)?Math.max(0,Math.min(1,saved.audio.spatialIntensity)):0.35,
    },
    shuffle: saved.shuffle === true,
    repeat: ["off", "all", "one"].includes(saved.repeat) ? saved.repeat : "off",
    lyrics: saved.lyrics !== false,
    closeToTray: saved.closeToTray !== false,
    eq: readEq(saved.eq),
    outputDevice: typeof saved.outputDevice === "string" && saved.outputDevice ? saved.outputDevice : null,
    visualizer: ["bars", "mirror", "wave", "radial", "off"].includes(saved.visualizer) ? saved.visualizer : null,
    palette: PALETTES.includes(saved.palette) ? saved.palette : "classic",
    theme: ["system", "light", "dark"].includes(saved.theme) ? saved.theme : "system",
    accent: ACCENTS.includes(RENAMED[saved.accent] ?? saved.accent) ? (RENAMED[saved.accent] ?? saved.accent) : "green",
    flatChrome: saved.flatChrome === true,
    haptics: saved.haptics !== false,
  };
}

export function saveSettings(settings) {
  write(SETTINGS, settings);
}

/** Срок действия потока URLs истекает, поэтому сохраненный трек никогда не будет содержать его. */
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
    artists: Array.isArray(track.artists) ? track.artists : [],
    album: track.album ?? null,
  };
}

export function saveQueue(tracks, index, position) {
  if (!tracks.length || index == null || index < 0 || index >= tracks.length) {
    try { localStorage.removeItem(QUEUE); } catch { /* хранилище может быть недоступно */ }
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
  try { localStorage.removeItem(QUEUE); } catch { /* хранилище может быть недоступно */ }
}
