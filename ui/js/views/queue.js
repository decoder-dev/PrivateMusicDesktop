import { api } from "../api.js";
import { h } from "../dom.js";
import { createTracksScreen } from "../tracks-screen.js";
import { t, addStrings } from "../i18n.js";
import { store, subscribe } from "../store.js";

addStrings({ ru: { "queue.clearNext": "Очистить следующие", "queue.up": "Выше в очереди", "queue.down": "Ниже в очереди" }, en: { "queue.clearNext": "Clear up next", "queue.up": "Move up", "queue.down": "Move down" } });

export function createQueueView() {
  const clear = h("button", { class: "btn", type: "button", text: t("queue.clearNext"), onclick: () => api.clearUpcoming() });
  const screen = createTracksScreen({
    title: t("queue.title"),
    playable: false,
    hooks: {
      emptyText: t("queue.empty"), art: "queue",
      pinLiked: false, // сердечки показываются при наведении; столбец отметок в каждой строке ни о чем не говорит
      headerExtra: clear,
      rowActions: [["move-up", "up", t("queue.up")], ["move-down", "down", t("queue.down")], ["close", "remove", t("remove")]],
      isSelected: (_track, index) => index === store.queue.current,
      onPlay: (_tracks, index) => api.jump(index),
      onAction: (action, _track, index) => {
        if (action === "remove") api.removeFromQueue(index);
        else if (action === "up" && index > (store.queue.current ?? -1) + 1) api.moveUpcoming(index, index - 1);
        else if (action === "down" && index > (store.queue.current ?? -1) && index + 1 < store.queue.tracks.length) api.moveUpcoming(index, index + 1);
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
