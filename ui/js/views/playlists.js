import { api, loadAllPages } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";

function card(playlist, open) {
  const art = playlist.artwork_url
    ? h("img", { class: "art", src: playlist.artwork_url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
    : h("div", { class: "art empty" }, icon("playlist", 36));
  return h("button", { class: "card", type: "button", onclick: () => open(playlist) },
    art,
    h("div", { class: "name", text: playlist.title }),
    h("div", { class: "count", text: t("playlists.tracks", { n: playlist.count }) }));
}

export function createPlaylistsView({ play, onTrackAction, toast }) {
  let loaded = false;
  let loading = false;
  let generation = 0;
  let detailGeneration = 0;

  const status = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "grid" });
  const header = h("header", { class: "view-header" }, h("h1", { text: t("playlists.title") }));
  const listScreen = h("section", { class: "view" }, header, status, grid);

  const detail = createTracksScreen({
    title: "",
    hooks: {
      emptyText: t("library.empty"),
      onPlay: play,
      onAction: onTrackAction,
      back: h("button", { class: "icon-btn", type: "button", title: t("playlists.back"), "aria-label": t("playlists.back"), onclick: () => showList() }, icon("back", 20)),
    },
  });
  detail.element.hidden = true;

  const element = h("div", { class: "view" }, listScreen, detail.element);

  function showList() {
    detailGeneration++;
    detail.element.hidden = true;
    listScreen.hidden = false;
  }

  async function open(playlist) {
    listScreen.hidden = true;
    detail.element.hidden = false;
    detail.setTitle(playlist.title);
    detail.setTracks([]);
    detail.setStatus(t("loading"));
    const mine = ++detailGeneration;
    try {
      const tracks = await loadAllPages((offset, count) => api.playlistTracks(playlist, offset, count), {
        isCancelled: () => mine !== detailGeneration,
        onProgress: (n, total, items) => {
          if (mine === detailGeneration) detail.setProgress(items.slice(), total);
        },
      });
      if (tracks === null) return;
      detail.clearStatus();
      detail.setTracks(tracks);
    } catch (error) {
      if (mine === detailGeneration) detail.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function load() {
    if (loading) return;
    loading = true;
    const mine = ++generation;
    status.hidden = false;
    status.className = "status";
    status.textContent = t("loading");
    try {
      const playlists = await loadAllPages((offset, count) => api.playlists(offset, count), {
        pageSize: 100,
        isCancelled: () => mine !== generation,
      });
      if (playlists === null) return;
      loaded = true;
      grid.replaceChildren(...playlists.map((p) => card(p, open)));
      status.hidden = playlists.length > 0;
      status.textContent = playlists.length ? "" : t("playlists.empty");
    } catch (error) {
      if (mine === generation) {
        status.className = "status error";
        status.textContent = errorText(error);
      }
      toast(errorText(error), { error: true });
    } finally {
      if (mine === generation) loading = false;
    }
  }

  return {
    element,
    show() {
      if (!loaded) load();
    },
    reset() {
      generation++;
      detailGeneration++;
      loaded = false;
      loading = false;
      grid.replaceChildren();
      showList();
    },
  };
}
