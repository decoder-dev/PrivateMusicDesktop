import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { albumCard, artistCard } from "../album-card.js";
import { openAlbum, openArtist } from "../navigation.js";

const PAGE = 50;
const SHELF = 30;
const TABS = ["tracks", "artists", "albums"];

export function createSearchView({ play, onTrackAction, toast }) {
  let sequence = 0;
  let nextOffset = null;
  let results = [];
  let query = "";
  let loadingMore = false;
  let debounce = null;
  let tab = "tracks";
  // Artists and albums are asked for when their tab is first shown for a
  // query: query -> list.
  const shelves = { artists: new Map(), albums: new Map() };

  const input = h("input", {
    class: "field", type: "search", placeholder: t("search.placeholder"), "aria-label": t("search.placeholder"),
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

  const screen = createTracksScreen({
    title: t("search.title"),
    hooks: { emptyText: t("search.empty"), onPlay: play, onAction: onTrackAction },
  });
  // The search box lives in the header, next to the title.
  const header = screen.element.querySelector(".view-header");
  header.insertBefore(input, header.querySelector(".meta"));

  const tabs = h("div", { class: "tabs", role: "tablist", "aria-label": t("search.title") },
    TABS.map((id) => h("button", { type: "button", role: "tab", "data-tab": id, onclick: () => select(id), text: t(`search.tab.${id}`) })));
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
  screen.element.append(h("div", { class: "tab-bar" }, tabs), tracksPane, panes.artists, panes.albums);

  screen.list.element.addEventListener("scroll", () => {
    const el = screen.list.element;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 400) loadMore();
  }, { passive: true });

  function renderTabs() {
    for (const button of tabs.children) button.setAttribute("aria-selected", String(button.dataset.tab === tab));
    for (const [id, pane] of Object.entries(panes)) pane.hidden = id !== tab;
    // Play and Shuffle act on the tracks.
    for (const button of header.querySelectorAll(".btn")) button.hidden = tab !== "tracks";
  }

  function select(id) {
    tab = id;
    renderTabs();
    if (id !== "tracks") loadShelf(id);
  }

  function setShelfStatus(id, text, { error = false } = {}) {
    const { status } = shelfParts[id];
    status.textContent = text ?? "";
    status.hidden = !text;
    status.className = `empty-state${error ? " error" : ""}`;
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
      input.value = "";
      shelves.artists.clear();
      shelves.albums.clear();
      select("tracks");
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}
