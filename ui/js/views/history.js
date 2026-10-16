import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { trackKey } from "../api.js";
import { store, subscribe } from "../store.js";
import { confirmDialog } from "../dialog.js";

/**
 * What was played on this computer, newest first. A track lands here once
 * 30 % of it has really been heard (the engine decides); the list lives in a
 * small file on this computer and can be cleared.
 */
export function createHistoryView({ play, onTrackAction, toast, openTrackMenu }) {
  let tracks = [];
  let generation = 0;
  let visible = false;

  const clearButton = h("button", { class: "btn danger-text", type: "button", onclick: () => clearAll(), text: t("history.clear"), disabled: true });
  const screen = createTracksScreen({
    title: t("history.title"),
    filterable: true,
    hooks: {
      emptyText: t("history.empty"),
      onPlay: play,
      onAction: onTrackAction,
      headerExtra: clearButton,
      onMenu: (track, at) => openTrackMenu(track, at, [
        { label: t("history.remove"), action: "remove-from-history", icon: "trash", destructive: true, run: () => remove(track) },
      ]),
    },
  });
  screen.header.after(h("div", { class: "page-note footer-note", text: t("history.privacy") }));

  async function load() {
    const mine = ++generation;
    try {
      const entries = await api.history();
      if (mine !== generation) return;
      tracks = entries.map((entry) => entry.track);
      screen.clearStatus();
      screen.setTracks(tracks);
      clearButton.disabled = tracks.length === 0;
    } catch (error) {
      if (mine === generation) screen.setStatus(errorText(error), { error: true });
    }
  }

  async function remove(track) {
    try {
      await api.historyRemove(trackKey(track));
      tracks = tracks.filter((other) => trackKey(other) !== trackKey(track));
      screen.setTracks(tracks);
      clearButton.disabled = tracks.length === 0;
      toast(t("history.removed"));
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function clearAll() {
    if (!(await confirmDialog({ text: t("history.clearConfirm"), confirm: t("history.clear"), danger: true }))) return;
    try {
      await api.historyClear();
      tracks = [];
      screen.setTracks(tracks);
      clearButton.disabled = true;
      toast(t("history.cleared"));
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  // A play is counted a while after the track starts, so a track that has
  // just finished its first third shows up when the next one begins.
  let lastTrack = null;
  subscribe("player", (player) => {
    const key = player.track ? trackKey(player.track) : null;
    if (key === lastTrack) return;
    lastTrack = key;
    if (visible) load();
  });
  subscribe("view", (view) => { visible = view === "history"; });

  return {
    element: screen.element,
    show() {
      visible = true;
      load();
    },
    reset() {
      generation++;
      tracks = [];
      screen.setTracks([]);
      screen.clearStatus();
      clearButton.disabled = true;
    },
  };
}
