import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { subscribe } from "../store.js";
import { formDialog, confirmDialog } from "../dialog.js";
import { loadAllPages } from "../api.js";
import { allPlaylists, ensurePlaylists, isOwn, replacePlaylist, dropPlaylist, changeCount, same, forgetPlaylists } from "../my-playlists.js";

function card(playlist, open) {
  const art = playlist.artwork_url
    ? h("img", { class: "art", src: playlist.artwork_url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
    : h("div", { class: "art empty" }, icon("playlist", 36));
  return h("button", { class: "card", type: "button", onclick: () => open(playlist) },
    art,
    h("div", { class: "name", text: playlist.title }),
    h("div", { class: "count", text: t("playlists.tracks", { n: playlist.count }) }));
}

export function createPlaylistsView({ play, onTrackAction, toast, openTrackMenu, newPlaylist }) {
  let loaded = false;
  let generation = 0;
  let detailGeneration = 0;
  let current = null; // the open playlist
  let tracks = [];

  const status = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "grid" });
  const header = h("header", { class: "view-header" }, h("h1", { text: t("playlists.title") }),
    h("button", { class: "btn primary", type: "button", onclick: () => newPlaylist(), text: t("pl.new") }));
  const listScreen = h("section", { class: "view" }, header, status, grid);

  const editButton = h("button", { class: "btn", type: "button", onclick: () => edit(), text: t("pl.edit") });
  const deleteButton = h("button", { class: "btn danger-text", type: "button", onclick: () => remove(), text: t("pl.delete") });
  const detail = createTracksScreen({
    title: "",
    hooks: {
      emptyText: t("library.empty"),
      onPlay: play,
      onAction: onTrackAction,
      onMenu: (track, at, index) => openTrackMenu(track, at, current && isOwn(current)
        ? [{ label: t("menu.removeFrom"), action: "remove-from-playlist", icon: "trash", run: () => removeTrack(track, index) }]
        : []),
      back: h("button", { class: "icon-btn", type: "button", title: t("playlists.back"), "aria-label": t("playlists.back"), onclick: () => showList() }, icon("back", 20)),
      headerExtra: h("div", { class: "set-inline pl-owner" }, editButton, deleteButton),
    },
  });
  detail.element.hidden = true;

  const element = h("div", { class: "view" }, listScreen, detail.element);

  function renderGrid() {
    const list = allPlaylists();
    grid.replaceChildren(...list.map((p) => card(p, open)));
    if (loaded) {
      status.hidden = list.length > 0;
      status.className = "status";
      status.textContent = list.length ? "" : t("playlists.empty");
    }
  }

  function renderOwner() {
    const own = current && isOwn(current);
    editButton.hidden = deleteButton.hidden = !own;
  }

  subscribe("playlists", () => {
    renderGrid();
    if (current) {
      const fresh = allPlaylists().find((p) => same(p, current));
      if (fresh) { current = fresh; detail.setTitle(fresh.title); }
    }
  });
  subscribe("session", renderOwner);

  function showList() {
    detailGeneration++;
    current = null;
    detail.element.hidden = true;
    listScreen.hidden = false;
  }

  async function open(playlist) {
    current = playlist;
    renderOwner();
    listScreen.hidden = true;
    detail.element.hidden = false;
    detail.setTitle(playlist.title);
    detail.setTracks([]);
    detail.setStatus(t("loading"));
    const mine = ++detailGeneration;
    try {
      const all = await loadAllPages((offset, count) => api.playlistTracks(playlist, offset, count), {
        isCancelled: () => mine !== detailGeneration,
        onProgress: (n, total, items) => {
          if (mine === detailGeneration) detail.setProgress(items.slice(), total);
        },
      });
      if (all === null) return;
      tracks = all;
      detail.clearStatus();
      detail.setTracks(all);
    } catch (error) {
      if (mine === detailGeneration) detail.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function edit() {
    const playlist = current;
    if (!playlist) return;
    const values = await formDialog({
      title: t("pl.edit"),
      confirm: t("pl.save"),
      fields: [
        { name: "title", label: t("pl.name"), value: playlist.title, required: true, max: 128 },
        { name: "description", label: t("pl.description"), value: playlist.description ?? "", multiline: true, max: 2000 },
      ],
    });
    if (!values) return;
    try {
      replacePlaylist(await api.editPlaylist(playlist, values.title, values.description));
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function remove() {
    const playlist = current;
    if (!playlist) return;
    if (!(await confirmDialog({ text: t("pl.deleteConfirm", { title: playlist.title }), confirm: t("pl.delete"), danger: true }))) return;
    try {
      await api.deletePlaylist(playlist);
      dropPlaylist(playlist);
      showList();
      toast(t("pl.deleted"));
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function removeTrack(track, index) {
    const playlist = current;
    try {
      await api.removeFromPlaylist(playlist, track);
      if (playlist !== current) return;
      tracks = tracks.filter((_, i) => i !== index);
      detail.setTracks(tracks);
      changeCount(playlist, -1);
      toast(t("pl.removed", { title: playlist.title }));
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function load() {
    const mine = ++generation;
    status.hidden = false;
    status.className = "status";
    status.textContent = t("loading");
    try {
      await ensurePlaylists();
      if (mine !== generation) return;
      loaded = true;
      renderGrid();
    } catch (error) {
      if (mine === generation) {
        status.className = "status error";
        status.textContent = errorText(error);
      }
      toast(errorText(error), { error: true });
    }
  }

  return {
    element,
    show() {
      if (!loaded) load();
    },
    reset() {
      generation++;
      loaded = false;
      forgetPlaylists();
      grid.replaceChildren();
      showList();
    },
  };
}
