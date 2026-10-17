import { mixName, mixColors, mixKey, moodMatch } from "../selena.js";
import { store, subscribe, own } from "../store.js";
import { h } from "../dom.js";
import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { albumCard } from "../album-card.js";
import { icon } from "../icons.js";
import { openAlbum } from "../navigation.js";
import { errorText, t, addStrings } from "../i18n.js";
import { paintStatus, watchSegmented } from "../ui-parts.js";

addStrings({ ru: { "fy.selena": "От Selena", "fy.personal": "Персональная подборка", "fy.personalNote": "По вашим прослушиваниям", "fy.energy": "Энергия", "fy.calm": "Спокойствие", "fy.rain": "Грусть", "fy.sun": "Радость", "fy.love": "Любовь", "fy.moodFallback": "Отдельный микс для этого настроения пока недоступен — играет Selena." }, en: { "fy.selena": "From Selena", "fy.personal": "Personal selection", "fy.personalNote": "Based on what you play", "fy.energy": "Energy", "fy.calm": "Calm", "fy.rain": "Melancholy", "fy.sun": "Joy", "fy.love": "Love", "fy.moodFallback": "There is no separate mix for this mood yet — playing Selena." } });
addStrings({ ru: { "fy.expand": "Все миксы", "fy.collapse": "Свернуть", "fy.tracks": "Треки подборки", "fy.pinned": "Закреплённые миксы", "fy.pin": "Закрепить микс", "fy.unpin": "Открепить микс", "fy.pinStorage": "Не удалось сохранить закреплённые миксы.", "fy.previous":"Предыдущие карточки", "fy.next":"Следующие карточки" }, en: { "fy.expand": "All mixes", "fy.collapse": "Collapse", "fy.tracks": "Selection tracks", "fy.pinned": "Pinned mixes", "fy.pin": "Pin mix", "fy.unpin": "Unpin mix", "fy.pinStorage": "Pinned mixes could not be saved.", "fy.previous":"Previous cards", "fy.next":"Next cards" } });
const MOODS = [
  ["energy",["энерг","бодр","energy","energetic"]], ["calm",["спокой","calm","relax"]], ["rain",["груст","печал","sad","melanch"]], ["sun",["радост","весел","joy","happy"]], ["love",["любов","романт","love","romance"]],
];
const MOOD_GLYPHS = { energy: "bolt", calm: "moon", rain: "rain", sun: "smile", love: "heart-on" };
const SOURCES = {
  mix: { label: "fy.mix", load: () => api.mix() },
  recs: { label: "fy.recs", load: () => api.recommendations() },
};

/** VK's personal mix and recommendations, one tab each, loaded on first look. */
/** A stable colour family per mix, so each card keeps its look between visits. */
function mixHue(text) {
  let hash = 0;
  for (const char of text) hash = (hash * 31 + char.codePointAt(0)) >>> 0;
  return hash % 5;
}

