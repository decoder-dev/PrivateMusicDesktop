import { h, clear, fold } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { createVirtualList } from "./virtual-list.js";
import { trackRow, ROW_HEIGHT } from "./track-row.js";
import { store, subscribe } from "./store.js";
import { trackKey } from "./api.js";
import { isLiked, isBusy } from "./likes.js";
import { isDownloaded, isDownloading } from "./downloads.js";

/**
 * The common screen for every list of tracks: header, optional filter,
 * status line and a virtual list.
 *
 * hooks:
 *   onPlay(visibleTracks, index, { shuffle })
 *   onAction(action, track, index)      hover buttons
 *   rowActions                           [[icon, action, label], …] or undefined for the default pair
 *   isSelected(track, index)             default: the playing track
 */
export function createTracksScreen({ title, filterable = false, playable = true, hooks }) {
  let all = [];
  let visible = [];
  let query = "";

  const heading = h("h1", { text: title });
  const meta = h("span", { class: "meta" });
  const filter = filterable
    ? h("input", {
        class: "field", type: "search", placeholder: t("library.filterShort"), "aria-label": t("library.filter"),
        oninput: (event) => { query = fold(event.target.value.trim()); applyFilter(); },
      })
    : null;
  const playButton = playable
    ? h("button", { class: "btn primary", type: "button", onclick: () => visible.length && hooks.onPlay(visible, 0, { shuffle: false }) }, t("play"))
    : null;
  const shuffleButton = playable
    ? h("button", { class: "btn", type: "button", onclick: () => visible.length && hooks.onPlay(visible, Math.floor(Math.random() * visible.length), { shuffle: true }) }, t("shuffle"))
    : null;
  const status = h("div", { class: "status", role: "status", hidden: true });
  const empty = h("div", { class: "empty-state", hidden: true });

  const list = createVirtualList({
    rowHeight: ROW_HEIGHT,
    renderRow: (track, index) =>
      trackRow(track, {
        selected: hooks.isSelected ? hooks.isSelected(track, index) : store.player.track && trackKey(store.player.track) === trackKey(track),
        actions: hooks.rowActions,
        liked: isLiked(track),
        busy: isBusy(track),
        pinLiked: hooks.pinLiked ?? true,
        downloaded: isDownloaded(track),
        downloading: isDownloading(track),
      }),
    onRowEvent: (type, event, track, index) => {
      const button = event.target.closest?.("[data-action]");
      if (type === "click" && button) {
        event.stopPropagation();
        hooks.onAction?.(button.dataset.action, track, index);
      } else if (type === "dblclick" && !button) {
        hooks.onPlay?.(visible, index, { shuffle: false });
      } else if (type === "keydown" && event.key === "Enter" && !button) {
        hooks.onPlay?.(visible, index, { shuffle: false });
      }
    },
  });

  const element = h("section", { class: "view" },
    h("header", { class: "view-header" }, hooks.back ?? null, heading, hooks.headerExtra ?? null, meta, filter, playButton, shuffleButton),
    status, empty, list.element);

  function applyFilter() {
    visible = query
      ? all.filter((track) => fold(track.title).includes(query) || fold(track.artist).includes(query))
      : all;
    list.setItems(visible);
    list.scrollToTop();
    renderEmpty();
  }

  function renderEmpty() {
    const show = visible.length === 0 && !status.textContent;
    empty.hidden = !show;
    if (show) empty.textContent = all.length === 0 ? (hooks.emptyText ?? "") : t("library.noMatches");
    for (const button of [playButton, shuffleButton]) if (button) button.disabled = visible.length === 0;
  }

  // Keep the "playing" highlight current without rebuilding anything else.
  let lastKey = null;
  subscribe("player", (player) => {
    const key = player.track ? trackKey(player.track) : null;
    if (key !== lastKey) {
      lastKey = key;
      list.refresh();
    }
  });

  subscribe("likes", () => list.refresh());
  subscribe("downloads", () => list.refresh());

  return {
    element,
    list,
    setTitle(text) { heading.textContent = text; },
    setTracks(tracks) {
      all = tracks;
      applyFilter();
      meta.textContent = tracks.length ? t("library.count", { n: tracks.length }) : "";
    },
    /** Shows progressive loading without throwing the list away. */
    setProgress(tracks, total) {
      all = tracks;
      visible = query ? all.filter((tr) => fold(tr.title).includes(query) || fold(tr.artist).includes(query)) : all;
      list.setItems(visible);
      status.hidden = false;
      status.className = "status";
      status.textContent = t("library.loaded", { n: tracks.length, total });
      renderEmpty();
    },
    setStatus(text, { error = false } = {}) {
      status.textContent = text ?? "";
      status.hidden = !text;
      status.className = `status${error ? " error" : ""}`;
      renderEmpty();
    },
    clearStatus() {
      status.textContent = "";
      status.hidden = true;
      renderEmpty();
    },
    get visible() { return visible; },
    refresh: () => list.refresh(),
  };
}

export { clear };
