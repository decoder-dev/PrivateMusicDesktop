// Account-local, bounded text index. Playback addresses and VK credentials never belong here.
export const SAVED_LYRICS_LIMIT = 150;
export const SAVED_LYRICS_BUDGET = 1_000_000;
const fold = value => String(value ?? '').normalize('NFKD').replace(/\p{M}/gu, '').toLocaleLowerCase();
export function savedLyricsEntry(track, lyrics) {
  if (!track || !Number.isSafeInteger(track.owner_id) || !track.owner_id || !Number.isSafeInteger(track.track_id) || track.track_id <= 0) return null;
  if (typeof lyrics?.text !== 'string' || !lyrics.text.trim() || new TextEncoder().encode(lyrics.text).length > 150_000) return null;
  const lines = lyrics.lines ?? [];
  if (!Array.isArray(lines) || lines.length > 10_000 || lines.some(line => !Number.isFinite(line?.time) || line.time < 0 || typeof line.text !== 'string')) return null;
  const saved = {
    track: { owner_id: track.owner_id, track_id: track.track_id, title: String(track.title ?? '').slice(0, 1000), artist: String(track.artist ?? '').slice(0, 1000),
      duration: Number.isInteger(track.duration) && track.duration >= 0 ? track.duration : 0,
      album_title: null, artwork_url: null, access_key: null, stream_url: null, lyrics_id: null, track_code: null, is_hq: false, artists: [], album: null },
    lyrics: { text: lyrics.text, source: String(lyrics.source ?? '').slice(0, 200), lines: lines.map(({time, text}) => ({time, text})) },
  };
  return JSON.stringify(saved).length <= 256_000 ? saved : null;
}
export function boundedLyrics(entries) {
  if (!Array.isArray(entries)) return [];
  const seen = new Set(), result = [];
  let size = 2;
  for (const raw of entries.slice(0, SAVED_LYRICS_LIMIT + 1)) {
    const entry = savedLyricsEntry(raw?.track, raw?.lyrics);
    if (!entry) continue;
    const key = `${entry.track.owner_id}_${entry.track.track_id}`;
    if (seen.has(key)) continue;
    const length = JSON.stringify(entry).length + 1;
    if (size + length > SAVED_LYRICS_BUDGET) continue;
    seen.add(key); size += length; result.push(entry);
    if (result.length >= SAVED_LYRICS_LIMIT) break;
  }
  return result;
}
export function searchSavedLyrics(entries, query) {
  const words = fold(query).trim().split(/\s+/u).filter(Boolean);
  if (words.join('').length < 2) return [];
  return entries.filter(({track, lyrics}) => {
    const haystack = fold(`${track.artist}\n${track.title}\n${lyrics.text}`);
    return words.every(word => haystack.includes(word));
  }).map(entry => ({...entry, excerpt: (entry.lyrics.text.split(/\r?\n/).find(line => words.some(word => fold(line).includes(word))) ?? entry.lyrics.text).slice(0, 200)}));
}
