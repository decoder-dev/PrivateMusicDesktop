import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { ROW_HEIGHT } from "../track-row.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { albumCard } from "../album-card.js";
import { artBox, paintStatus, backButton, pageHero, headerOnScroll, setAmbience } from "../ui-parts.js";
import { openAlbum } from "../navigation.js";

const TRACKS = 50;
const ALBUMS = 30;
// В популярных треках показано такое количество строк; остальные прокручиваются внутри списка.
const VISIBLE_ROWS = 8;

/**
 * Исполнитель: фото и имя, популярные треки (воспроизводимые, видимый список — очередь) и сетка
 * альбомов. Открыт с помощью `{ id, name }`; без идентификатора артист ищется по имени, и если VK не
 * ​​знает такого исполнителя, на странице отображаются простые результаты поиска по имени, как это
 * делает приложение iOS.
 */
export function createArtistView({ play, onTrackAction, toast, back }) {
  let current = null; // { id, name } as opened
  let loadedKey = null;
  let generation = 0;
  let trackNextOffset = null;
  let albumNextOffset = null;
  let trackItems = [];
  let loadingTracks = false;
  let loadingAlbums = false;

  const photo = h("span", { class: "hero-art page-art round" });
  const name = h("h1");
  const note = h("div", { class: "page-note", hidden: true });
  const listenButton = h("button", { class: "btn primary", type: "button", hidden: true,
    onclick: () => tracks.visible.length && play(tracks.visible, 0, { shuffle: false }) }, icon("play", 14), t("artist.listen"));
  const shuffleButton = h("button", { class: "icon-btn circle-btn", type: "button", hidden: true, title: t("shuffle"), "aria-label": t("shuffle"),
    onclick: () => tracks.visible.length && play(tracks.visible, Math.floor(Math.random() * tracks.visible.length), { shuffle: true }) }, icon("shuffle", 16));
  const head = pageHero({
    back: backButton(t("playlists.back"), () => back()),
    art: photo,
    body: [h("div", { class: "kicker", text: t("artist.kicker") }), name, note],
    actions: [listenButton, shuffleButton],
  });

  const tracks = createTracksScreen({
    title: t("artist.popular"),
    heading: "h2",
    playable: false,
    hooks: { emptyText: t("artist.noTracks"), art: "artist", onPlay: play, onAction: onTrackAction },
  });
  tracks.element.classList.add("artist-tracks");

  const albumsStatus = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "grid artist-albums" });
  const albumsHeading = h("h2", { class: "shelf-title", text: t("artist.albums") });
  const scroller = h("div", { class: "page-scroll" }, tracks.element, albumsHeading, albumsStatus, grid);
  const element = h("section", { class: "view artist-view" }, head, scroller);
  headerOnScroll(head, scroller);

  // Популярные треки имеют собственный ограниченный скроллер; В карточках альбомов используется
  // окружающая прокрутка страниц. Вызовите другую страницу только тогда, когда соответствующая область
  // просмотра приближается к концу.
  tracks.list.element.addEventListener("scroll", () => {
    const el = tracks.list.element;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - ROW_HEIGHT * 2) loadMoreTracks();
  }, { passive: true });
  scroller.addEventListener("scroll", () => {
    if (scroller.scrollTop + scroller.clientHeight > scroller.scrollHeight - 400) loadMoreAlbums();
  }, { passive: true });

  function fitTracks(count) {
    tracks.list.element.style.height = `${Math.min(Math.max(count, 0), VISIBLE_ROWS) * ROW_HEIGHT}px`;
  }

  function renderHeader(artist) {
    name.textContent = artist.name;
    photo.replaceChildren(artBox(artist.photo_url, { glyph: "artist", size: 36, className: "round" }));
    setAmbience(head, artist.photo_url);
  }

  function setAlbumsStatus(text, { error = false } = {}) {
    paintStatus(albumsStatus, text, { error, kind: "cards", extra: "compact" });
  }

  async function loadTracks(id, query, mine) {
    trackNextOffset = null;
    trackItems = [];
    tracks.setTracks([]);
    fitTracks(0);
    tracks.setStatus(t("loading"));
    try {
      const page = id ? await api.artistTracks(id, 0, TRACKS) : await api.search(query, 0, TRACKS);
      if (mine !== generation) return;
      tracks.clearStatus();
      trackItems = page.items;
      tracks.setTracks(trackItems);
      trackNextOffset = page.next_offset;
      fitTracks(page.items.length);
      listenButton.hidden = shuffleButton.hidden = trackItems.length === 0;
    } catch (error) {
      if (mine !== generation) return;
      loadedKey = null; // let the next visit try again
      tracks.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function loadMoreTracks() {
    if (loadingTracks || trackNextOffset == null || !current) return;
    loadingTracks = true;
    const mine = generation;
    try {
      const page = current.id
        ? await api.artistTracks(current.id, trackNextOffset, TRACKS)
        : await api.search(current.name, trackNextOffset, TRACKS);
      if (mine !== generation) return;
      const known = new Set(trackItems.map((track) => `${track.owner_id}_${track.track_id}`));
      trackItems = trackItems.concat(page.items.filter((track) => !known.has(`${track.owner_id}_${track.track_id}`)));
      tracks.setTracks(trackItems);
      fitTracks(trackItems.length);
      trackNextOffset = page.next_offset;
    } catch (error) {
      if (mine === generation) toast(errorText(error), { error: true });
    } finally {
      loadingTracks = false;
    }
  }

  async function loadAlbums(id, query, mine) {
    albumNextOffset = null;
    grid.replaceChildren();
    setAlbumsStatus(t("loading"));
    try {
      let albums;
      if (id) {
        const result = await api.artistAlbums(id, 0, ALBUMS);
        if (mine !== generation) return;
        // В каталоге указан художник с фотографией; сохраните имя, на которое нажал пользователь, если
        // оно не дало ни одного.
        if (result.artist) renderHeader({ ...result.artist, name: result.artist.name || current.name });
        albums = result.albums.items;
        albumNextOffset = result.albums.next_offset;
      } else {
        const page = await api.searchAlbums(query, 0, ALBUMS);
        if (mine !== generation) return;
        albums = page.items;
        albumNextOffset = page.next_offset;
      }
      grid.replaceChildren(...albums.map((album) => albumCard(album, openAlbum, { showArtist: !id })));
      setAlbumsStatus(albums.length ? "" : t("artist.noAlbums"));
    } catch (error) {
      if (mine !== generation) return;
      loadedKey = null;
      setAlbumsStatus(errorText(error), { error: true });
    }
  }

  async function loadMoreAlbums() {
    if (loadingAlbums || albumNextOffset == null || !current) return;
    loadingAlbums = true;
    const mine = generation;
    try {
      const result = current.id
        ? await api.artistAlbums(current.id, albumNextOffset, ALBUMS)
        : await api.searchAlbums(current.name, albumNextOffset, ALBUMS);
      if (mine !== generation) return;
      const albums = current.id ? result.albums.items : result.items;
      grid.append(...albums.map((album) => albumCard(album, openAlbum)));
      albumNextOffset = current.id ? result.albums.next_offset : result.next_offset;
      setAlbumsStatus("");
    } catch (error) {
      if (mine === generation) setAlbumsStatus(errorText(error), { error: true });
    } finally {
      loadingAlbums = false;
    }
  }

  async function open(artist) {
    const key = artist.id ? `id:${artist.id}` : `name:${artist.name}`;
    if (key === loadedKey) return; // вернуться на страницу, как это было
    loadedKey = key;
    current = artist;
    const mine = ++generation;
    renderHeader(artist);
    note.hidden = true;
    listenButton.hidden = shuffleButton.hidden = true;
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
          current = { ...current, id };
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
      trackNextOffset = null;
      albumNextOffset = null;
      loadingTracks = false;
      loadingAlbums = false;
      loadedKey = null;
      current = null;
      trackItems = [];
      tracks.setTracks([]);
      tracks.clearStatus();
      grid.replaceChildren();
      setAlbumsStatus("");
      listenButton.hidden = shuffleButton.hidden = true;
    },
  };
}
