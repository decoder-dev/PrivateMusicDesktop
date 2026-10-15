import { h } from "../dom.js";
import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";

const SOURCES = {
  mix: { label: "fy.mix", load: () => api.mix() },
  recs: { label: "fy.recs", load: () => api.recommendations() },
};

/** VK's personal mix and recommendations, one tab each, loaded on first look. */
export function createForYouView({ play, onTrackAction, toast }) {
  let current = "mix";
  const cache = new Map(); // source -> tracks
  let generation = 0;

  const tabs = h("div", { class: "tabs", role: "tablist" },
    Object.entries(SOURCES).map(([id, source]) =>
      h("button", { type: "button", role: "tab", "data-source": id, onclick: () => select(id), text: t(source.label) })));
  const refresh = h("button", { class: "btn", type: "button", onclick: () => load(current, { force: true }), text: t("fy.refresh") });

  const screen = createTracksScreen({
    title: t("fy.title"),
    hooks: {
      emptyText: t("fy.empty"),
      onPlay: play,
      onAction: onTrackAction,
      headerExtra: h("div", { class: "set-inline" }, tabs, refresh),
    },
  });

  function renderTabs() {
    for (const button of tabs.children) button.setAttribute("aria-selected", String(button.dataset.source === current));
  }

  async function load(id, { force = false } = {}) {
    if (!force && cache.has(id)) {
      screen.clearStatus();
      screen.setTracks(cache.get(id));
      return;
    }
    const mine = ++generation;
    screen.setTracks([]);
    screen.setStatus(t("loading"));
    refresh.disabled = true;
    try {
      const tracks = await SOURCES[id].load();
      cache.set(id, tracks);
      if (mine !== generation) return;
      screen.clearStatus();
      screen.setTracks(tracks);
    } catch (error) {
      if (mine !== generation) return;
      screen.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    } finally {
      if (mine === generation) refresh.disabled = false;
    }
  }

  function select(id) {
    current = id;
    renderTabs();
    load(id);
  }

  renderTabs();
  return {
    element: screen.element,
    show() { if (!cache.has(current)) load(current); },
    reset() { generation += 1; cache.clear(); screen.setTracks([]); screen.clearStatus(); },
  };
}
