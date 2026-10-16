import { h } from "../dom.js";
import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { albumCard } from "../album-card.js";
import { openAlbum } from "../navigation.js";
import { errorText, t } from "../i18n.js";

const SOURCES = {
  mix: { label: "fy.mix", load: () => api.mix() },
  recs: { label: "fy.recs", load: () => api.recommendations() },
};

/** VK's personal mix and recommendations, one tab each, loaded on first look. */
export function createForYouView({ play, onTrackAction, toast }) {
  let current = "mix";
  const cache = new Map(); // source -> tracks
  let generation = 0;
  let categories = null;
  let selected = readSelection();
  let catalogLoaded = false;
  let catalogGeneration = 0;
  let releasesLoaded = false;
  let releasesGeneration = 0;
  // The tuning the list on screen was loaded with: a queue started from it
  // keeps refilling from the same mix.
  let listOptions = null;

  const tabs = h("div", { class: "tabs", role: "tablist" },
    Object.entries(SOURCES).map(([id, source]) =>
      h("button", { type: "button", role: "tab", "data-source": id, onclick: () => select(id), text: t(source.label) })));
  const refresh = h("button", { class: "btn", type: "button", onclick: () => load(current, { force: true }), text: t("fy.refresh") });
  const tuningContent = h("div", { class: "mix-tuning-content" });
  const tuning = h("details", { class: "mix-tuning", ontoggle: (event) => {
    if (event.target.open) loadTuning();
  } }, h("summary", { text: t("fy.tune") }), tuningContent);
  const catalogStatus = h("div", { class: "status", role: "status", hidden: true });
  const catalogGrid = h("div", { class: "mix-grid" });
  const catalogShelf = h("section", { class: "mix-catalog" }, h("h2", { class: "shelf-title", text: t("fy.catalog") }), catalogStatus, catalogGrid);
  const releasesStatus = h("div", { class: "status", role: "status", hidden: true });
  const releasesRow = h("div", { class: "release-row" });
  const releasesShelf = h("section", { class: "mix-catalog new-releases" }, h("h2", { class: "shelf-title", text: t("fy.releases") }), releasesStatus, releasesRow);

  const screen = createTracksScreen({
    title: t("fy.title"),
    hooks: {
      emptyText: t("fy.empty"),
      onPlay: (tracks, index, options) =>
        play(tracks, index, current === "mix" ? { ...options, radio: { kind: "personal", options: listOptions } } : options),
      onAction: onTrackAction,
      headerExtra: h("div", { class: "set-inline" }, tabs, tuning, refresh),
    },
  });
  // The shelves scroll on their own, so a long catalogue never pushes the list off the screen.
  screen.element.insertBefore(h("div", { class: "fy-shelves" }, catalogShelf, releasesShelf), screen.list.element);

  function renderTabs() {
    for (const button of tabs.children) button.setAttribute("aria-selected", String(button.dataset.source === current));
    tuning.hidden = current !== "mix";
    catalogShelf.hidden = current !== "mix";
  }

  async function load(id, { force = false } = {}) {
    const options = id === "mix" ? encodeSelection(selected) : null;
    const cacheKey = id === "mix" ? `mix:${options ?? "plain"}` : id;
    if (!force && cache.has(cacheKey)) {
      if (id === "mix") listOptions = options;
      screen.clearStatus();
      screen.setTracks(cache.get(cacheKey));
      return;
    }
    const mine = ++generation;
    screen.setTracks([]);
    screen.setStatus(t("loading"));
    refresh.disabled = true;
    try {
      const tracks = id === "mix" ? await api.mix(options) : await SOURCES[id].load();
      cache.set(cacheKey, tracks);
      if (mine !== generation) return;
      if (id === "mix") listOptions = options;
      screen.clearStatus();
      screen.setTracks(tracks);
    } catch (error) {
      if (mine !== generation) return;
      screen.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    } finally {
      if (mine === generation) refresh.disabled = false;
    }
  }

  function select(id) {
    current = id;
    renderTabs();
    load(id);
  }

  async function loadTuning() {
    if (categories) return;
    tuningContent.replaceChildren(h("div", { class: "status", text: t("loading") }));
    try {
      const settings = await api.mixSettings();
      categories = findCategories(settings);
      if (!categories.length) {
        tuningContent.replaceChildren(h("div", { class: "status", text: t("fy.noTuning") }));
        return;
      }
      for (const category of categories) {
        const id = category.type ?? category.id ?? category.title;
        const options = Array.isArray(category.options) ? category.options : [];
        const multiple = category.multi_select === true || category.multiselect === true;
        const fields = options.map((option, index) => {
          const value = typeof option === "string" ? option : String(option.id ?? option.option_id ?? option.value ?? option.key ?? option.title ?? option.name ?? index);
          const label = typeof option === "string" ? option : String(option.title ?? option.name ?? option.text ?? value);
          const checked = option && typeof option === "object" && (option.selected === true || option.is_selected === true || option.active === true);
          if (checked && !selected[id]) selected[id] = [];
          if (checked && !selected[id].includes(value)) selected[id].push(value);
          const input = h("input", {
            type: multiple ? "checkbox" : "radio",
            name: `mix-option-${id}`,
            value,
            checked: selected[id]?.includes(value) ?? false,
            onchange: (event) => updateSelection(id, value, multiple, event.target.checked),
          });
          return h("label", { class: "mix-tuning-option" }, input, h("span", { text: label }));
        });
        tuningContent.append(h("fieldset", { class: "mix-tuning-category" },
          h("legend", { text: category.title ?? id }), ...fields));
      }
      const actions = h("div", { class: "mix-tuning-actions" },
        h("button", { class: "btn", type: "button", text: t("fy.clearTune"), onclick: () => {
          selected = {};
          saveSelection();
          tuningContent.querySelectorAll("input").forEach((input) => { input.checked = false; });
        } }),
        h("button", { class: "btn primary", type: "button", text: t("fy.applyTune"), onclick: () => {
          saveSelection();
          cache.clear();
          tuning.open = false;
          current = "mix";
          renderTabs();
          load("mix", { force: true });
        } }));
      tuningContent.append(actions);
    } catch (error) {
      categories = null;
      tuningContent.replaceChildren(h("div", { class: "status error", text: errorText(error) }),
        h("button", { class: "btn", type: "button", text: t("fy.retryTune"), onclick: loadTuning }));
    }
  }

  async function loadCatalog(force = false) {
    if (catalogLoaded && !force) return;
    const mine = ++catalogGeneration;
    catalogStatus.className = "status";
    catalogStatus.hidden = false;
    catalogStatus.textContent = t("loading");
    catalogGrid.replaceChildren();
    try {
      const mixes = await api.catalogMixes();
      if (mine !== catalogGeneration) return;
      const visible = mixes.filter((mix) => mix.id !== "common");
      catalogGrid.replaceChildren(...visible.map((mix) => {
        const artwork = mix.artwork_url
          ? h("img", { src: mix.artwork_url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer" })
          : h("span", { class: "mix-placeholder", text: "♪" });
        const button = h("button", { class: "mix-tile", type: "button", onclick: () => playCatalogMix(mix, button) },
          artwork, h("span", { class: "mix-tile-title", text: mix.title }),
          mix.section_title ? h("span", { class: "mix-tile-section", text: mix.section_title }) : null);
        return button;
      }));
      catalogLoaded = true;
      catalogStatus.textContent = visible.length ? "" : t("fy.noCatalog");
      catalogStatus.hidden = !!visible.length;
    } catch (error) {
      if (mine !== catalogGeneration) return;
      catalogStatus.className = "status error";
      catalogStatus.replaceChildren(document.createTextNode(errorText(error)), h("span", { text: " " }),
        h("button", { class: "btn", type: "button", text: t("fy.refresh"), onclick: () => loadCatalog(true) }));
    }
  }

  async function loadReleases(force = false) {
    if (releasesLoaded && !force) return;
    const mine = ++releasesGeneration;
    releasesStatus.className = "status";
    releasesStatus.hidden = false;
    releasesStatus.textContent = t("loading");
    releasesRow.replaceChildren();
    try {
      const albums = await api.newReleases();
      if (mine !== releasesGeneration) return;
      // A release VK sends without a title still gets a card.
      releasesRow.replaceChildren(...albums.map((album) =>
        albumCard(album.title?.trim() ? album : { ...album, title: t("album.untitled") }, () => openAlbum(album))));
      releasesLoaded = true;
      releasesStatus.textContent = albums.length ? "" : t("fy.noReleases");
      releasesStatus.hidden = !!albums.length;
    } catch (error) {
      if (mine !== releasesGeneration) return;
      releasesStatus.className = "status error";
      releasesStatus.replaceChildren(document.createTextNode(errorText(error)), h("span", { text: " " }),
        h("button", { class: "btn", type: "button", text: t("fy.refresh"), onclick: () => loadReleases(true) }));
    }
  }

  async function playCatalogMix(mix, button) {
    button.disabled = true;
    catalogStatus.className = "status";
    catalogStatus.hidden = false;
    catalogStatus.textContent = t("fy.mixLoading", { title: mix.title });
    try {
      const tracks = await api.streamMix(mix.id, mix.options);
      if (!tracks.length) throw new Error(t("fy.noCatalog"));
      play(tracks, 0, { shuffle: false, radio: { kind: "catalog", mix_id: mix.id, options: mix.options ?? null } });
      catalogStatus.hidden = true;
    } catch (error) {
      catalogStatus.className = "status error";
      catalogStatus.textContent = errorText(error);
      toast(errorText(error), { error: true });
    } finally {
      button.disabled = false;
    }
  }

  function updateSelection(category, value, multiple, checked) {
    const values = new Set(selected[category] ?? []);
    if (checked) {
      if (!multiple) values.clear();
      values.add(value);
    } else values.delete(value);
    if (values.size) selected[category] = [...values];
    else delete selected[category];
    saveSelection();
  }

  renderTabs();
  return {
    element: screen.element,
    show() {
      if (!cache.has(current)) load(current);
      loadCatalog();
      loadReleases();
    },
    reset() {
      generation += 1;
      catalogGeneration += 1;
      cache.clear();
      catalogLoaded = false;
      catalogGrid.replaceChildren();
      catalogStatus.hidden = true;
      releasesGeneration += 1;
      releasesLoaded = false;
      releasesRow.replaceChildren();
      releasesStatus.hidden = true;
      listOptions = null;
      categories = null;
      selected = readSelection();
      tuning.open = false;
      tuningContent.replaceChildren();
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}

function encodeSelection(selection) {
  const clean = Object.fromEntries(Object.entries(selection).filter(([, values]) => values.length));
  return Object.keys(clean).length ? JSON.stringify(clean) : null;
}

function readSelection() {
  try {
    const value = JSON.parse(localStorage.getItem("mix.filter-selection.v1") ?? "{}");
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    return Object.fromEntries(Object.entries(value)
      .filter(([key, items]) => key.length > 0 && key.length <= 256 && Array.isArray(items))
      .map(([key, items]) => [key, [...new Set(items.filter((item) => typeof item === "string" && item.length <= 256))].slice(0, 32)])
      .filter(([, items]) => items.length));
  } catch { return {}; }
}

function saveSelection() {
  try { localStorage.setItem("mix.filter-selection.v1", JSON.stringify(selected)); }
  catch { /* storage can be disabled */ }
}

function findCategories(value, depth = 0) {
  if (!value || depth > 8 || typeof value !== "object") return [];
  if (Array.isArray(value)) {
    if (value.some((item) => item && typeof item === "object" && Array.isArray(item.options))) return value;
    for (const item of value) {
      const found = findCategories(item, depth + 1);
      if (found.length) return found;
    }
    return [];
  }
  for (const key of ["mix_categories", "categories", "settings"]) {
    if (Array.isArray(value[key])) {
      const found = findCategories(value[key], depth + 1);
      if (found.length) return found;
    }
  }
  for (const item of Object.values(value)) {
    const found = findCategories(item, depth + 1);
    if (found.length) return found;
  }
  return [];
}
