import { h } from "./dom.js";
import { icon } from "./icons.js";
import { tn } from "./i18n.js";
import { artBox, initial } from "./ui-parts.js";

const artistLine = (album) => album.artists.map((a) => a.name).join(", ");

/** Релиз в сетке: обложка, название и год или исполнитель под ним. */
export function albumCard(album, open, { showArtist = true } = {}) {
  const art = h("span", { class: "card-art art" }, artBox(album.artwork_url, { className: "fill", fallbackText: initial(album.title) }), h("span", { class: "play-chip" }, icon("play", 12)));
  const details = [album.year, showArtist ? artistLine(album) : null, !showArtist && album.count ? tn("album.count", album.count) : null]
    .filter(Boolean).join(" · ");
  return h("button", { class: "card album-card", type: "button", onclick: () => open(album) },
    art,
    h("div", { class: "name", text: album.title }),
    h("div", { class: "count", text: details }));
}

/** Художник в сетке: круглое фото и имя. */
export function artistCard(artist, open) {
  const art = h("span", { class: "card-art art" }, artBox(artist.photo_url, { glyph: "artist", size: 36, className: "fill round" }));
  return h("button", { class: "card artist-card", type: "button", onclick: () => open(artist) },
    art,
    h("div", { class: "name", text: artist.name }));
}

export { artistLine };
