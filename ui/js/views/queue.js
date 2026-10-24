import { playbackAction } from "../playback-action.js";
import { ft } from "../feature-i18n.js";
import { api } from "../api.js";
import { h } from "../dom.js";
import { createTracksScreen } from "../tracks-screen.js";
import { t, addStrings } from "../i18n.js";
import { store, subscribe } from "../store.js";
import { queueHistory, rememberQueue, restoreQueue } from "../queue-history.js";
import { toast } from "../toast.js";
import { openMenu } from "../menu.js";
import { errorText } from "../i18n.js";
import { removeSelected } from "../batch-remove.js";
import { trackKey } from "../api.js";
import { accountId } from "../personal.js";

addStrings({ ru: { "queue.clearNext": "Очистить следующие", "queue.up": "Выше в очереди", "queue.down": "Ниже в очереди" }, en: { "queue.clearNext": "Clear up next", "queue.up": "Move up", "queue.down": "Move down" } });
addStrings({ru:{"queue.history":"Предыдущие очереди","queue.undo":"Восстановить"},en:{"queue.history":"Previous queues","queue.undo":"Restore"}});
addStrings({ru:{"queue.changed":"Очередь изменена"},en:{"queue.changed":"Queue changed"}});

export function createQueueView() {
  async function undoable(run) {
    const id=accountId();
    try {
      const at=rememberQueue(); await run();
      if (at && id===accountId()) toast(t("queue.changed"),{action:{label:t("queue.undo"),run:()=>id===accountId() && restoreQueue(at).catch(error=>toast(errorText(error),{error:true}))}});
    } catch(error) { toast(errorText(error),{error:true}); }
  }
  const clear = h("button", { class: "btn", type: "button", text: t("queue.clearNext"), onclick: () => undoable(()=>api.clearUpcoming()) });
  const history = h("button",{class:"btn",type:"button",text:t("queue.history"),onclick:event=>{
    const box=event.currentTarget.getBoundingClientRect();
    const entries=queueHistory();
    openMenu({x:box.left,y:box.bottom+4},entries.map(entry=>({label:`${new Date(entry.at).toLocaleString()} · ${entry.tracks[entry.index]?.title} · ${entry.tracks.length}`,action:"restore-queue",run:()=>restoreQueue(entry.at).catch(error=>toast(errorText(error),{error:true}))})),{label:t("queue.history")});
  }});
  const renderHistory=()=>{history.disabled=queueHistory().length===0;};
  subscribe("queueHistory",renderHistory);subscribe("session",renderHistory);renderHistory();
  const screen = createTracksScreen({
    title: t("queue.title"),
    playable: false,
    hooks: {
      emptyText: t("queue.empty"), art: "queue",
      onBatchRemove:items=>undoable(()=>removeSelected(items,async track=>{const index=store.queue.tracks.findIndex(other=>trackKey(other)===trackKey(track));if(index>=0)await api.removeFromQueue(index);},ft("Удалить выбранное из очереди?"))),
      pinLiked: false, // сердечки показываются при наведении; столбец отметок в каждой строке ни о чем не говорит
      headerExtra: h("div",{class:"set-inline"},history,clear),
      rowActions: [["move-up", "up", t("queue.up")], ["move-down", "down", t("queue.down")], ["close", "remove", t("remove")]],
      isSelected: (_track, index) => index === store.queue.current,
      onPlay: (_tracks, index) => playbackAction(() => api.jump(index)),
      onAction: (action, _track, index) => {
        if (action === "remove") undoable(()=>api.removeFromQueue(index));
        else if (action === "up" && index > (store.queue.current ?? -1) + 1) playbackAction(() => api.moveUpcoming(index, index - 1));
        else if (action === "down" && index > (store.queue.current ?? -1) && index + 1 < store.queue.tracks.length) playbackAction(() => api.moveUpcoming(index, index + 1));
      },
    },
  });

  // Очередь, запущенная из микса, говорит об этом: она продолжает пополняться во время воспроизведения.
  const note = h("div", { class: "radio-note", role: "note", hidden: true });
  screen.element.insertBefore(note, screen.list.element);
  const noteKeys = { personal: "queue.radio.personal", catalog: "queue.radio.catalog", similar: "queue.radio.similar" };
  function renderNote(player) {
    const key = noteKeys[player.radio];
    note.textContent = key ? t(key) : "";
    note.hidden = !key;
    clear.disabled = player.queue_len <= (player.queue_index ?? -1) + 1;
  }
  subscribe("player", renderNote);
  renderNote(store.player);

  // Очередь может состоять из тысяч треков и изменяться при каждом пропуске, поэтому она рисуется
  // только пока находится на экране.
  let visible = false;
  let dirty = true;

  function render() {
    dirty = false;
    screen.setTracks(store.queue.tracks);
  }

  subscribe("queue", () => {
    if (visible) render();
    else dirty = true;
  });
  subscribe("view", (view) => {
    visible = view === "queue";
    if (visible && dirty) render();
  });
  return {
    element: screen.element,
    show() {
      visible = true;
      if (dirty) render();
    },
    reset() {
      dirty = true;
      note.hidden = true;
      screen.setTracks([]);
    },
  };
}
