// Which tracks are on this computer, for the download buttons everywhere.

import { api, trackKey } from "./api.js";
import { publish } from "./store.js";
import { t } from "./i18n.js";

let job = null;
export const downloadJob = () => job;
export function cancelDownloads() { if (job) { job.cancelled = true; publish("downloadJob"); } }
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

/** Downloads one item at a time; cancellation stops scheduling new requests. */
export async function downloadAll(items, { isCancelled = () => false, onProgress = () => {} } = {}) {
  if (job?.running) return null;
  const unique = [...new Map(items.map(track => [trackKey(track),track])).values()].slice(0,20000);
  const state = { running:true,cancelled:false,completed:0,total:unique.length,failed:0,current:null,errors:[] };
  job=state;
  publish("downloadJob");
  try {
    for (const track of unique) {
      if (state.cancelled || isCancelled()) { state.cancelled=true; break; }
      const key=trackKey(track);
      state.current=track;
      publish("downloadJob");
      if (!keys.has(key) && !pending.has(key)) {
        pending.add(key); publish("downloads");
        try { await api.download(track); keys.add(key); }
        catch (error) { state.failed++; state.errors.push({track,error}); }
        finally { pending.delete(key); publish("downloads"); }
      }
      state.completed++;
      onProgress(state.completed,state.total,state.failed);
      publish("downloadJob");
    }
    return {completed:state.completed,failed:state.failed,cancelled:state.cancelled};
  } finally {
    state.running=false; state.current=null;
    await refreshDownloads();
    publish("downloadJob");
  }
}
