// The releases the user follows, for the Albums shelf and the follow button
// on album pages.

import { api, loadAllPages } from "./api.js";
import { publish } from "./store.js";

let all = [];
let loaded = false;
let loading = null;
let epoch = 0;
// Changes made here since the list was loaded: album key -> followed.
const decided = new Map();

export const albumKey = (album) => `${album.owner_id}_${album.album_id}`;

export function followedAlbums() { return all; }

/** Loads once; later calls reuse it. */
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

/** Follows or unfollows; resolves to the new state, rejects as the API does. */
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