export function createForYouView({ play, onTrackAction, toast }) {
  let current = "mix";
  const cache = new Map(); // source -> tracks
  let generation = 0;
  let sessionGeneration = 0;
  let categories = null;
  let tuningRequest = null;
  let catalog = [];
  let listSource = { kind: "personal", options: null };
  let activeMix = null;
  let pinStorage = null;
  let pinned = [];
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
  watchSegmented(tabs);
  const refresh = h("button", { class: "btn", type: "button", onclick: () => { refreshCurrent(); loadCatalog(true); loadReleases(true); } }, icon("refresh", 14), t("fy.refresh"));
  const tuningContent = h("div", { class: "mix-tuning-content glass strong" });
  const tuning = h("details", { class: "mix-tuning", ontoggle: (event) => {
    if (event.target.open) loadTuning();
  } }, h("summary", { text: t("fy.tune") }), tuningContent);
  // A shelf shows only with something to show: its items, or one line saying
  // why there are none. An empty answer hides it altogether.
  const catalogStatus = h("div", { class: "status", role: "status", hidden: true });
  const catalogGrid = h("div", { class: "mix-grid" });
  const catalogTitle = h("h2", { class: "shelf-title", text: t("fy.catalog") });
  const expand = h("button", { class: "btn shelf-expand", type: "button", "aria-expanded": "false", text: t("fy.expand"), onclick: () => {
    const expanded = expand.getAttribute("aria-expanded") !== "true";
    expand.setAttribute("aria-expanded", String(expanded));
    expand.textContent = t(expanded ? "fy.collapse" : "fy.expand");
    catalogGrid.classList.toggle("expanded", expanded);
  } });
  const catalogShelf = h("section", { class: "mix-catalog", hidden: true }, h("div", { class: "shelf-heading" }, catalogTitle, h("div",{class:"shelf-tools"},expand,carouselControls(catalogGrid))), catalogStatus, catalogGrid);
  const releasesStatus = h("div", { class: "status", role: "status", hidden: true });
  const releasesRow = h("div", { class: "release-row" });
  const releasesTitle = h("h2", { class: "shelf-title", text: t("fy.releases") });
  const releasesShelf = h("section", { class: "mix-catalog new-releases", hidden: true }, releasesTitle, releasesStatus, releasesRow);

  const personalButton = h("button", { class: "selena-card selena-personal", type: "button", onclick: () => startMood(null, personalButton) },
    h("span", { class: "selena-kicker", text: t("fy.personal") }), h("strong", { text: "Selena" }), h("span", { class: "selena-subtitle", text: t("fy.personalNote") }), h("span", { class: "selena-play", "aria-hidden": "true" }, icon("play",18)));
  mixColors("common#|Selena").forEach((color,index) => personalButton.style.setProperty(`--mix-c${index}`,color));
  const pinnedGrid = h("div", { class: "mix-grid pinned-grid" });
  const pinnedShelf = h("section", { class: "pinned-shelf", hidden: true }, h("h2", { class: "shelf-title", text: t("fy.pinned") }), pinnedGrid);
  const moodButtons = MOODS.map(([id,aliases]) => {
    const button = h("button", { class: `selena-card selena-mood mood-${id}`, type: "button", onclick: () => startMood(aliases,button) }, h("span", { class: "mood-symbol", "aria-hidden": "true" }, icon(MOOD_GLYPHS[id],20)), h("span", { class: "selena-kicker", text: t("fy.selena") }), h("strong", { text: t(`fy.${id}`) }), h("span", { class: "selena-play", "aria-hidden": "true" },icon("play",16)));
    return button;
  });
  const moodRow = h("div", { class: "selena-row" },personalButton,...moodButtons);
  const selenaShelf = h("section", { class: "selena-shelf" },h("div",{class:"shelf-heading"},h("h2", { class: "shelf-title", text: t("fy.selena") }),carouselControls(moodRow)),moodRow);

  const screen = createTracksScreen({
    title: t("fy.title"),
    hooks: {
      emptyText: t("fy.empty"),
      onPlay: (tracks, index, options) =>
        play(tracks, index, current === "mix" ? { ...options, radio: listSource } : options),
      onAction: onTrackAction,
      headerExtra: h("div", { class: "set-inline" }, tabs, tuning, refresh),
    },
  });
  // Shelves and tracks share one vertical scroll surface.
  screen.element.classList.add("discovery-view");
  const jumpTracks = h("button", { class: "btn", type: "button", text: t("fy.tracks"), onclick: () => screen.list.scrollToRows() });
  screen.element.querySelector(".vh-actions").append(jumpTracks);
  const shelves = h("div", { class: "fy-shelves" }, selenaShelf, pinnedShelf, catalogShelf, releasesShelf, h("h2", { class: "discovery-tracks-title shelf-title", text: t("fy.tracks") }));
  screen.list.setHeader(shelves);

  function renderTabs() {
    for (const button of tabs.querySelectorAll("button")) button.setAttribute("aria-selected", String(button.dataset.source === current));
    tuning.hidden = current !== "mix";
    paintShelves();
  }

  // What each shelf holds, so showing and hiding stays in one place.
  const shelfState = { catalog: "empty", releases: "empty" };
  function paintShelves() {
    selenaShelf.hidden = current !== "mix";
    pinnedShelf.hidden = current !== "mix" || !pinned.length;
    expand.hidden = shelfState.catalog !== "items";
    catalogShelf.classList.toggle("inline-error",shelfState.catalog === "error");
    catalogShelf.hidden = current !== "mix" || shelfState.catalog === "empty";
    releasesShelf.hidden = shelfState.releases === "empty";
  }

  /** One compact line: the reason and a link to try again. */
  function paintShelfError(status, title, error, retry) {
    paintStatus(status, errorText(error), { error: true, extra: "inline" });
    status.append(h("button", { class: "link-text retry-link", type: "button", text: t("fy.retryTune"), onclick: retry }));
    title.hidden = true;
  }

  async function load(id, { force = false } = {}) {
    const mine = ++generation;
    const options = id === "mix" ? encodeSelection(selected) : null;
    const cacheKey = id === "mix" ? `mix:${options ?? "plain"}` : id;
    refresh.disabled = false;
    if (!force && cache.has(cacheKey)) {
      if (id === "mix") { listOptions = options; listSource = { kind: "personal", options }; activeMix=options?null:"common#"; paintActive(); }
      screen.clearStatus();
      screen.setTracks(cache.get(cacheKey));
      return;
    }
    const hadTracks = screen.list.length > 0;
    if (!hadTracks) screen.setTracks([]);
    if (!hadTracks) screen.setStatus(t("loading"));
    refresh.disabled = true;
    try {
      const tracks = id === "mix" ? await api.mix(options) : await SOURCES[id].load();
      if (mine !== generation) return;
      cache.set(cacheKey, tracks);
      if (id === "mix") { listOptions = options; listSource = { kind: "personal", options }; activeMix=options?null:"common#"; paintActive(); }
      screen.clearStatus();
      screen.setTracks(tracks);
    } catch (error) {
      if (mine !== generation) return;
      if (!hadTracks) screen.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    } finally {
      if (mine === generation) refresh.disabled = false;
    }
  }

  async function refreshCurrent() {
    const mine=++generation;
    const source={...listSource};
    const id=current;
    refresh.disabled=true;
    try {
      const tracks=id!=="mix" ? await SOURCES[id].load() : source.kind==="catalog" ? await api.streamMix(source.mix_id,source.options) : await api.mix(source.options);
      if(mine!==generation)return;
      if(id!=="mix")cache.set(id,tracks);
      else if(source.kind==="personal")cache.set(`mix:${source.options??"plain"}`,tracks);
      screen.clearStatus();screen.setTracks(tracks);
    }catch(error){if(mine===generation)toast(errorText(error),{error:true});}
    finally{if(mine===generation)refresh.disabled=false;}
  }

  function select(id) {
    current = id;
    renderTabs();
    load(id);
  }

  async function loadTuning() {
    if (categories) return;
    if (tuningRequest) return tuningRequest;
    const request = fillTuning();
    tuningRequest = request;
    try { await request; } finally { if (tuningRequest === request) tuningRequest = null; }
  }

  async function fillTuning() {
    const mine = sessionGeneration;
    tuningContent.replaceChildren(h("div", { class: "status inline", text: t("loading") }));
    try {
      const settings = await api.mixSettings();
      if (mine !== sessionGeneration) return;
      categories = findCategories(settings);
      tuningContent.replaceChildren();
      if (!categories.length) {
        tuningContent.replaceChildren(h("div", { class: "status inline", text: t("fy.noTuning") }));
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
      if (mine !== sessionGeneration) return;
      categories = null;
      tuningContent.replaceChildren(h("div", { class: "status inline error", text: errorText(error) }),
        h("button", { class: "btn", type: "button", text: t("fy.retryTune"), onclick: loadTuning }));
    }
  }

  async function loadCatalog(force = false) {
    if (catalogLoaded && !force) return;
    const mine = ++catalogGeneration;
    // No placeholder: a shelf that turns out empty must not flash a panel.
    // A retry keeps its error line, with the link disabled, until the answer.
    catalogStatus.querySelector(".retry-link")?.setAttribute("disabled", "");
    try {
      const mixes = await api.catalogMixes();
      if (mine !== catalogGeneration) return;
      catalog = mixes;
      catalogGrid.replaceChildren(...mixes.map(renderMix));
      const visible = mixes;
      catalogLoaded = true;
      paintStatus(catalogStatus, "");
      catalogTitle.hidden = false;
      shelfState.catalog = visible.length ? "items" : "empty";
    } catch (error) {
      if (mine !== catalogGeneration) return;
      catalogGrid.replaceChildren();
      paintShelfError(catalogStatus, catalogTitle, error, () => loadCatalog(true));
      shelfState.catalog = "error";
    }
    paintShelves();
  }

  async function loadReleases(force = false) {
    if (releasesLoaded && !force) return;
    const mine = ++releasesGeneration;
    releasesStatus.querySelector(".retry-link")?.setAttribute("disabled", "");
    try {
      const albums = await api.newReleases();
      if (mine !== releasesGeneration) return;
      // A release VK sends without a title still gets a card.
      releasesRow.replaceChildren(...albums.map((album) =>
        albumCard(album.title?.trim() ? album : { ...album, title: t("album.untitled") }, () => openAlbum(album))));
      releasesLoaded = true;
      paintStatus(releasesStatus, "");
      releasesTitle.hidden = false;
      shelfState.releases = albums.length ? "items" : "empty";
    } catch (error) {
      if (mine !== releasesGeneration) return;
      releasesRow.replaceChildren();
      paintShelfError(releasesStatus, releasesTitle, error, () => loadReleases(true));
      shelfState.releases = "error";
    }
    paintShelves();
  }

  async function startMood(aliases,button) {
    const mine = ++generation;
    refresh.disabled = false;
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
    try {
      let options = null;
      if (aliases) {
        // Tuned catalogue cards carry the authoritative mix_options.
        const tuned = catalog.find(mix => mix.options && moodMatch(`${mix.title} ${mix.subtitle}`, aliases));
        if (tuned) {
          await playCatalogMix(tuned, button);
          return;
        }
        await loadTuning();
        if (mine !== generation) return;
        const category = (categories ?? []).find((item)=> /mood|настро/i.test(String(item.type ?? item.id ?? "")+String(item.title ?? "")));
        const option = category?.options?.find((item)=>moodMatch(typeof item === "string" ? item : item.title ?? item.name ?? item.text ?? item.id ?? "", aliases));
        if (option != null) {
          const value = typeof option === "string" ? option : String(option.id ?? option.option_id ?? option.value ?? option.key ?? option.title ?? option.name);
          options = JSON.stringify({ ...selected, [category.type ?? category.id ?? category.title]: [value] });
        } else toast(t("fy.moodFallback"));
      }
      const tracks = await api.mix(options);
      if (mine !== generation) return;
      if (!tracks.length) throw new Error(t("fy.empty"));
      current = "mix";
      renderTabs();
      listOptions = options;
      listSource = { kind: "personal", options };
      activeMix = aliases ? button.className : "common#";
      screen.clearStatus(); screen.setTracks(tracks);
      await play(tracks,0,{ radio: { kind: "personal", options }, shuffle: false });
    } catch (error) { if (mine === generation) toast(errorText(error), { error: true }); }
    finally { button.disabled = false; button.removeAttribute("aria-busy"); }
  }

  async function playCatalogMix(mix, button) {
    const mine = ++generation;
    refresh.disabled = false;
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
    paintStatus(catalogStatus, t("fy.mixLoading", { title: mix.title }), { extra: "inline" });
    try {
      const tracks = await api.streamMix(mix.id, mix.options);
      if (mine !== generation) return;
      if (!tracks.length) throw new Error(t("fy.noCatalog"));
      // The plain personal stream is the same radio as the "My mix" tab.
      const personal = mix.id === "common" && !mix.options;
      const source = personal ? { kind: "personal", options: null } : { kind: "catalog", mix_id: mix.id, options: mix.options ?? null };
      current = "mix";
      listSource = source;
      listOptions = mix.options ?? null;
      activeMix = mixKey(mix);
      renderTabs();
      screen.clearStatus();
      screen.setTracks(tracks);
      paintActive();
      await play(tracks, 0, { shuffle: false, radio: source });
      paintStatus(catalogStatus, "");
    } catch (error) {
      if (mine !== generation) return;
      paintStatus(catalogStatus, errorText(error), { error: true, extra: "inline" });
      toast(errorText(error), { error: true });
    } finally {
      button.disabled = false;
      button.removeAttribute("aria-busy");
    }
  }

  function renderMix(mix) {
    const key = mixKey(mix);
    const glyph = /настро|mood/i.test(mix.section_title ?? "") ? "sparkles" : /жанр|genre/i.test(mix.section_title ?? "") ? "album" : "radio";
    const button = h("button", { class: `mix-tile mobile-mix hue-${mixHue(key)}`, type: "button", "data-mix-key": key, "aria-pressed": String(activeMix === key), onclick: () => playCatalogMix(mix, button) },
      h("span", { class: "mix-symbol", "aria-hidden": "true" }, icon(glyph,23)),
      h("span", { class: "mix-text" },
        mix.section_title ? h("span", { class: "mix-tile-section", text: mixName(mix.section_title) }) : null,
        h("span", { class: "mix-tile-title", text: mixName(mix.title) }),
        mix.subtitle ? h("span", { class: "mix-tile-subtitle", text: mixName(mix.subtitle) }) : null),
      h("span", { class: "mix-play", "aria-hidden": "true" }, icon("play",14)));
    mixColors(`${key}|${mixName(mix.title)}`).forEach((color,index) => button.style.setProperty(`--mix-c${index}`,color));
    const isPinned = pinned.some(item => mixKey(item) === key);
    const pin = h("button", { class: "mix-pin icon-btn", type: "button", "aria-label": t(isPinned ? "fy.unpin" : "fy.pin"), "aria-pressed": String(isPinned), disabled: !pinStorage, onclick: () => {
      pinned = isPinned ? pinned.filter(item => mixKey(item) !== key) : [...pinned,mix].slice(-32);
      if (!pinStorage) return;
      try { localStorage.setItem(pinStorage,JSON.stringify(pinned)); } catch { toast(t("fy.pinStorage"),{error:true}); }
      renderPinned();
      catalogGrid.replaceChildren(...catalog.map(renderMix));
    } },icon("pin",14));
    return h("article", { class: "mix-card" },button,pin);
  }

  function paintActive() {
    for (const button of shelves.querySelectorAll("[data-mix-key]")) button.setAttribute("aria-pressed",String(button.dataset.mixKey === activeMix));
  }

  function renderPinned() {
    pinnedGrid.replaceChildren(...pinned.map(renderMix));
    paintShelves();
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

  function saveSelection() {
    try { localStorage.setItem("mix.filter-selection.v1", JSON.stringify(selected)); }
    catch { /* storage can be disabled */ }
  }

  const syncPins = session => {
    const key = session.signed_in && session.profile?.id ? `pm.mixes.pinned.v1.${session.profile.id}` : null;
    if (pinStorage === key) return;
    pinStorage = key;
    pinned = key ? readPins(key) : [];
    renderPinned();
    catalogGrid.replaceChildren(...catalog.map(renderMix));
  };
  subscribe("session", syncPins);
  syncPins(store.session);
  renderTabs();
  renderPinned();
  return {
    element: screen.element,
    show() {
      if (listSource.kind !== "catalog" || !screen.list.length) load(current);
      loadCatalog();
      loadReleases();
    },
    reset() {
      sessionGeneration += 1;
      generation += 1;
      catalogGeneration += 1;
      cache.clear();
      catalogLoaded = false;
      catalogGrid.replaceChildren();
      paintStatus(catalogStatus, "");
      catalogTitle.hidden = false;
      releasesGeneration += 1;
      releasesLoaded = false;
      releasesRow.replaceChildren();
      paintStatus(releasesStatus, "");
      releasesTitle.hidden = false;
      shelfState.catalog = "empty";
      shelfState.releases = "empty";
      paintShelves();
      listOptions = null;
      categories = null;
      tuningRequest = null;
      catalog = [];
      listSource = { kind: "personal", options: null };
      activeMix = null;
      selected = readSelection();
      tuning.open = false;
      tuningContent.replaceChildren();
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}

function carouselControls(scroller) {
 const move = direction => scroller.scrollBy({left:direction*Math.max(200,scroller.clientWidth*0.75),behavior:matchMedia("(prefers-reduced-motion:reduce)").matches?"instant":"smooth"});
 const previous=h("button",{class:"icon-btn carousel-prev",type:"button","aria-label":t("fy.previous"),onclick:()=>move(-1)},icon("chevron",16));
 const next=h("button",{class:"icon-btn carousel-next",type:"button","aria-label":t("fy.next"),onclick:()=>move(1)},icon("chevron",16));
 const element=h("div",{class:"carousel-controls"},previous,next);
 const render=()=>{const has=scroller.scrollWidth>scroller.clientWidth+2;element.hidden=!has;previous.disabled=scroller.scrollLeft<=2;next.disabled=scroller.scrollLeft+scroller.clientWidth>=scroller.scrollWidth-2;};
 const resize=new ResizeObserver(render);resize.observe(scroller);
 const mutation=new MutationObserver(render);mutation.observe(scroller,{childList:true,attributes:true,attributeFilter:["class"]});
 scroller.addEventListener("scroll",render,{passive:true});
 own(()=>{resize.disconnect();mutation.disconnect();scroller.removeEventListener("scroll",render);});
 return element;
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


function readPins(key) {
  try {
    const items=JSON.parse(localStorage.getItem(key) ?? "[]");
    if (!Array.isArray(items)) return [];
    return [...new Map(items.filter(item => item && typeof item.id === "string" && item.id.length <= 256 && typeof item.title === "string" && item.title.length <= 256 && (item.options == null || typeof item.options === "string" && item.options.length <= 4096)).map(item => {const clean={id:item.id,title:item.title,options:item.options??null,subtitle:typeof item.subtitle==="string"?item.subtitle.slice(0,512):null};return [mixKey(clean),clean];})).values()].slice(-32);
  } catch { return []; }
}
