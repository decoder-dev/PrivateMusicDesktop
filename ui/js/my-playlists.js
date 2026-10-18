// Собственные плейлисты пользователя (те, которые он может редактировать) для меню "Add to playlist" и
// экрана плейлистов.

import { api, loadAllPages } from "./api.js";
import { store, publish } from "./store.js";

let all = [];
let loaded = false;
let loading = null;
let epoch = 0;

const userId = () => store.session.profile?.id ?? null;
/**
 * Плейлисты, которые пользователь может редактировать: собственные (позитивный владелец = человек, а
 * не сообщество).
 */
export const isOwn = (playlist) => (userId() !== null ? playlist.owner_id === userId() : playlist.owner_id > 0);

export function setPlaylists(list) {
  all = list.slice();
  loaded = true;
  publish("playlists");
}

export function allPlaylists() { return all; }

export function ownPlaylists() {
  return all.filter(isOwn);
}

/** Загружается один раз; последующие вызовы повторно используют его. */
export function ensurePlaylists() {
  if (loaded) return Promise.resolve(all);
  if (loading) return loading;
  const mine = epoch;
  const request = loadAllPages((offset,count) => api.playlists(offset,count), { pageSize:100, isCancelled: () => mine !== epoch })
    .then(list => { if (mine !== epoch) return []; setPlaylists(list ?? []); return all; })
    .finally(() => { if (loading === request) loading = null; });
  loading = request;
  return request;
}

export function forgetPlaylists() {
  epoch++;
  loading = null;
  all = [];
  loaded = false;
  publish("playlists");
}

export function addPlaylist(playlist) {
  all = [playlist, ...all.filter((p) => !same(p, playlist))];
  publish("playlists");
}

export function replacePlaylist(playlist) {
  all = all.map((p) => (same(p, playlist) ? playlist : p));
  publish("playlists");
}

export function dropPlaylist(playlist) {
  all = all.filter((p) => !same(p, playlist));
  publish("playlists");
}

export function changeCount(playlist, delta) {
  replacePlaylist({ ...playlist, count: Math.max(0, (all.find((p) => same(p, playlist)) ?? playlist).count + delta) });
}

export const same = (a, b) => a.owner_id === b.owner_id && a.playlist_id === b.playlist_id;
