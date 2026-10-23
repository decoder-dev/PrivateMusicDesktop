import { radioSnapshot } from "./radio-snapshot.js";
import { api, trackKey } from "./api.js";
import { store } from "./store.js";
import { accountId, readPersonal, writePersonal, cleanTrack } from "./personal.js";
import { rememberQueue } from "./queue-history.js";
import { t } from "./i18n.js";
import "./mobile/strings.js";

const LIMIT = 200;
export function listenLater() {
  const value = readPersonal("listenLater", {});
  if (!Array.isArray(value.tracks) || !value.tracks.length || value.tracks.length > LIMIT ||
      !Number.isInteger(value.index) || value.index < 0 || value.index >= value.tracks.length ||
      !value.tracks.every(track => track && Number.isInteger(track.owner_id) && Number.isInteger(track.track_id) && typeof track.title === "string")) return null;
  return {...value,radio:radioSnapshot(value.radio)};
}
export function saveForLater() {
  const { tracks, current } = store.queue;
  if (!Number.isInteger(current) || !tracks[current] || !store.player.track ||
      trackKey(tracks[current]) !== trackKey(store.player.track)) throw new Error(t("later.noQueue"));
  const start = Math.min(current, Math.max(0, tracks.length - LIMIT));
  const duration = Number(store.player.duration) || 0;
  const position = Number.isFinite(store.player.position) ? Math.max(0, store.player.position) : 0;
  writePersonal("listenLater", {
    tracks: tracks.slice(start, start + LIMIT).map(cleanTrack), index: current - start,
    position: duration > 0 ? Math.min(position, duration) : position,
    radio: radioSnapshot(store.player.radio_context),
    shuffle: !!store.player.shuffle, repeat: store.player.repeat, savedAt: Date.now(),
  });
}
export const clearListenLater = () => writePersonal("listenLater", {});
export async function resumeLater() {
  const entry = listenLater();
  const account = accountId();
  if (!entry || !account) return;
  rememberQueue();
  const current = () => account === accountId() && store.session.signed_in;
  await api.setShuffle(!!entry.shuffle);
  if (!current()) return;
  await api.setRepeat(["off", "all", "one"].includes(entry.repeat) ? entry.repeat : "off");
  if (!current()) return;
  const position=Number.isFinite(entry.position) ? Math.max(0, entry.position) : 0;
  if(entry.radio) await api.restoreRadio(entry.radio,entry.tracks,entry.index,position);
  else await api.loadQueue(entry.tracks, entry.index, position);
  if (!current()) return;
  await api.toggle();
  // Keep the saved queue available until the user explicitly removes or replaces it.
}
