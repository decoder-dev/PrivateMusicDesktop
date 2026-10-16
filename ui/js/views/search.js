import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { searchField, paintStatus, fillPanel, watchSegmented } from "../ui-parts.js";
import { albumCard, artistCard } from "../album-card.js";
import { openAlbum, openArtist } from "../navigation.js";

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
    autocomplete: "off", spellcheck: "false",
    oninput: () => {
      clearTimeout(debounce);
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
    shelfParts[id] = { status, grid };
    panes[id] = h("div", { class: "tab-pane", hidden: true }, status, grid);
  }
  screen.element.append(h("div", { class: "tab-bar" }, tabs), recentHeading, recentList, tracksPane, panes.artists, panes.albums);
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

  async function loadShelf(id) {
    const text = query;
    if (!text) {
      shelfParts[id].grid.replaceChildren();
      setShelfStatus(id, t("search.empty"));
      return;
    }
    if (shelves[id].has(text)) return renderShelf(id, shelves[id].get(text));
    const mine = sequence;
    shelfParts[id].grid.replaceChildren();
    setShelfStatus(id, t("loading"));
    try {
      const items = id === "artists"
        ? await api.searchArtists(text, 0, SHELF)
        : (await api.searchAlbums(text, 0, SHELF)).items;
      shelves[id].set(text, items);
      if (mine !== sequence || tab !== id) return;
      renderShelf(id, items);
    } catch (error) {
      if (mine !== sequence || tab !== id) return;
      setShelfStatus(id, errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function run(raw) {
    const text = raw.trim();
    const mine = ++sequence;
    query = text;
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
      results = page.items;
      saveRecent(text);
      nextOffset = page.next_offset;
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
    try {
      const page = await api.search(query, nextOffset, PAGE);
      if (mine !== sequence) return;
      results = results.concat(page.items);
      nextOffset = page.next_offset;
      screen.setTracks(results);
    } catch (error) {
      toast(errorText(error), { error: true });
    } finally {
      loadingMore = false;
    }
  }

  renderTabs();
  return {
    element: screen.element,
    show() { input.focus(); },
    focus() { input.focus(); input.select(); },
    reset() {
      sequence++;
      results = [];
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
    return [...new Set(value.filter((item) => typeof item === "string").map((item) => item.trim()).filter(Boolean))].slice(0, HISTORY_LIMIT);
  } catch {
    return [];
  }
}
