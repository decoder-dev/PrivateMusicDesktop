// Небольшие общие строительные блоки: графическое изображение с заполнителем, поле поиска, заливка
// ползунка, состояния status/skeleton, сегментированный элемент управления, главный заголовок.

import { h } from "./dom.js";
import { icon, illustration } from "./icons.js";
import { t } from "./i18n.js";
import { coverPalette } from "./palette.js";
import { decorateStatus, forgetStatus } from "./network.js";

/** Real artwork with an explicit load state; absent artwork stays quiet. */
export function artBox(url, { glyph = "note", size = 18, className = "", eager = false, fallbackText } = {}) {
  const fallback = h("span", { class: "art-ph", "aria-hidden": "true" }, fallbackText == null ? icon(glyph, size) : String(fallbackText));
  const box = h("span", { class: `art-box${className ? ` ${className}` : ""}`, "data-art": url ? "loading" : "missing" }, fallback);
  if (url) {
    const image = h("img", { alt: "", loading: eager ? "eager" : "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" });
    image.addEventListener("load", () => { box.dataset.art = "ready"; }, { once: true });
    image.addEventListener("error", () => { box.dataset.art = "error"; image.remove(); }, { once: true });
    image.src = url;
    box.append(image);
  }
  return box;
}

/**
 * Replace the old track's artwork immediately. Waiting for decode used to leave
 * the previous cover beside the new title for seconds on a slow connection.
 */
export function swapArt(slot, url, options, current) {
  if (current()) slot.replaceChildren(artBox(url, { fallbackText: "", ...options, eager: true }));
}

/** Поле поиска с ведущим глифом и кнопкой очистки. Возвращает {элемент, ввод}. */
export function searchField({ placeholder, label, className = "", ...handlers }) {
  const input = h("input", { class: "field", type: "search", placeholder, "aria-label": label ?? placeholder, inputmode: "search", enterkeyhint: "search", ...handlers });
  const clear = h("button", { class: "clear", type: "button", hidden: true, title: t("field.clear"), "aria-label": t("field.clear"),
    onclick: () => { input.value = ""; sync(); input.dispatchEvent(new Event("input", { bubbles: true })); input.focus(); } }, icon("clear", 16));
  const sync = () => { clear.hidden = !input.value; };
  input.addEventListener("input", sync);
  const element = h("label", { class: `search-field ${className}`.trim() }, icon("search", 16), input, clear);
  return { element, input, sync };
}

/** Сохраняет заполненную часть дорожки ползунка в соответствии с его значением. */
export function syncRange(input) {
  const min = Number(input.min || 0);
  const max = Number(input.max || 100);
  const share = max > min ? ((Number(input.value) - min) / (max - min)) * 100 : 0;
  const fill=`${Math.max(0, Math.min(100, share))}%`;
  if(input.style.getPropertyValue("--fill")!==fill) input.style.setProperty("--fill", fill); // CSSOM: разрешено под номером CSP.
}

export function bindRange(input) {
  input.addEventListener("input", () => syncRange(input));
  syncRange(input);
  return input;
}

const LOADING = () => new Set([t("loading"), t("friends.loading")]);


/**
 * В одном случае на экране отображается "loading", ошибка или пустое примечание в элементе состояния:
 * загрузка получает краткую текстовую подпись без декоративных заглушек, все
 * остальное - панелью состояния. Пустой текст скрывает элемент.
 */
export function paintStatus(element, text, { error = false, kind = "rows", extra = "", art = null } = {}) {
  forgetStatus(element);
  if (!text) {
    element.hidden = true;
    element.className = `status${extra ? ` ${extra}` : ""}`;
    element.replaceChildren();
    return;
  }
  element.hidden = false;
  if (LOADING().has(text)) {
    element.className = `status loading ${kind}${extra ? ` ${extra}` : ""}`;
    element.setAttribute("aria-busy", "true");
    element.replaceChildren(h("span", { class: "loading-label", text }));
    return;
  }
  element.removeAttribute("aria-busy");
  element.className = `status${error ? " error" : ""}${extra ? ` ${extra}` : ""}`;
  fillPanel(element, text, error ? "error" : art);
  // Неудачное соединение получает подсказку VPN и повторную попытку (см. network.js).
  if (error) decorateStatus(element, text);
}

/**
 * Устанавливает текст панели с небольшой иллюстрацией над ним, если панель настоящая (а не встроенная
 * строка).
 */
export function fillPanel(element, text, art) {
  const panel = !/\b(inline|progress|radio-note)\b/.test(element.className);
  if (panel && art !== false) {
    element.classList.add("has-il");
    const label = h("span", { class: "status-text", text });
    element.replaceChildren(illustration(art ?? "music"), label);
  } else {
    element.classList.remove("has-il");
    element.textContent = text;
  }
}

/**
 * Делает `.tabs` сегментированным элементом управления, большой палец которого перемещается под
 * выбранную кнопку. Выбор считывается из aria-selected всякий раз, когда он изменяется.
 */
export function watchSegmented(tabs) {
  const place = () => {
    const selected = tabs.querySelector('button[aria-selected="true"]');
    if (!selected || !selected.offsetWidth) return;
    tabs.style.setProperty("--seg-x", `${selected.offsetLeft}px`);
    tabs.style.setProperty("--seg-w", `${selected.offsetWidth}px`);
    // Первое размещение не должно появляться из ниоткуда.
    requestAnimationFrame(() => tabs.classList.add("ready"));
  };
  new MutationObserver(place).observe(tabs, { attributes: true, attributeFilter: ["aria-selected"], subtree: true });
  new ResizeObserver(place).observe(tabs);
  place();
  return tabs;
}

/**
 * Заголовок с карточкой героя для страниц об одном (исполнитель, альбом, плейлист, друг): кнопка
 * «Назад» вверху, затем рисунок, текст и действия.
 */
export function pageHero({ back, art, body, actions }) {
  return h("header", { class: "view-header page-head" },
    h("div", { class: "page-nav" }, back ?? null),
    h("div", { class: "hero-card" }, art, h("div", { class: "hero-body" }, body, actions ? h("div", { class: "hero-actions" }, actions) : null)));
}

export function backButton(label, onclick) {
  return h("button", { class: "icon-btn back circle-btn", type: "button", title: label, "aria-label": label, onclick }, icon("back", 18));
}

/** Переключает линию волос под заголовком после прокрутки содержимого ниже. */
export function headerOnScroll(header, scroller) {
  let frame = 0;
  scroller.addEventListener("scroll", () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      header.classList.toggle("is-scrolled", scroller.scrollTop > 2);
    });
  }, { passive: true });
}

/**
 * Цветовая атмосфера за заголовком героя: два мягких радиальных оттенка из палитры обложки (движок уже
 * извлекает их), нарисованные в виде градиентов, поэтому нет размытия и растра, который нужно
 * перерисовывать. Без обложки или когда палитра не читается, заголовок просто остается простым.
 */
export async function setAmbience(header, url) {
  const token = (header._ambience = (header._ambience ?? 0) + 1);
  if (!url) { header.classList.remove("has-amb"); return; }
  try {
    const palette = await coverPalette(url);
    if (token !== header._ambience) return;
    header.style.setProperty("--amb-a", palette.primary);
    header.style.setProperty("--amb-b", palette.secondary ?? palette.primary);
    header.classList.add("has-amb");
  } catch {
    if (token === header._ambience) header.classList.remove("has-amb");
  }
}
