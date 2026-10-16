import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t, tn } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { subscribe } from "../store.js";
import { formDialog, confirmDialog } from "../dialog.js";
import { loadAllPages } from "../api.js";
import { allPlaylists, ensurePlaylists, isOwn, replacePlaylist, dropPlaylist, changeCount, same, forgetPlaylists } from "../my-playlists.js";
import { ensureAlbums, followedAlbums } from "../my-albums.js";
import { albumCard } from "../album-card.js";
import { openAlbum } from "../navigation.js";
import { artBox, paintStatus, backButton, headerOnScroll } from "../ui-parts.js";
import { enter } from "../motion.js";

export function playlistCard(playlist, open) {
  const art = h("span", { class: "card-art art" }, artBox(playlist.artwork_url, { glyph: "playlist", size: 36, className: "fill" }), h("span", { class: "play-chip" }, icon("play", 12)));
  return h("button", { class: "card", type: "button", onclick: () => open(playlist) },
    art,
    h("div", { class: "name", text: playlist.title }),
    h("div", { class: "count", text: tn("playlists.tracks", playlist.count) }));
}

export function createPlaylistsView({ play, onTrackAction, toast, openTrackMenu, newPlaylist }) {
  let loaded = false;
  let generation = 0;
  let detailGeneration = 0;
  let current = null; // the open playlist
  let tracks = [];

  const status = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "grid" });
  const header = h("header", { class: "view-header" }, h("div", { class: "vh-text" }, h("h1", { text: t("playlists.title") })),
    h("button", { class: "btn primary", type: "button", onclick: () => newPlaylist() }, icon("plus", 14), t("pl.new")));
  // Followed releases, below the playlists.
  const albumsStatus = h("div", { class: "status", role: "status", hidden: true });
  const albumsGrid = h("div", { class: "grid followed-albums" });
  const shelves = h("div", { class: "page-scroll" }, status, grid,
    h("h2", { class: "shelf-title", text: t("albums.title") }), albumsStatus, albumsGrid);
  const listScreen = h("section", { class: "view" }, header, shelves);
  headerOnScroll(header, shelves);

  const editButton = h("button", { class: "btn", type: "button", onclick: () => edit(), text: t("pl.edit") });
  const deleteButton = h("button", { class: "btn danger-text", type: "button", onclick: () => remove(), text: t("pl.delete") });
  const detail = createTracksScreen({
    title: "",
    hero: true,
    kicker: t("playlists.kicker"),
    hooks: {
      emptyText: t("library.empty"),
      onPlay: play,
      onAction: onTrackAction,
      onMenu: (track, at, index) => openTrackMenu(track, at, current && isOwn(current)
        ? [{ label: t("menu.removeFrom"), action: "remove-from-playlist", icon: "trash", destructive: true, run: () => removeTrack(track, index) }]
        : []),
      back: backButton(t("playlists.back"), () => showList()),
      headerExtra: h("div", { class: "set-inline pl-owner" }, editButton, deleteButton),
    },
  });
  detail.element.hidden = true;

  const element = h("div", { class: "view" }, listScreen, detail.element);

  function renderGrid() {
    const list = allPlaylists();
    grid.replaceChildren(...list.map((p) => playlistCard(p, open)));
    if (loaded) {
      paintStatus(status, list.length ? "" : t("playlists.empty"));
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
      if (fresh) { current = fresh; detail.setTitle(fresh.title); detail.setNote(fresh.description ?? ""); }
    }
  });
  subscribe("session", renderOwner);

  let albumsLoaded = false;
  function renderAlbums() {
    if (!albumsLoaded) return;
    const list = followedAlbums();
    albumsGrid.replaceChildren(...list.map((album) => albumCard(album, openAlbum)));
    paintStatus(albumsStatus, list.length ? "" : t("albums.empty"), { extra: "compact" });
  }
  subscribe("albums", renderAlbums);

  async function loadAlbums() {
    const mine = generation;
    paintStatus(albumsStatus, t("loading"), { kind: "cards" });
    try {
      await ensureAlbums();
      if (mine !== generation) return;
      albumsLoaded = true;
      renderAlbums();
    } catch (error) {
      if (mine !== generation) return;
      paintStatus(albumsStatus, errorText(error), { error: true, extra: "compact" });
    }
  }

  function showList() {
    detailGeneration++;
    current = null;
    detail.element.hidden = true;
    listScreen.hidden = false;
    enter(listScreen);
  }

  async function open(playlist) {
    current = playlist;
    renderOwner();
    listScreen.hidden = true;
    detail.element.hidden = false;
    enter(detail.element);
    detail.setTitle(playlist.title);
    detail.setCover(playlist.artwork_url);
    detail.setNote(playlist.description ?? "");
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
      toast(t("pl.removed", { title: playlist.title }), {
        action: { label: t("toast.undo"), run: () => undoRemove(playlist, track, index) },
      });
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  /** The undo banner: puts the track back (VK may append it) and says so. */
  async function undoRemove(playlist, track, index) {
    try {
      await api.addToPlaylist(playlist, track);
      changeCount(playlist, 1);
      if (playlist === current || (current && same(playlist, current))) {
        tracks = [...tracks.slice(0, index), track, ...tracks.slice(index)];
        detail.setTracks(tracks);
      }
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function load() {
    const mine = ++generation;
    paintStatus(status, t("loading"), { kind: "cards" });
    try {
      await ensurePlaylists();
      if (mine !== generation) return;
      loaded = true;
      renderGrid();
    } catch (error) {
      if (mine === generation) {
        paintStatus(status, errorText(error), { error: true });
      }
      toast(errorText(error), { error: true });
    }
  }

  return {
    element,
    show() {
      if (!loaded) load();
      if (!albumsLoaded) loadAlbums();
    },
    /** A sidebar click while a playlist is open goes back to the list. */
    root() { if (current) showList(); },
    reset() {
      generation++;
      loaded = false;
      albumsLoaded = false;
      albumsGrid.replaceChildren();
      forgetPlaylists();
      grid.replaceChildren();
      showList();
    },
  };
}
