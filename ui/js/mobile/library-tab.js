// The Library tab, after the iOS LibraryView: a row of sections (All, Tracks,
// Playlists, Albums) under a large title, a summary card for the downloads, shelves
// of playlists and albums, and the liked tracks themselves. The toolbar holds the
// iOS toolbar's three: downloads, history and a "+" menu (new playlist, friends).
//
// The track list is the existing "My music" screen, moved into the Tracks
// section: it keeps its filter, its play and shuffle buttons and its virtual list.

import { h } from "../dom.js";
import { icon } from "../icons.js";
import { errorText, t, tn } from "../i18n.js";
import { store, subscribe } from "../store.js";
import { ensurePlaylists, allPlaylists, forgetPlaylists } from "../my-playlists.js";
import { ensureAlbums, followedAlbums, forgetAlbums } from "../my-albums.js";
import { playlistCard } from "../views/playlists.js";
import { albumCard } from "../album-card.js";
import { openAlbum } from "../navigation.js";
import { downloads, formatSize } from "../downloads.js";
import { paintStatus } from "../ui-parts.js";
import { openMenu } from "../menu.js";
import { openActionSheet } from "./sheet.js";
import "./strings.js";

const SECTIONS = ["all", "tracks", "playlists", "albums"];

export function createLibraryTab({ libraryView, push, newPlaylist, toast }) {
  let section = "all";
  let loadingData = 0;

  const heading = h("h1", { text: t("tab.library") });
  const sections = h("div", { class: "lib-sections", role: "tablist", "aria-label": t("tab.library") },
    SECTIONS.map((id) => h("button", { type: "button", role: "tab", "data-section": id, onclick: () => select(id), text: t(`lib.${id}`) })));
  const header = h("header", { class: "view-header" }, h("div", { class: "vh-text" }, heading), sections);

  // ---- All
  const offlineCard = h("button", { class: "lib-summary", type: "button", onclick: () => push("downloads") },
    h("span", { class: "lib-summary-ico", "aria-hidden": "true" }, icon("arrow-down-circle", 22)),
    h("span", { class: "lib-summary-text" }, h("strong", { class: "lib-summary-title", text: t("lib.offline") }), h("span", { class: "lib-summary-detail tnum" })),
    h("span", { class: "chev" }, icon("chevronRight", 16)));
  const likedRow = h("button", { class: "lib-summary liked", type: "button", onclick: () => select("tracks") },
    h("span", { class: "lib-summary-ico", "aria-hidden": "true" }, icon("heart-on", 20)),
    h("span", { class: "lib-summary-text" }, h("strong", { class: "lib-summary-title", text: t("lib.liked") }), h("span", { class: "lib-summary-detail tnum" })),
    h("span", { class: "chev" }, icon("chevronRight", 16)));
  // Умные плейлисты (правила по локальной статистике): отдельный экран, открывается страницей поверх вкладки.
  const smartRow = h("button", { class: "lib-summary smart", type: "button", onclick: () => push("smart") },
    h("span", { class: "lib-summary-ico", "aria-hidden": "true" }, icon("sparkles", 20)),
    h("span", { class: "lib-summary-text" }, h("strong", { class: "lib-summary-title", text: t("nav.smart") }), h("span", { class: "lib-summary-detail", text: t("lib.smartNote") })),
    h("span", { class: "chev" }, icon("chevronRight", 16)));
  const playlistRow = h("div", { class: "release-row lib-row" });
  const albumRow = h("div", { class: "release-row lib-row" });
  const playlistHead = shelfHead(t("lib.playlists"), () => select("playlists"));
  const albumHead = shelfHead(t("lib.albums"), () => select("albums"));
  const dataStatus = h("div", { class: "status", role: "status", hidden: true });
  const allPane = h("div", { class: "lib-pane", "data-pane": "all" }, offlineCard, likedRow, smartRow, dataStatus,
    playlistHead.element, playlistRow, albumHead.element, albumRow);

  // ---- Tracks: the existing screen moves in here
  libraryView.element.classList.add("embedded");
  const tracksPane = h("div", { class: "lib-pane", "data-pane": "tracks", hidden: true });

  // ---- Playlists / Albums grids
  const playlistGrid = h("div", { class: "grid lib-grid" });
  const playlistsStatus = h("div", { class: "status", role: "status", hidden: true });
  const playlistsPane = h("div", { class: "lib-pane", "data-pane": "playlists", hidden: true },
    h("button", { class: "btn lib-new", type: "button", onclick: () => newPlaylist() }, icon("plus", 14), t("pl.new")), playlistsStatus, playlistGrid);
  const albumGrid = h("div", { class: "grid lib-grid" });
  const albumsStatus = h("div", { class: "status", role: "status", hidden: true });
  const albumsPane = h("div", { class: "lib-pane", "data-pane": "albums", hidden: true }, albumsStatus, albumGrid);

  const panes = { all: allPane, tracks: tracksPane, playlists: playlistsPane, albums: albumsPane };
  const element = h("section", { class: "view library-hub", hidden: true }, header, allPane, tracksPane, playlistsPane, albumsPane);

  function shelfHead(text, more) {
    const element = h("div", { class: "shelf-head" }, h("h2", { class: "shelf-title", text }),
      h("button", { class: "shelf-more", type: "button", onclick: more }, t("lib.see"), icon("chevronRight", 12)));
    return { element };
  }

  function select(id) {
    section = id;
    for (const button of sections.querySelectorAll("button")) button.setAttribute("aria-selected", String(button.dataset.section === id));
    for (const [key, pane] of Object.entries(panes)) pane.hidden = key !== id;
    if (id === "tracks") {
      tracksPane.append(libraryView.element);
      libraryView.element.hidden = false;
      libraryView.show?.();
    }
  }

  function renderData() {
    const playlists = allPlaylists();
    const albums = followedAlbums();
    playlistRow.replaceChildren(...playlists.map((p) => playlistCard(p, (playlist) => push("playlists", playlist))));
    albumRow.replaceChildren(...albums.map((a) => albumCard(a, openAlbum)));
    playlistHead.element.hidden = playlistRow.hidden = playlists.length === 0;
    albumHead.element.hidden = albumRow.hidden = albums.length === 0;
    playlistGrid.replaceChildren(...playlists.map((p) => playlistCard(p, (playlist) => push("playlists", playlist))));
    albumGrid.replaceChildren(...albums.map((a) => albumCard(a, openAlbum)));
    paintStatus(playlistsStatus, playlists.length || loadingData ? "" : t("lib.noPlaylists"), { art: "playlist", extra: "compact" });
    paintStatus(albumsStatus, albums.length || loadingData ? "" : t("lib.noAlbums"), { art: "album", extra: "compact" });
  }

  function renderSummary() {
    const { tracks, bytes } = downloads();
    offlineCard.querySelector(".lib-summary-detail").textContent = tracks.length
      ? `${tn("library.count", tracks.length)} · ${formatSize(bytes)}`
      : t("lib.offlineEmpty");
    const liked = libraryView.count?.() ?? 0;
    likedRow.querySelector(".lib-summary-detail").textContent = liked ? tn("library.count", liked) : "";
  }

  async function loadData() {
    loadingData++;
    paintStatus(dataStatus, "");
    try {
      await Promise.all([ensurePlaylists(), ensureAlbums()]);
    } catch (error) {
      paintStatus(dataStatus, errorText(error), { error: true, extra: "inline" });
    } finally {
      loadingData--;
      renderData();
    }
  }

  subscribe("playlists", renderData);
  subscribe("albums", renderData);
  subscribe("downloads", renderSummary);
  subscribe("likes", renderSummary);

  select("all");

  /** The three buttons of the iOS toolbar. */
  function navActions() {
    const button = (glyph, label, onclick) => h("button", { type: "button", class: "nav-btn", "aria-label": label, onclick }, icon(glyph, 22));
    return [
      button("arrow-down-circle", t("lib.offline"), () => push("downloads")),
      button("history", t("history.title"), () => push("history")),
      button("plus-circle", t("lib.actions"), () => openMenu({ x: 0, y: 0 }, [
        { label: t("pl.new"), action: "new-playlist", icon: "plus", run: () => newPlaylist() },
        { label: t("nav.smart"), action: "smart", icon: "sparkles", run: () => push("smart") },
        { label: t("nav.friends"), action: "friends", icon: "friends", run: () => push("friends") },
      ], { label: t("lib.actions") })),
    ];
  }

  return {
    element,
    navActions,
    show() {
      renderSummary();
      loadData();
      if (section === "tracks") libraryView.show?.();
    },
    /** Pull to refresh: everything this tab shows is asked for again. */
    async reload() {
      forgetPlaylists();
      forgetAlbums();
      await Promise.allSettled([loadData(), section === "tracks" ? libraryView.reload?.() : null]);
    },
    root() { select("all"); },
    /** On the phone "My music" is a section of this tab; beside the sidebar it is a page of the main column. */
    embed(on, main) {
      if (on) {
        tracksPane.append(libraryView.element);
        libraryView.element.hidden = false;
      } else {
        main.insertBefore(libraryView.element, main.querySelector(".player-dock"));
        libraryView.element.hidden = true;
      }
    },
    reset() { renderData(); renderSummary(); select("all"); },
    select,
  };
}
