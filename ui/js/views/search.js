import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t, addStrings } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { searchField, paintStatus, fillPanel, watchSegmented } from "../ui-parts.js";
import { albumCard, artistCard } from "../album-card.js";
import { openAlbum, openArtist } from "../navigation.js";

addStrings({ru:{"search.more":"Показать ещё", "search.found":"Найдено треков: {count}"},en:{"search.more":"Show more", "search.found":"Tracks found: {count}"}});
const normalize = raw => raw.trim().replace(/\s+/gu," ");
const uniqueTracks = items => [...new Map(items.map(item => [`${item.owner_id}_${item.track_id}`,item])).values()];
const advancingOffset = (next, current) => Number.isInteger(next) && next > current ? next : null;
const PAGE = 50;
const SHELF = 30;
const TABS = ["tracks", "artists", "albums"];
const HISTORY_KEY = "search.recent.queries.v1";
const HISTORY_LIMIT = 10;

export function createSearchView({ play, onTrackAction, toast }) {
  let sequence = 0;
  let nextOffset = null;
  let results = [];
  let query = "";
  let loadingMore = false;
  let debounce = null;
  let tab = "tracks";
  let recent = readRecent();
  // Artists and albums are asked for when their tab is first shown for a
  // query: query -> list.
  const shelves = { artists: new Map(), albums: new Map() };

  const field = searchField({
    placeholder: t("search.placeholder"), label: t("search.placeholder"),
    autocomplete: "off", spellcheck: "false", maxlength: "120",
    oninput: () => {
      clearTimeout(debounce);
      sequence++; // Invalidate responses immediately, including during debounce.
      nextOffset = null;
      loadingMore = false;
      if (!normalize(input.value)) { run(""); return; }
      debounce = setTimeout(() => run(input.value), 350);
    },
    onkeydown: (event) => {
      if (event.key === "Enter") {
        clearTimeout(debounce);
        run(input.value);
      }
    },
  });
  const input = field.input;

  const screen = createTracksScreen({
    title: t("search.title"),
    hooks: { emptyText: t("search.empty"), art: "search", onPlay: play, onAction: onTrackAction },
  });
  // The search box lives in the header, next to the title.
  const header = screen.element.querySelector(".view-header");
  header.insertBefore(field.element, header.querySelector(".vh-actions"));

  const tabs = h("div", { class: "tabs", role: "tablist", "aria-label": t("search.title") },
    TABS.map((id) => h("button", { type: "button", role: "tab", "data-tab": id, onclick: () => select(id), text: t(`search.tab.${id}`) })));
  watchSegmented(tabs);
  const recentHeading = h("div", { class: "search-recents-head" },
    h("h2", { class: "shelf-title", text: t("search.recent") }),
    h("button", { class: "btn", type: "button", text: t("search.clearRecent"), onclick: clearRecent }));
  const recentList = h("div", { class: "search-recents" });
  // The track list, its status and empty note move into a pane of their own
  // so the other tabs can take their place.
  const tracksPane = h("div", { class: "tab-pane" }, ...[...screen.element.children].filter((child) => child !== header));
  const panes = { tracks: tracksPane };
  const shelfParts = {};
  for (const id of ["artists", "albums"]) {
    const status = h("div", { class: "empty-state", role: "status", hidden: true });
    const grid = h("div", { class: `grid ${id === "artists" ? "artist-grid" : ""}` });
    const more = h("button",{class:"btn search-more",type:"button",hidden:true,text:t("search.more"),onclick:()=>loadShelf(id,true)});
    shelfParts[id] = { status, grid, more };
    panes[id] = h("div", { class: "tab-pane", hidden: true }, status, grid, more);
  }
  screen.element.append(h("div", { class: "tab-bar" }, tabs), recentHeading, recentList, tracksPane, panes.artists, panes.albums);
  const summary=h("div",{class:"search-summary",role:"status",hidden:true});
  tracksPane.prepend(summary);
  renderRecent();

  screen.list.element.addEventListener("scroll", () => {
    const el = screen.list.element;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 400) loadMore();
  }, { passive: true });

  function renderTabs() {
    for (const button of tabs.querySelectorAll("button")) button.setAttribute("aria-selected", String(button.dataset.tab === tab));
    for (const [id, pane] of Object.entries(panes)) pane.hidden = id !== tab;
    // Play and Shuffle act on the tracks.
    header.querySelector(".vh-actions").hidden = tab !== "tracks";
  }

  function renderRecent() {
    const visible = !query && recent.length > 0;
    recentHeading.hidden = !visible;
    recentList.hidden = !visible;
    recentList.replaceChildren(...recent.map((item) => {
      const use = h("button", { class: "search-recent-query", type: "button", onclick: () => {
        input.value = item;
        field.sync();
        run(item);
      } }, icon("history", 14), h("span", { text: item }));
      const remove = h("button", { class: "search-recent-remove", type: "button", title: t("search.removeRecent", { query: item }), "aria-label": t("search.removeRecent", { query: item }), onclick: () => removeRecent(item) }, icon("close", 14));
      return h("div", { class: "search-recent" }, use, remove);
    }));
  }

  function saveRecent(value) {
    const normalized = value.trim();
    if (!normalized) return;
    recent = [normalized, ...recent.filter((item) => item.toLocaleLowerCase() !== normalized.toLocaleLowerCase())].slice(0, HISTORY_LIMIT);
    persistRecent();
    renderRecent();
  }

  function removeRecent(value) {
    recent = recent.filter((item) => item !== value);
    persistRecent();
    renderRecent();
  }

  function clearRecent() {
    recent = [];
    try { localStorage.removeItem(HISTORY_KEY); } catch { /* storage can be disabled */ }
    renderRecent();
  }

  function persistRecent() {
    try { localStorage.setItem(HISTORY_KEY, JSON.stringify(recent)); } catch { /* storage can be disabled */ }
  }

  function select(id) {
    tab = id;
    renderTabs();
    if (id !== "tracks") loadShelf(id);
  }

  function setShelfStatus(id, text, { error = false } = {}) {
    const { status } = shelfParts[id];
    if (text === t("loading")) return paintStatus(status, text, { kind: "cards" });
    status.removeAttribute("aria-busy");
    status.className = `empty-state${error ? " error" : ""}`;
    fillPanel(status, text ?? "", error ? "error" : id === "artists" ? "artist" : "album");
    status.hidden = !text;
  }

  function renderShelf(id, items) {
    const { grid } = shelfParts[id];
    grid.replaceChildren(...items.map((item) => (id === "artists" ? artistCard(item, openArtist) : albumCard(item, openAlbum))));
    setShelfStatus(id, items.length ? "" : t(id === "artists" ? "search.noArtists" : "search.noAlbums"));
  }

  async function loadShelf(id, append = false) {
    const text = query;
    if (!text) {
      shelfParts[id].grid.replaceChildren();
      shelfParts[id].more.hidden=true;
      setShelfStatus(id, t("search.empty"));
      return;
    }
    const cached = shelves[id].get(text);
    if (!append && cached) { shelfParts[id].more.disabled=false; renderShelf(id,cached.items); shelfParts[id].more.hidden=cached.next == null; return; }
    if (append && (shelfParts[id].more.disabled || cached?.next == null)) return;
    const offset=append?cached.next:0;
    shelfParts[id].more.disabled=true;
    shelfParts[id].more.hidden=!append;
    const mine = sequence;
    if (!append) { shelfParts[id].grid.replaceChildren(); setShelfStatus(id, t("loading")); }
    try {
      const page = id === "artists" ? await api.searchArtists(text, offset, SHELF) : await api.searchAlbums(text, offset, SHELF);
      if (mine !== sequence) return;
      const fresh=id==="artists"?page:page.items;
      const key=item=>id==="artists"?String(item.id ?? item.name):`${item.owner_id}_${item.album_id}`;
      const items=[...new Map([...(append?cached.items:[]),...fresh].map(item=>[key(item),item])).values()];
      const next=id==="artists"?(fresh.length===SHELF?offset+SHELF:null):advancingOffset(page.next_offset,offset);
      shelves[id].set(text, {items,next:append && items.length===cached.items.length?null:next});
      if (shelves[id].size > 20) shelves[id].delete(shelves[id].keys().next().value);
      if (tab !== id) return;
      renderShelf(id, items);
      shelfParts[id].more.hidden=shelves[id].get(text).next==null;
      saveRecent(text);
    } catch (error) {
      if (mine !== sequence || tab !== id) return;
      if (!append) setShelfStatus(id, errorText(error), { error: true });
      toast(errorText(error), { error: true });
    } finally { if (mine===sequence) shelfParts[id].more.disabled=false; }
  }

  async function run(raw) {
    const text = normalize(raw);
    const mine = ++sequence;
    summary.hidden=true;
    if (text !== query) { results=[]; screen.setTracks([]); }
    query = text;
    loadingMore = false;
    nextOffset = null;
    renderRecent();
    if (tab !== "tracks") loadShelf(tab);
    if (!text) {
      results = [];
      screen.setTracks([]);
      screen.clearStatus();
      return;
    }
    screen.setStatus(t("loading"));
    try {
      const page = await api.search(text, 0, PAGE);
      if (mine !== sequence) return; // a newer query replaced this one
      results = uniqueTracks(page.items);
      summary.textContent=t("search.found",{count:Number.isFinite(page.total)?page.total:results.length});
      summary.hidden=false;
      saveRecent(text);
      nextOffset = advancingOffset(page.next_offset,0);
      screen.clearStatus();
      screen.setTracks(results);
      if (!results.length) screen.setStatus(t("search.none"));
    } catch (error) {
      if (mine !== sequence) return;
      screen.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function loadMore() {
    if (loadingMore || nextOffset == null || !query) return;
    loadingMore = true;
    const mine = sequence;
    const offset=nextOffset;
    try {
      const page = await api.search(query, offset, PAGE);
      if (mine !== sequence) return;
      const merged=uniqueTracks(results.concat(page.items));
      nextOffset = merged.length>results.length?advancingOffset(page.next_offset,offset):null;
      results=merged;
      screen.setTracks(results);
    } catch (error) {
      if (mine === sequence) toast(errorText(error), { error: true });
    } finally {
      if (mine === sequence) loadingMore = false;
    }
  }

  renderTabs();
  return {
    element: screen.element,
    show() { input.focus(); },
    focus() { input.focus(); input.select(); },
    reset() {
      clearTimeout(debounce);
      loadingMore = false;
      sequence++;
      results = [];
      summary.hidden=true;
      nextOffset = null;
      query = "";
      renderRecent();
      input.value = "";
      field.sync();
      shelves.artists.clear();
      shelves.albums.clear();
      select("tracks");
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}

function readRecent() {
  try {
    const value = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]");
    if (!Array.isArray(value)) return [];
    return [...new Set(value.filter((item) => typeof item === "string").map((item) => normalize(item).slice(0,120)).filter(Boolean))].slice(0, HISTORY_LIMIT);
  } catch {
    return [];
  }
}
