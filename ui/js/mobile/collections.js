// Device-local collection preferences belong to the signed-in account.
import { readPersonal, writePersonal } from "../personal.js";

export const collectionKey = (kind, item) => `${kind}:${item.owner_id}_${kind === "album" ? item.album_id : item.playlist_id}`;
export function pinnedCollections() {
  return readPersonal("pinnedCollections", []).filter((entry) =>
    entry && ["playlist", "album"].includes(entry.kind) && entry.item && entry.key === collectionKey(entry.kind, entry.item)).slice(0, 24);
}
export const isCollectionPinned = (kind, item) => pinnedCollections().some((entry) => entry.key === collectionKey(kind, item));
export function toggleCollectionPin(kind, item) {
  const key = collectionKey(kind, item);
  const entries = pinnedCollections();
  if (entries.some((entry) => entry.key === key)) {
    writePersonal("pinnedCollections", entries.filter((entry) => entry.key !== key));
  } else {
    if (entries.length >= 24) return false;
    writePersonal("pinnedCollections", [...entries, { key, kind, item }]);
  }
  return true;
}
export const collectionLayout = () => readPersonal("collectionLayout", "grid") === "list" ? "list" : "grid";
export const setCollectionLayout = (layout) => writePersonal("collectionLayout", layout);
