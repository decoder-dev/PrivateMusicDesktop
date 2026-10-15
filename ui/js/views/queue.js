import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { t } from "../i18n.js";
import { store, subscribe } from "../store.js";

export function createQueueView() {
  const screen = createTracksScreen({
    title: t("queue.title"),
    playable: false,
    hooks: {
      emptyText: t("queue.empty"),
      rowActions: [["close", "remove", t("remove")]],
      isSelected: (_track, index) => index === store.queue.current,
      onPlay: (_tracks, index) => api.jump(index),
      onAction: (action, _track, index) => {
        if (action === "remove") api.removeFromQueue(index);
      },
    },
  });

  // The queue can be thousands of tracks long and changes on every skip, so
  // it is only drawn while it is on screen.
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
      screen.setTracks([]);
    },
  };
}
