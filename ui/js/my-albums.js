// Релизы, за которыми следит пользователь, отображаются на полке «Альбомы» и кнопке «подписаться» на
// страницах альбомов.

import { api, loadAllPages } from "./api.js";
import { publish } from "./store.js";

let all = [];
let loaded = false;
let loading = null;
let epoch = 0;
// Изменения, внесенные здесь с момента загрузки списка: ключ альбома -> следовать.
const decided = new Map();

export const albumKey = (album) => `${album.owner_id}_${album.album_id}`;

export function followedAlbums() { return all; }

/** Загружается один раз; последующие вызовы повторно используют его. */
export function ensureAlbums() {
  if (loaded) return Promise.resolve(all);
  if (loading) return loading;
  const mine = epoch;
  const request = loadAllPages((offset,count) => api.followedAlbums(offset,count), { pageSize:100, isCancelled: () => mine !== epoch })
    .then(list => { if (mine !== epoch) return []; all = list ?? []; loaded = true; publish("albums"); return all; })
    .finally(() => { if (loading === request) loading = null; });
  loading = request;
  return request;
}

export function isFollowed(album) {
  const key = albumKey(album);
  if (decided.has(key)) return decided.get(key);
  return all.some((a) => albumKey(a) === key) || !!album.is_followed;
}

/** Подписывается или отписывается; переходит в новое состояние, отклоняется, как это делает API. */
export async function toggleFollow(album) {
  const mine = epoch;
  const follow = !isFollowed(album);
  await api.followAlbum(album, follow);
  if (mine !== epoch) return follow;
  const key = albumKey(album);
  decided.set(key, follow);
  all = all.filter((a) => albumKey(a) !== key);
  if (follow) all = [{ ...album, is_followed: true }, ...all];
  publish("albums");
  return follow;
}

export function forgetAlbums() {
  epoch++;
  loading = null;
  all = [];
  loaded = false;
  decided.clear();
  publish("albums");
}
