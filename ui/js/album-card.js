import { h } from "./dom.js";
import { icon } from "./icons.js";
import { tn } from "./i18n.js";
import { artBox } from "./ui-parts.js";

const artistLine = (album) => album.artists.map((a) => a.name).join(", ");

/** A release in a grid: cover, title, and year or artist under it. */
export function albumCard(album, open, { showArtist = true } = {}) {
  const art = h("span", { class: "card-art art" }, artBox(album.artwork_url, { glyph: "album", size: 36, className: "fill" }), h("span", { class: "play-chip" }, icon("play", 12)));
  const details = [album.year, showArtist ? artistLine(album) : null, !showArtist && album.count ? tn("album.count", album.count) : null]
    .filter(Boolean).join(" · ");
  return h("button", { class: "card album-card", type: "button", onclick: () => open(album) },
    art,
    h("div", { class: "name", text: album.title }),
    h("div", { class: "count", text: details }));
}

/** An artist in a grid: round photo and name. */
export function artistCard(artist, open) {
  const art = h("span", { class: "card-art art" }, artBox(artist.photo_url, { glyph: "artist", size: 36, className: "fill round" }));
  return h("button", { class: "card artist-card", type: "button", onclick: () => open(artist) },
    art,
    h("div", { class: "name", text: artist.name }));
}

export { artistLine };
