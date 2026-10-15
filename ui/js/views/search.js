import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";

const PAGE = 50;

export function createSearchView({ play, onTrackAction, toast }) {
  let sequence = 0;
  let nextOffset = null;
  let results = [];
  let query = "";
  let loadingMore = false;
  let debounce = null;

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
  screen.element.querySelector(".view-header").insertBefore(input, screen.element.querySelector(".view-header .meta"));

  screen.list.element.addEventListener("scroll", () => {
    const el = screen.list.element;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 400) loadMore();
  }, { passive: true });

  async function run(raw) {
    const text = raw.trim();
    const mine = ++sequence;
    query = text;
    nextOffset = null;
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
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}
