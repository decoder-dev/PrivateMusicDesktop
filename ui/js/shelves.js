// Две горизонтальные полки каталога VK на главной телефона: миксы (нажатие запускает микс) и новые
// релизы (нажатие открывает альбом). Это те же данные и те же карточки «Студии», что на экране
// «Для вас» (views/for-you.js), без закрепления и настройки: их место там. Полка показывается, только
// когда есть что показать: предметы или одна строка о причине. Пустой ответ скрывает её совсем.

import { h } from "./dom.js";
import { api } from "./api.js";
import { albumCard } from "./album-card.js";
import { openAlbum } from "./navigation.js";
import { errorText, t } from "./i18n.js";
import { icon } from "./icons.js";
import { mixName, mixColors, mixKey } from "./selena.js";
import { paintStatus } from "./ui-parts.js";

/** Устойчивое семейство цветов для каждого микса, чтобы карточка выглядела одинаково между визитами. */
export function mixHue(text) {
  let hash = 0;
  for (const char of text) hash = (hash * 31 + char.codePointAt(0)) >>> 0;
  return hash % 5;
}

/** Одна компактная строка: причина и ссылка, чтобы повторить попытку. */
function paintShelfError(status, title, error, retry) {
  paintStatus(status, errorText(error), { error: true, extra: "inline" });
  status.append(h("button", { class: "link-text retry-link", type: "button", text: t("fy.retryTune"), onclick: retry }));
  title.hidden = true;
}

/** Миксы каталога: `{ element, load(force), reset(), suppress(bool), state }`. */
export function createMixShelf({ play, toast, heading = t("fy.catalog") }) {
  let loaded = false;
  let generation = 0;
  let state = "empty"; // "empty" | "items" | "error"
  let suppressed = false;
  const status = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "mix-grid" });
  const title = h("h2", { class: "shelf-title", text: heading });
  const element = h("section", { class: "mix-catalog", hidden: true }, title, status, grid);

  const paint = () => { element.hidden = suppressed || state === "empty"; };

  async function playMix(mix, button) {
    button.disabled = true;
    paintStatus(status, t("fy.mixLoading", { title: mix.title }), { extra: "inline" });
    try {
      const tracks = await api.streamMix(mix.id, mix.options);
      if (!tracks.length) throw new Error(t("fy.noCatalog"));
      // Обычный личный поток — то же радио, что и вкладка «Мой микс».
      const personal = mix.id === "common" && !mix.options;
      play(tracks, 0, { shuffle: false, radio: personal ? { kind: "personal", options: null } : { kind: "catalog", mix_id: mix.id, options: mix.options ?? null } });
      paintStatus(status, "");
    } catch (error) {
      paintStatus(status, errorText(error), { error: true, extra: "inline" });
      toast(errorText(error), { error: true });
    } finally {
      button.disabled = false;
    }
  }

  async function load(force = false) {
    if (loaded && !force) return;
    const mine = ++generation;
    // Без заглушки: полка, которая окажется пустой, не должна мигать панелью. При повторе строка
    // ошибки остаётся с отключённой ссылкой до ответа.
    status.querySelector(".retry-link")?.setAttribute("disabled", "");
    try {
      const mixes = await api.catalogMixes();
      if (mine !== generation) return;
      grid.replaceChildren(...mixes.map((mix) => {
        const key = mixKey(mix);
        const glyph = /настро|mood/i.test(mix.section_title ?? "") ? "sparkles" : /жанр|genre/i.test(mix.section_title ?? "") ? "album" : "radio";
        const button = h("button", { class: `mix-tile mobile-mix hue-${mixHue(key)}`, type: "button", "data-mix-key": key, onclick: () => playMix(mix, button) },
          h("span", { class: "mix-symbol", "aria-hidden": "true" }, icon(glyph, 23)),
          h("span", { class: "mix-text" },
            mix.section_title ? h("span", { class: "mix-tile-section", text: mixName(mix.section_title) }) : null,
            h("span", { class: "mix-tile-title", text: mixName(mix.title) }),
            mix.subtitle ? h("span", { class: "mix-tile-subtitle", text: mixName(mix.subtitle) }) : null),
          h("span", { class: "mix-play", "aria-hidden": "true" }, icon("play", 14)));
        mixColors(`${key}|${mixName(mix.title)}`).forEach((color, index) => button.style.setProperty(`--mix-c${index}`, color));
        return h("article", { class: "mix-card" }, button);
      }));
      loaded = true;
      paintStatus(status, "");
      title.hidden = false;
      state = mixes.length ? "items" : "empty";
    } catch (error) {
      if (mine !== generation) return;
      grid.replaceChildren();
      paintShelfError(status, title, error, () => load(true));
      state = "error";
    }
    paint();
  }

  return {
    element, load, suppress(value) { suppressed = value; paint(); },
    get state() { return state; },
    reset() {
      generation += 1;
      loaded = false;
      grid.replaceChildren();
      paintStatus(status, "");
      title.hidden = false;
      state = "empty";
      paint();
    },
  };
}

/** Новые релизы. */
export function createReleasesShelf({ heading = t("fy.releases") } = {}) {
  let loaded = false;
  let generation = 0;
  let state = "empty";
  const status = h("div", { class: "status", role: "status", hidden: true });
  const row = h("div", { class: "release-row" });
  const title = h("h2", { class: "shelf-title", text: heading });
  const element = h("section", { class: "mix-catalog new-releases", hidden: true }, title, status, row);

  const paint = () => { element.hidden = state === "empty"; };

  async function load(force = false) {
    if (loaded && !force) return;
    const mine = ++generation;
    status.querySelector(".retry-link")?.setAttribute("disabled", "");
    try {
      const albums = await api.newReleases();
      if (mine !== generation) return;
      // Релиз, пришедший без названия, всё равно получает карточку.
      row.replaceChildren(...albums.map((album) =>
        albumCard(album.title?.trim() ? album : { ...album, title: t("album.untitled") }, () => openAlbum(album))));
      loaded = true;
      paintStatus(status, "");
      title.hidden = false;
      state = albums.length ? "items" : "empty";
    } catch (error) {
      if (mine !== generation) return;
      row.replaceChildren();
      paintShelfError(status, title, error, () => load(true));
      state = "error";
    }
    paint();
  }

  return {
    element, load,
    get state() { return state; },
    reset() {
      generation += 1;
      loaded = false;
      row.replaceChildren();
      paintStatus(status, "");
      title.hidden = false;
      state = "empty";
      paint();
    },
  };
}
