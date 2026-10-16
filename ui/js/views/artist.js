import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { ROW_HEIGHT } from "../track-row.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { albumCard } from "../album-card.js";
import { openAlbum } from "../navigation.js";

const TRACKS = 50;
const ALBUMS = 30;
// Popular tracks show this many rows; the rest scroll inside the list.
const VISIBLE_ROWS = 8;

/**
 * An artist: photo and name, popular tracks (playable, the visible list is
 * the queue) and the albums grid. Opened with `{ id, name }`; without an id
 * the artist is looked up by name, and if VK knows no such artist the page
 * shows plain search results for the name, as the iOS app does.
 */
export function createArtistView({ play, onTrackAction, toast, back }) {
  let current = null; // { id, name } as opened
  let loadedKey = null;
  let generation = 0;

  const photo = h("div", { class: "page-art round" });
  const name = h("h1");
  const note = h("div", { class: "page-note", hidden: true });
  const backButton = h("button", { class: "icon-btn", type: "button", title: t("playlists.back"), "aria-label": t("playlists.back"), onclick: () => back() }, icon("back", 20));
  const head = h("header", { class: "view-header page-head" }, backButton, photo, h("div", { class: "page-info" }, name, note));

  const tracks = createTracksScreen({
    title: t("artist.popular"),
    heading: "h2",
    hooks: { emptyText: t("artist.noTracks"), onPlay: play, onAction: onTrackAction },
  });
  tracks.element.classList.add("artist-tracks");

  const albumsStatus = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "grid artist-albums" });
  const albumsHeading = h("h2", { class: "shelf-title", text: t("artist.albums") });
  const scroller = h("div", { class: "page-scroll" }, tracks.element, albumsHeading, albumsStatus, grid);
  const element = h("section", { class: "view artist-view" }, head, scroller);

  function fitTracks(count) {
    tracks.list.element.style.height = `${Math.min(Math.max(count, 0), VISIBLE_ROWS) * ROW_HEIGHT}px`;
  }

  function renderHeader(artist) {
    name.textContent = artist.name;
    photo.replaceChildren(artist.photo_url
      ? h("img", { src: artist.photo_url, alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
      : icon("artist", 40));
  }

  function setAlbumsStatus(text, { error = false } = {}) {
    albumsStatus.textContent = text ?? "";
    albumsStatus.hidden = !text;
    albumsStatus.className = `status${error ? " error" : ""}`;
  }

  async function loadTracks(id, query, mine) {
    tracks.setTracks([]);
    fitTracks(0);
    tracks.setStatus(t("loading"));
    try {
      const page = id ? await api.artistTracks(id, 0, TRACKS) : await api.search(query, 0, TRACKS);
      if (mine !== generation) return;
      tracks.clearStatus();
      tracks.setTracks(page.items);
      fitTracks(page.items.length);
    } catch (error) {
      if (mine !== generation) return;
      loadedKey = null; // let the next visit try again
      tracks.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function loadAlbums(id, query, mine) {
    grid.replaceChildren();
    setAlbumsStatus(t("loading"));
    try {
      let albums;
      if (id) {
        const result = await api.artistAlbums(id, 0, ALBUMS);
        if (mine !== generation) return;
        // The catalogue names the artist with a photo; keep the name the
        // user clicked if it gave none.
        if (result.artist) renderHeader({ ...result.artist, name: result.artist.name || current.name });
        albums = result.albums.items;
      } else {
        albums = (await api.searchAlbums(query, 0, ALBUMS)).items;
        if (mine !== generation) return;
      }
      grid.replaceChildren(...albums.map((album) => albumCard(album, openAlbum, { showArtist: !id })));
      setAlbumsStatus(albums.length ? "" : t("artist.noAlbums"));
    } catch (error) {
      if (mine !== generation) return;
      loadedKey = null;
      setAlbumsStatus(errorText(error), { error: true });
    }
  }

  async function open(artist) {
    const key = artist.id ? `id:${artist.id}` : `name:${artist.name}`;
    if (key === loadedKey) return; // back to the page as it was
    loadedKey = key;
    current = artist;
    const mine = ++generation;
    renderHeader(artist);
    note.hidden = true;
    scroller.scrollTop = 0;
    let id = artist.id;
    if (!id) {
      tracks.setTracks([]);
      tracks.setStatus(t("loading"));
      setAlbumsStatus(t("loading"));
      grid.replaceChildren();
      try {
        const found = await api.findArtist(artist.name);
        if (mine !== generation) return;
        if (found) {
          id = found.id;
          renderHeader(found);
        } else {
          note.textContent = t("artist.notFound");
          note.hidden = false;
        }
      } catch (error) {
        if (mine !== generation) return;
        loadedKey = null; // let the next visit try again
        tracks.setStatus(errorText(error), { error: true });
        setAlbumsStatus("");
        toast(errorText(error), { error: true });
        return;
      }
    }
    await Promise.all([loadTracks(id, artist.name, mine), loadAlbums(id, artist.name, mine)]);
  }

  return {
    element,
    open,
    reset() {
      generation++;
      loadedKey = null;
      current = null;
      tracks.setTracks([]);
      tracks.clearStatus();
      grid.replaceChildren();
      setAlbumsStatus("");
    },
  };
}
