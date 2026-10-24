// Какие треки есть на этом компьютере, везде кнопки скачать.

import { api, trackKey } from "./api.js";
import { store, publish } from "./store.js";
import { t } from "./i18n.js";
import { downloadPermission } from "./download-permission.js";
import { accountId } from "./personal.js";
import { readPersonal,writePersonal } from "./personal.js";

let job = null;
export const downloadJob = () => job;
export function cancelDownloads() { if (job) { job.cancelled = true; publish("downloadJob"); } }
let tracks = [];
let bytes = 0;
const keys = new Set();
const pending = new Set();
let refreshGeneration = 0;

export const isDownloaded = (track) => !!track && keys.has(trackKey(track));
export const isDownloading = (track) => !!track && pending.has(trackKey(track));
export const downloads = () => ({ tracks, bytes });

export async function refreshDownloads() {
  const generation = ++refreshGeneration;
  try {
    const result = await api.downloads();
    if (generation !== refreshGeneration) return;
    tracks = result.tracks;
    bytes = result.bytes;
    keys.clear();
    for (const track of tracks) keys.add(trackKey(track));
  } catch {
    // Keep the last known offline files when the refresh is unavailable.
  }
  publish("downloads");
}

/** Разрешается как "added" / "removed"; отклоняется с ошибкой API. */
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
    const account=accountId();
    const allowed=downloadPermission();
    if (!await allowed()) return null;
    await api.download(track);
    if(account!==accountId())return null;
    writePersonal("manualDownloads",[...new Set([...readPersonal("manualDownloads",[]),key])].slice(-20000));
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

/** Загружает по одному элементу за раз; отмена останавливает планирование новых запросов. */
export async function downloadAll(items, { isCancelled = () => false, onProgress = () => {}, managed=false } = {}) {
  if (job?.running) return null;
  const unique = [...new Map(items.map(track => [trackKey(track),track])).values()].slice(0,20000);
  const state = { running:true,cancelled:false,completed:0,total:unique.length,failed:0,current:null,errors:[] };
  const account = accountId();
  const cancelled = () => state.cancelled || isCancelled() || account !== accountId() || !store.session.signed_in;
  const allowed=downloadPermission({interactive:!managed,isCancelled:cancelled,onBlocked:reason=>{state.blocked=reason;}});
  job=state;
  publish("downloadJob");
  try {
    for (const track of unique) {
      if (cancelled()) { state.cancelled=true; break; }
      const key=trackKey(track);
      state.current=track;
      publish("downloadJob");
      const needsDownload=!keys.has(key) && !pending.has(key);
      if(needsDownload && !await allowed()){state.cancelled=true;break;}
      if(cancelled()){state.cancelled=true;break;}
      if(!managed)writePersonal("manualDownloads",[...new Set([...readPersonal("manualDownloads",[]),key])].slice(-20000));
      if (needsDownload) {
        pending.add(key); publish("downloads");
        try { await api.download(track); keys.add(key); }
        catch (error) { state.failed++; state.errors.push({track,error}); }
        finally { pending.delete(key); publish("downloads"); }
      }
      state.completed++;
      if(cancelled()){state.cancelled=true;break;}
      onProgress(state.completed,state.total,state.failed);
      publish("downloadJob");
    }
    return {completed:state.completed,failed:state.failed,cancelled:state.cancelled,...(state.blocked ? {blocked:state.blocked} : {})};
  } finally {
    state.running=false; state.current=null;
    await refreshDownloads();
    publish("downloadJob");
  }
}
