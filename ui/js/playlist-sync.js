import { isAndroid } from "./mobile/layout.js";
import { t } from "./i18n.js";
import { ft } from "./feature-i18n.js";
import { api, loadAllPages, trackKey } from "./api.js";
import {
  downloads,
  refreshDownloads,
  downloadAll,
  downloadJob,
} from "./downloads.js";
import { accountId, readPersonal, writePersonal } from "./personal.js";
import { publish, own, subscribe } from "./store.js";

const keyOf = (playlist) => `${playlist.owner_id}_${playlist.playlist_id}`;
export function syncRules() {
  const rules = readPersonal("playlistSync", []);
  return Array.isArray(rules) ? rules.slice(0, 100) : [];
}
export const isSynced = (playlist) =>
  syncRules().some((rule) => rule.key === keyOf(playlist));
export function setPlaylistSync(playlist, on) {
  const rules = syncRules().filter((rule) => rule.key !== keyOf(playlist));
  if (on)
    rules.push({
      key: keyOf(playlist),
      playlist: { ...playlist, artwork_url: null },
      owned: [],
      last: 0,
      error: null,
    });
  writePersonal("playlistSync", rules);
  publish("playlistSync");
}
let running = false;
export function sharedProtection(currentKey){
  const manual=new Set(),protectedKeys=new Set();let complete=true;
  const addKeys=(target,values)=>{for(const id of values){if(typeof id==='string' && /^-?\d+_\d+$/.test(id))target.add(id);else complete=false;}};
  // Файлы загрузок общие для компьютера: чужое локальное закрепление тоже защищает файл.
  for(let i=0;i<localStorage.length;i++){
    const key=localStorage.key(i);if(!/^pm\.personal\.[^.]+\.(playlistSync|manualDownloads|autoLikeDownloads)$/.test(key || ""))continue;
    let values;try{values=JSON.parse(localStorage.getItem(key));}catch{complete=false;continue;}
    if(!Array.isArray(values)){complete=false;continue;}
    if(key.endsWith(".manualDownloads") || key.endsWith(".autoLikeDownloads")){addKeys(manual,values);continue;}
    for(const other of values){
      if(!other || typeof other!=='object' || Array.isArray(other)){complete=false;continue;}
      if(key===`pm.personal.${accountId()}.playlistSync` && other.key===currentKey)continue;
      if(!Array.isArray(other.wanted))complete=false;
      const ids=Array.isArray(other.wanted)?other.wanted:Array.isArray(other.owned)?other.owned:[];
      addKeys(protectedKeys,ids);
    }
  }
  return {manual,protectedKeys,complete};
}
export async function syncPlaylists() {
  if (running || downloadJob()?.running || !accountId()) return;
  running = true;
  publish("playlistSync");
  const id = accountId();
  try {
    await refreshDownloads();
    for (const original of syncRules()) {
      if (id !== accountId() || !isSynced(original.playlist)) break;
      let rule = { ...original };
      try {
        const items = await loadAllPages(
          (offset, count) => api.playlistTracks(rule.playlist, offset, count),
          { isCancelled: () => id !== accountId() || !isSynced(rule.playlist) },
        );
        if (!items || id !== accountId()) break;
        const before = new Set(downloads().tracks.map(trackKey));
        const result = await downloadAll(items, {
          managed: true,
          isCancelled: () => id !== accountId() || !isSynced(rule.playlist),
        });
        // Another job may have started while metadata was loading. Keep this rule pending.
        if (!result) break;
        if (id !== accountId()) break;
        const existing = new Set(downloads().tracks.map(trackKey));
        const wanted = new Set(items.map(trackKey));
        const created = items
          .map(trackKey)
          .filter((key) => !before.has(key) && existing.has(key));
        rule.owned = [...new Set([...(rule.owned || []), ...created])];
        const protection=sharedProtection(rule.key);
        if (
          result &&
          !result.cancelled &&
          !result.failed &&
          readPersonal("syncRemoveObsolete", false) &&
          protection.complete &&
          syncRules()
            .filter((other) => other.key !== rule.key)
            .every((other) => Array.isArray(other.wanted))
        ) {
          const {manual,protectedKeys}=protection;
          let observedManual=null;
          for (const key of rule.owned) {
            if (id !== accountId() || !isSynced(rule.playlist)) break;
            // A manual save may happen while an earlier removal is awaiting IPC.
            // Reparse only when this account's stored list actually changed.
            const raw=localStorage.getItem(`pm.personal.${id}.manualDownloads`);
            if(raw!==observedManual){
              const retained=JSON.parse(raw ?? "[]");
              if(!Array.isArray(retained))throw new Error(ft("Не удалось синхронизировать"));
              for(const saved of retained)manual.add(saved);
              observedManual=raw;
            }
            if (!wanted.has(key) && !manual.has(key) && !protectedKeys.has(key))
              await api.removeDownload(key);
          }
          rule.owned = rule.owned.filter(
            (key) =>
              wanted.has(key) || manual.has(key) || protectedKeys.has(key),
          );
        }
        rule.wanted = [...wanted];
        rule.last = Date.now();
        rule.blocked = ["metered","unknown"].includes(result?.blocked) ? result.blocked : null;
        rule.error = rule.blocked
          ? t(rule.blocked === "metered" ? "sync.waitMetered" : "sync.waitUnknown")
          : result?.cancelled
          ? ft("Остановлено")
          : result?.failed
            ? ft("Не загружено: {n}",{n:result.failed})
            : null;
      } catch (error) {
        rule.blocked = null;
        rule.error = error.kind
          ? ft("Ошибка: {kind}",{kind:error.kind})
          : String(error.message || ft("Не удалось синхронизировать"));
      }
      if (id !== accountId()) break;
      const rules = syncRules();
      if (rules.some((other) => other.key === rule.key))
        writePersonal(
          "playlistSync",
          rules.map((other) => (other.key === rule.key ? rule : other)),
        );
      publish("playlistSync");
    }
  } finally {
    running = false;
    await refreshDownloads();
    publish("playlistSync");
  }
}
export const syncRunning = () => running;
export function installPlaylistSync() {
  let disposed=false,checking=false;
  const resumeBlocked = async () => {
    if(disposed || checking || !isAndroid() || document.hidden || running || downloadJob()?.running)return;
    const account=accountId();
    if(!account || !syncRules().some(rule=>["metered","unknown"].includes(rule.blocked)))return;
    checking=true;
    try {
      // A local system query only: waiting on mobile data never polls VK.
      const network=await api.networkUsage();
      if(!disposed && account===accountId() && network?.connected===true && network?.metered===false)await syncPlaylists();
    } catch { /* Keep the existing pause reason; no background consent dialog or repeated toast. */ }
    finally {checking=false;}
  };
  const costTimer=isAndroid() ? setInterval(resumeBlocked,30_000) : null;
  window.addEventListener("focus",resumeBlocked);
  document.addEventListener("visibilitychange",resumeBlocked);
  const timer = setInterval(
    () => syncPlaylists().catch(() => {}),
    15 * 60 * 1000,
  );
  const online = () => syncPlaylists().catch(() => {});
  window.addEventListener("online", online);
  subscribe("session", () => {
    if (accountId()) online();
  });
  own(() => {
    disposed=true;
    clearInterval(timer);if(costTimer!==null)clearInterval(costTimer);
    window.removeEventListener("focus",resumeBlocked);
    document.removeEventListener("visibilitychange",resumeBlocked);
    window.removeEventListener("online", online);
  });
  online();
}
