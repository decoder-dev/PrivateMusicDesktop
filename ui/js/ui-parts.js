// Small shared building blocks: artwork with a placeholder, search field,
// slider fill, status/skeleton states, segmented control, hero header.

import { h } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { decorateStatus } from "./network.js";

/**
 * Artwork. The placeholder (accent gradient and a glyph) sits behind the image,
 * so a missing, slow or broken cover never leaves an empty square. No fade: the
 * lists that use it recreate their rows while scrolling.
 */
export function artBox(url, { glyph = "note", size = 18, className = "", eager = false } = {}) {
  const box = h("span", { class: `art-box${className ? ` ${className}` : ""}` }, h("span", { class: "art-ph" }, icon(glyph, size)));
  if (url) {
    const image = h("img", { src: url, alt: "", loading: eager ? "eager" : "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" });
    image.addEventListener("error", () => image.remove());
    box.append(image);
  }
  return box;
}

/**
 * Puts a new cover in `slot` only once it has decoded, so the old cover stays
 * until then (no flash of the placeholder between tracks). `current` tells
 * whether the request is still the latest.
 */
export function swapArt(slot, url, options, current) {
  const next = artBox(url, { ...options, eager: true });
  const image = next.querySelector("img");
  if (!image) { slot.replaceChildren(next); return; }
  image.decode().catch(() => {}).finally(() => { if (current()) slot.replaceChildren(next); });
}

/** A search field with a leading glyph and a clear button. Returns { element, input }. */
export function searchField({ placeholder, label, className = "", ...handlers }) {
  const input = h("input", { class: "field", type: "search", placeholder, "aria-label": label ?? placeholder, ...handlers });
  const clear = h("button", { class: "clear", type: "button", hidden: true, title: t("field.clear"), "aria-label": t("field.clear"),
    onclick: () => { input.value = ""; sync(); input.dispatchEvent(new Event("input", { bubbles: true })); input.focus(); } }, icon("clear", 16));
  const sync = () => { clear.hidden = !input.value; };
  input.addEventListener("input", sync);
  const element = h("label", { class: `search-field ${className}`.trim() }, icon("search", 16), input, clear);
  return { element, input, sync };
}

/** Keeps the filled part of a slider's track in step with its value. */
export function syncRange(input) {
  const min = Number(input.min || 0);
  const max = Number(input.max || 100);
  const share = max > min ? ((Number(input.value) - min) / (max - min)) * 100 : 0;
  input.style.setProperty("--fill", `${Math.max(0, Math.min(100, share))}%`); // CSSOM: allowed under the CSP
}

export function bindRange(input) {
  input.addEventListener("input", () => syncRange(input));
  syncRange(input);
  return input;
}

const LOADING = () => new Set([t("loading"), t("friends.loading")]);

function skeleton(kind) {
  if (kind === "tiles") {
    return h("div", { class: "sk-tiles", "aria-hidden": "true" }, Array.from({ length: 4 }, () => h("div", { class: "sk-art sk-tile" })));
  }
  if (kind === "cards") {
    return h("div", { class: "sk-cards", "aria-hidden": "true" },
      Array.from({ length: 6 }, () => h("div", { class: "sk-card" }, h("div", { class: "sk-art" }), h("div", { class: "sk-bar" }), h("div", { class: "sk-bar" }))));
  }
  return h("div", { class: "sk-rows", "aria-hidden": "true" },
    Array.from({ length: 6 }, () => h("div", { class: "sk-row" }, h("div", { class: "sk-art" }),
      h("div", { class: "sk-lines" }, h("div", { class: "sk-bar a" }), h("div", { class: "sk-bar b" })))));
}

/**
 * The one way a screen shows "loading", an error or an empty note in its status
 * element: loading becomes a static skeleton (the text stays for screen
 * readers), anything else a status panel. Empty text hides the element.
 */
export function paintStatus(element, text, { error = false, kind = "rows", extra = "" } = {}) {
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
    element.replaceChildren(h("span", { class: "sr-only", text }), skeleton(kind));
    return;
  }
  element.removeAttribute("aria-busy");
  element.className = `status${error ? " error" : ""}${extra ? ` ${extra}` : ""}`;
  element.textContent = text;
  // A failed connection gets the VPN hint and a retry (see network.js).
  if (error) decorateStatus(element, text);
}

/**
 * Makes `.tabs` a segmented control whose thumb slides under the selected
 * button. The selection is read from aria-selected whenever it changes.
 */
export function watchSegmented(tabs) {
  const place = () => {
    const selected = tabs.querySelector('button[aria-selected="true"]');
    if (!selected || !selected.offsetWidth) return;
    tabs.style.setProperty("--seg-x", `${selected.offsetLeft}px`);
    tabs.style.setProperty("--seg-w", `${selected.offsetWidth}px`);
    // The first placement must not slide in from nowhere.
    requestAnimationFrame(() => tabs.classList.add("ready"));
  };
  new MutationObserver(place).observe(tabs, { attributes: true, attributeFilter: ["aria-selected"], subtree: true });
  new ResizeObserver(place).observe(tabs);
  place();
  return tabs;
}

/**
 * A header with a hero card, for pages about one thing (artist, album,
 * playlist, friend): back button above, then art, text and actions.
 */
export function pageHero({ back, art, body, actions }) {
  return h("header", { class: "view-header page-head" },
    h("div", { class: "page-nav" }, back ?? null),
    h("div", { class: "hero-card" }, art, h("div", { class: "hero-body" }, body, actions ? h("div", { class: "hero-actions" }, actions) : null)));
}

export function backButton(label, onclick) {
  return h("button", { class: "icon-btn back circle-btn", type: "button", title: label, "aria-label": label, onclick }, icon("back", 18));
}

/** Toggles a hairline under the header once the content below has scrolled. */
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
