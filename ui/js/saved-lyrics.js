import { accountId, readPersonal, writePersonal } from './personal.js';
import { boundedLyrics, savedLyricsEntry, searchSavedLyrics } from './saved-lyrics-model.js';
const NAME = 'savedLyrics.v1';
const key = track => `${track.owner_id}_${track.track_id}`;
export const savedLyrics = track => readEntries().find(entry => key(entry.track) === key(track))?.lyrics ?? null;
export const findSavedLyrics = query => searchSavedLyrics(readEntries(), query);
function readEntries() { return boundedLyrics(readPersonal(NAME, [])); }
export function rememberLyrics(track, lyrics) {
  if (!accountId()) return false;
  const entry = savedLyricsEntry(track, lyrics);
  if (!entry) return false;
  const old = readEntries();
  if (JSON.stringify(old[0]) === JSON.stringify(entry)) return true;
  let next = boundedLyrics([entry, ...old.filter(item => key(item.track) !== key(track))]);
  // A quota failure leaves the previous value intact; progressively evict oldest entries.
  while (next.length) {
    try { writePersonal(NAME, next); return true; } catch { next = next.slice(0, -1); }
  }
  return false;
}
export function forgetSavedLyrics(track) {
  writePersonal(NAME, readEntries().filter(entry => key(entry.track) !== key(track)));
}
