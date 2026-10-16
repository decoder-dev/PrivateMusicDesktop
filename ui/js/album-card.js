import { h } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";

const artistLine = (album) => album.artists.map((a) => a.name).join(", ");

/** A release in a grid: cover, title, and year or artist under it. */
export function albumCard(album, open, { showArtist = true } = {}) {
  const art = album.artwork_url
    ? h("img", { class: "art", src: album.artwork_url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
    : h("div", { class: "art empty" }, icon("album", 36));
  const details = [album.year, showArtist ? artistLine(album) : null, !showArtist && album.count ? t("album.count", { n: album.count }) : null]
    .filter(Boolean).join(" · ");
  return h("button", { class: "card album-card", type: "button", onclick: () => open(album) },
    art,
    h("div", { class: "name", text: album.title }),
    h("div", { class: "count", text: details }));
}

/** An artist in a grid: round photo and name. */
export function artistCard(artist, open) {
  const art = artist.photo_url
    ? h("img", { class: "art", src: artist.photo_url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
    : h("div", { class: "art empty" }, icon("artist", 36));
  return h("button", { class: "card artist-card", type: "button", onclick: () => open(artist) },
    art,
    h("div", { class: "name", text: artist.name }));
}

export { artistLine };
