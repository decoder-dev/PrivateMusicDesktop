import { collectionShareLink } from "../share-links.js";
import { linkControls } from "../sharing.js";
import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t, tn } from "../i18n.js";
import { artBox, backButton, pageHero, headerOnScroll, setAmbience, initial } from "../ui-parts.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { subscribe } from "../store.js";
import { openArtist } from "../navigation.js";
import { albumKey, ensureAlbums, isFollowed, toggleFollow } from "../my-albums.js";

const PAGE = 100;

/**
 * Релиз: обложка, название, исполнители (каждый открывает свою страницу), год и размер, кнопка
 * «подписаться» и треки. Движок находит ключ доступа, необходимый альбому сообщества; альбом, на
 * который он отвечает (обложка, год, состояние подписки, ключ), заменяет то, с чем была открыта
 * страница.
 */
export function createAlbumView({ play, onTrackAction, toast, back }) {
  let album = null;
  let loadedKey = null;
  let generation = 0;
  let busy = false;

  const cover = h("span", { class: "hero-art page-art" });
  const title = h("h1");
  const artists = h("div", { class: "page-artists" });
  const meta = h("div", { class: "page-note" });
  const followButton = h("button", { class: "btn follow", type: "button", onclick: () => follow() });
  const listenButton = h("button", { class: "btn primary", type: "button", hidden: true,
    onclick: () => tracks.visible.length && play(tracks.visible, 0, { shuffle: false }) }, icon("play", 14), t("artist.listen"));
  const shuffleButton = h("button", { class: "icon-btn circle-btn", type: "button", hidden: true, title: t("shuffle"), "aria-label": t("shuffle"),
    onclick: () => tracks.visible.length && play(tracks.visible, Math.floor(Math.random() * tracks.visible.length), { shuffle: true }) }, icon("shuffle", 16));
  const links=linkControls(()=>collectionShareLink("album",album),()=>album?.title,toast);
  const head = pageHero({
    back: backButton(t("playlists.back"), () => back()),
    art: cover,
    body: [h("div", { class: "kicker", text: t("album.kicker") }), title, artists, meta],
    actions: [listenButton, shuffleButton, followButton, links.copy, links.share],
  });

  const tracks = createTracksScreen({
    title: t("album.tracks"),
    heading: "h2",
    playable: false,
    hooks: { emptyText: t("album.empty"), art: "album", onPlay: play, onAction: onTrackAction },
  });
  const element = h("section", { class: "view album-view" }, head, tracks.element);
  headerOnScroll(head, tracks.list.element);

  function renderFollow() {
    if (!album) return;
    const on = isFollowed(album);
    followButton.textContent = on ? t("album.unfollow") : t("album.follow");
    followButton.setAttribute("aria-pressed", String(on));
    followButton.classList.toggle("on", on);
    followButton.disabled = busy;
    followButton.setAttribute("aria-busy", String(busy));
  }

  function render() {
    links.refresh();
    title.textContent = album.title;
    cover.replaceChildren(artBox(album.artwork_url, { fallbackText: initial(album.title) }));
    setAmbience(head, album.artwork_url);
    artists.replaceChildren(...album.artists.flatMap((artist, i) => [
      i ? ", " : null,
      h("button", { class: "link-text", type: "button", title: t("artist.open"), onclick: () => openArtist(artist), text: artist.name }),
    ]).filter(Boolean));
    meta.textContent = [album.year, album.count ? tn("album.count", album.count) : null].filter(Boolean).join(" · ");
    renderFollow();
  }

  subscribe("albums", renderFollow);

  async function follow() {
    if (!album || busy) return;
    busy = true;
    renderFollow();
    try {
      const on = await toggleFollow(album);
      toast(on ? t("album.followed", { title: album.title }) : t("album.unfollowed"));
    } catch (error) {
      toast(errorText(error), { error: true });
    } finally {
      busy = false;
      renderFollow();
    }
  }

  async function open(opened) {
    const key = albumKey(opened);
    if (key === loadedKey) return; // вернуться на страницу, как это было
    loadedKey = key;
    album = opened;
    busy = false;
    const mine = ++generation;
    render();
    ensureAlbums().catch(() => {}); // следующее состояние; собственный флаг альбома означает провал
    tracks.setTracks([]);
    tracks.setStatus(t("loading"));
    listenButton.hidden = shuffleButton.hidden = true;
    let collected = [];
    let offset = 0;
    try {
      for (;;) {
        const result = await api.albumTracks(album, offset, PAGE);
        if (mine !== generation) return;
        // Последующие страницы запрашиваются с помощью ключа, открывающего первую.
        album = { ...result.album, title: result.album.title || album.title };
        render();
        collected = collected.concat(result.tracks.items);
        tracks.setProgress(collected.slice(), result.tracks.total);
        const next = result.tracks.next_offset;
        if (next == null || next <= offset) break;
        offset = next;
      }
      tracks.clearStatus();
      tracks.setTracks(collected);
      listenButton.hidden = shuffleButton.hidden = collected.length === 0;
    } catch (error) {
      if (mine !== generation) return;
      loadedKey = null; // let the next visit try again
      const text = error?.kind === "vk" ? t("album.denied") : errorText(error);
      tracks.setStatus(text, { error: true });
      toast(text, { error: true });
    }
  }

  return {
    element,
    open,
    reset() {
      generation++;
      loadedKey = null;
      album = null;
      tracks.setTracks([]);
      tracks.clearStatus();
    },
  };
}
