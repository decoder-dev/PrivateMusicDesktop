// What is in "My music", for the hearts everywhere. A track added from search
// gets a new id in the library, so the copy is remembered under the key of the
// track it came from, and either one shows as liked.

import { api, trackKey } from "./api.js";
import { publish } from "./store.js";

const inLibrary = new Map(); // key of a library track -> the track
const copies = new Map(); // key of the source track -> its library copy
const pending = new Set();
const listeners = new Set();

/** The library view reports what it loaded. */
export function setLibrary(tracks) {
  inLibrary.clear();
  for (const track of tracks) inLibrary.set(trackKey(track), track);
  for (const [source, copy] of copies) if (!inLibrary.has(trackKey(copy))) copies.delete(source);
  publish("likes");
}

export function forgetLibrary() {
  inLibrary.clear();
  copies.clear();
  publish("likes");
}

function libraryCopy(track) {
  const key = trackKey(track);
  return inLibrary.get(key) ?? copies.get(key) ?? null;
}

export const isLiked = (track) => !!track && libraryCopy(track) !== null;
export const isBusy = (track) => !!track && pending.has(trackKey(track));

/** `listener({ added } | { removed })` — the library view keeps its list in step. */
export function onLibraryChange(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Adds or removes; resolves to "added" / "removed", rejects with the API error. */
export async function toggleLike(track) {
  const key = trackKey(track);
  if (pending.has(key)) return null;
  pending.add(key);
  publish("likes");
  try {
    const copy = libraryCopy(track);
    if (copy) {
      await api.removeFromLibrary(copy);
      inLibrary.delete(trackKey(copy));
      for (const [source, value] of copies) if (trackKey(value) === trackKey(copy)) copies.delete(source);
      listeners.forEach((fn) => fn({ removed: copy }));
      return "removed";
    }
    const added = await api.addToLibrary(track);
    inLibrary.set(trackKey(added), added);
    if (trackKey(added) !== key) copies.set(key, added);
    listeners.forEach((fn) => fn({ added }));
    return "added";
  } finally {
    pending.delete(key);
    publish("likes");
  }
}
