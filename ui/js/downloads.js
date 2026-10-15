// Which tracks are on this computer, for the download buttons everywhere.

import { api, trackKey } from "./api.js";
import { publish } from "./store.js";
import { t } from "./i18n.js";

let tracks = [];
let bytes = 0;
const keys = new Set();
const pending = new Set();

export const isDownloaded = (track) => !!track && keys.has(trackKey(track));
export const isDownloading = (track) => !!track && pending.has(trackKey(track));
export const downloads = () => ({ tracks, bytes });

export async function refreshDownloads() {
  try {
    const result = await api.downloads();
    tracks = result.tracks;
    bytes = result.bytes;
    keys.clear();
    for (const track of tracks) keys.add(trackKey(track));
  } catch {
    // Downloads unavailable: everything shows as not downloaded.
  }
  publish("downloads");
}

/** Resolves to "added" / "removed"; rejects with the API error. */
export async function toggleDownload(track) {
  const key = trackKey(track);
  if (pending.has(key)) return null;
  pending.add(key);
  publish("downloads");
  try {
    if (keys.has(key)) {
      await api.removeDownload(key);
      return "removed";
    }
    await api.download(track);
    return "added";
  } finally {
    pending.delete(key);
    await refreshDownloads();
  }
}

export function formatSize(value) {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} ${t("unit.gb")}`;
  if (value >= 1024 ** 2) return `${Math.round(value / 1024 ** 2)} ${t("unit.mb")}`;
  return `${Math.max(1, Math.round(value / 1024))} ${t("unit.kb")}`;
}
