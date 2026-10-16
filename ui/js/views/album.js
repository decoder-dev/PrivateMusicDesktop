import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { subscribe } from "../store.js";
import { openArtist } from "../navigation.js";
import { albumKey, ensureAlbums, isFollowed, toggleFollow } from "../my-albums.js";

const PAGE = 100;

/**
 * A release: cover, title, artists (each opens its page), year and size,
 * a follow button, and the tracks. The engine finds the access key a
 * community album needs; the album it answers with (cover, year, follow
 * state, key) replaces what the page was opened with.
 */
export function createAlbumView({ play, onTrackAction, toast, back }) {
  let album = null;
  let loadedKey = null;
  let generation = 0;
  let busy = false;

  const cover = h("div", { class: "page-art" });
  const title = h("h1");
  const artists = h("div", { class: "page-artists" });
  const meta = h("div", { class: "page-note" });
  const followButton = h("button", { class: "btn follow", type: "button", onclick: () => follow() });
  const backButton = h("button", { class: "icon-btn", type: "button", title: t("playlists.back"), "aria-label": t("playlists.back"), onclick: () => back() }, icon("back", 20));
  const head = h("header", { class: "view-header page-head" }, backButton, cover, h("div", { class: "page-info" }, title, artists, meta), followButton);

  const tracks = createTracksScreen({
    title: t("album.tracks"),
    heading: "h2",
    hooks: { emptyText: t("album.empty"), onPlay: play, onAction: onTrackAction },
  });
  const element = h("section", { class: "view album-view" }, head, tracks.element);

  function renderFollow() {
    if (!album) return;
    const on = isFollowed(album);
    followButton.textContent = on ? t("album.unfollow") : t("album.follow");
    followButton.setAttribute("aria-pressed", String(on));
    followButton.classList.toggle("primary", !on);
    followButton.disabled = busy;
  }

  function render() {
    title.textContent = album.title;
    cover.replaceChildren(album.artwork_url
      ? h("img", { src: album.artwork_url, alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
      : icon("album", 40));
    artists.replaceChildren(...album.artists.flatMap((artist, i) => [
      i ? ", " : null,
      h("button", { class: "link-text", type: "button", title: t("artist.open"), onclick: () => openArtist(artist), text: artist.name }),
    ]).filter(Boolean));
    meta.textContent = [album.year, album.count ? t("album.count", { n: album.count }) : null].filter(Boolean).join(" · ");
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
    if (key === loadedKey) return; // back to the page as it was
    loadedKey = key;
    album = opened;
    busy = false;
    const mine = ++generation;
    render();
    ensureAlbums().catch(() => {}); // the follow state; the album's own flag covers a failure
    tracks.setTracks([]);
    tracks.setStatus(t("loading"));
    let collected = [];
    let offset = 0;
    try {
      for (;;) {
        const result = await api.albumTracks(album, offset, PAGE);
        if (mine !== generation) return;
        // Later pages are asked for with the key that opened the first.
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
