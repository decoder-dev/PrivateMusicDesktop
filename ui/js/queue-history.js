import { radioSnapshot } from "./radio-snapshot.js";
import { ft } from "./feature-i18n.js";
import { api } from "./api.js";
import { store, publish } from "./store.js";
import { toast } from "./toast.js";
import {
  readPersonal,
  writePersonal,
  cleanTrack,
  accountId,
} from "./personal.js";

export function queueHistory() {
  const saved = readPersonal("queues", []);
  return Array.isArray(saved)
    ? saved
        .filter(
          (q) =>
            Array.isArray(q.tracks) &&
            q.tracks.length &&
            Number.isInteger(q.index) &&
            q.index >= 0 &&
            q.index < q.tracks.length,
        )
        .slice(0, 10)
    : [];
}
export function rememberQueue() {
  const { tracks, current } = store.queue;
  if (!tracks.length || current == null || !accountId()) return;
  const snapshot = {
    at: Math.max(Date.now(), (queueHistory()[0]?.at || 0) + 1),
    tracks: tracks.map(cleanTrack),
    index: current,
    position: store.player.position || 0,
    shuffle: store.player.shuffle,
    repeat: store.player.repeat,
    radio: radioSnapshot(store.player.radio_context),
  };
  let entries=[snapshot,...queueHistory()].slice(0,10);
  for(;;){try{writePersonal("queues",entries);break;}catch{if(entries.length>1){entries.pop();continue;}toast(ft("История очереди не сохранена: локальное хранилище заполнено"),{error:true});return;}}
  publish("queueHistory");
  return snapshot.at;
}
export async function restoreQueue(at) {
  const id = accountId();
  const history = queueHistory();
  const entry = history.find((q) => q.at === at);
  if (!entry) return;
  // Восстановленная очередь остаётся на паузе, как при обычном перезапуске приложения.
  await api.setShuffle(!!entry.shuffle);
  if (id !== accountId()) return;
  await api.setRepeat(
    ["off", "all", "one"].includes(entry.repeat) ? entry.repeat : "off",
  );
  if (id !== accountId()) return;
  const radio=radioSnapshot(entry.radio);
  if(radio)await api.restoreRadio(radio,entry.tracks,entry.index,entry.position);
  else await api.loadQueue(entry.tracks, entry.index, entry.position);
  if (id !== accountId()) return;
  writePersonal(
    "queues",
    history.filter((q) => q.at !== at),
  );
  publish("queueHistory");
}
