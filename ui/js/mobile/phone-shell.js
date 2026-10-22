// The phone's chrome: a bottom dock with four root tabs (Home, Library, Search,
// Profile), an iOS-style navigation bar and a push stack per tab. It is built
// from the same view modules the sidebar shows; nothing here loads or changes
// data. The dock copies the iOS app's custom dock: a glass capsule with three
// tabs and Search as a round button beside it.
//
// Navigation model: every tab owns a stack of pages { id, payload }. Tapping a
// tab shows its stack as it was left; tapping the active tab pops to its root
// (and scrolls to the top once there). `pop()` is what Back does, on screen and
// from the Android button.

import { h } from "../dom.js";
import { icon } from "../icons.js";
import { t } from "../i18n.js";
import { haptic } from "./layout.js";
import "./strings.js";

export const TABS = [
  { id: "home", root: "home", label: "tab.home", glyph: "house" },
  { id: "library", root: "libraryHub", label: "tab.library", glyph: "music-list" },
  { id: "profile", root: "profile", label: "tab.profile", glyph: "person-circle" },
  { id: "search", root: "search", label: "tab.search", glyph: "search" },
];

/** Where each view lives when something asks to "show" it by name (shortcuts, deep links). */
const TAB_OF = {
  home: "home", forYou: "home",
  libraryHub: "library", library: "library", playlists: "library", smart: "library", downloads: "library", friends: "library", history: "library",
  profile: "profile", settings: "profile", stats: "profile",
  search: "search",
};

/** Inline titles of pages whose own heading is not a plain h1. */
const STATIC_TITLES = { home: "tab.home" };

export function createPhoneShell({ main, views, display, activeId }) {
  const tabState = Object.fromEntries(TABS.map((tab) => [tab.id, { stack: [], current: { id: tab.root, payload: null } }]));
  let tab = "home";
  let active = false;

  // ------------------------------------------------------------------ dock
  const tabButton = (item) => h("button", {
    type: "button", class: `dock-tab${item.id === "search" ? " dock-search" : ""}`, "data-tab": item.id, "aria-label": t(item.label),
    onclick: () => selectTab(item.id),
  },
  h("span", { class: "dock-ico" }, icon(item.glyph, item.id === "search" ? 22 : 20)),
  h("span", { class: "dock-label", text: t(item.label) }));
  const dockButtons = TABS.map(tabButton);
  const dock = h("nav", { class: "dock", "aria-label": t("tab.nav") },
    h("div", { class: "dock-tabs glass" }, dockButtons.filter((b) => b.dataset.tab !== "search")),
    h("div", { class: "dock-searchwrap glass" }, dockButtons.find((b) => b.dataset.tab === "search")));

  // ---------------------------------------------------------------- navbar
  const back = h("button", { type: "button", class: "nav-back", "aria-label": t("nav.back"), hidden: true, onclick: () => pop() }, icon("back", 22));
  const title = h("div", { class: "nav-title", "aria-hidden": "true" });
  const actions = h("div", { class: "nav-actions" });
  const navbar = h("header", { class: "navbar" }, back, title, actions);

  function titleOf(id) {
    if (STATIC_TITLES[id]) return t(STATIC_TITLES[id]);
    const element = views[id]?.element;
    const heading = [...(element?.querySelectorAll("h1") ?? [])].find((node) => node.offsetParent && node.textContent.trim());
    return heading?.textContent.trim() ?? "";
  }

  // The inline title appears when the large one has scrolled under the bar. Pages
  // with no large title (Home) show it at all times.
  let frame = 0;
  function syncTitle() {
    frame = 0;
    const id = activeId();
    const element = views[id]?.element;
    if (!element) return;
    const name = titleOf(id);
    title.textContent = name;
    const heading = [...element.querySelectorAll("h1")].find((node) => node.offsetParent);
    const always = !heading || STATIC_TITLES[id];
    let shown = always;
    if (!always) {
      const bar = navbar.getBoundingClientRect();
      shown = heading.getBoundingClientRect().bottom < bar.bottom + 6;
    }
    navbar.classList.toggle("has-title", !!shown && !!name);
    navbar.classList.toggle("is-scrolled", element.scrollTop > 2);
  }
  const scheduleTitle = () => { if (!frame) frame = requestAnimationFrame(syncTitle); };
  // Scroll events do not bubble; the capture phase sees every scroller of the pages.
  main.addEventListener("scroll", scheduleTitle, { capture: true, passive: true });

  function renderNavActions(id) {
    const items = views[id]?.navActions?.() ?? [];
    actions.replaceChildren(...items);
  }

  // ------------------------------------------------------------- the stacks
  const state = () => tabState[tab];

  function render(direction = "") {
    const { current, stack } = state();
    display(current.id, direction);
    for (const button of dockButtons) {
      if (button.dataset.tab === tab) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    back.hidden = stack.length === 0;
    document.documentElement.dataset.depth = String(stack.length);
    renderNavActions(current.id);
    syncTitle();
  }

  function selectTab(next, { fromUser = true } = {}) {
    if (fromUser) haptic(6);
    if (next === tab) {
      // Tapping the tab you are on: back to its first page, or to the top of it.
      if (state().stack.length) {
        const root = state().stack[0];
        state().stack = [];
        state().current = root;
        render("back");
        views[root.id].root?.();
      } else {
        const element = views[state().current.id]?.element;
        element?.scrollTo?.({ top: 0, behavior: "smooth" });
      }
      return;
    }
    tab = next;
    render();
  }

  function push(id, payload = null) {
    const s = state();
    s.stack.push(s.current);
    s.current = { id, payload };
    render("push");
    views[id].open?.(payload);
    // A fresh page starts at its top.
    const element = views[id]?.element;
    if (element) element.scrollTop = 0;
  }

  function pop() {
    const s = state();
    if (!s.stack.length) return false;
    s.current = s.stack.pop();
    render("back");
    if (s.current.payload) views[s.current.id].open?.(s.current.payload);
    return true;
  }

  /** Shows a view by name (the "/" shortcut, programmatic navigation): its tab, and the page itself if it is not a root. */
  function showView(id) {
    const wanted = TAB_OF[id] ?? "home";
    const root = TABS.find((item) => item.id === wanted).root;
    selectTab(wanted, { fromUser: false });
    if (id !== root && TAB_OF[id] && state().current.id !== id) push(id);
  }

  return {
    dock, navbar, TABS,
    get tab() { return tab; },
    get active() { return active; },
    get depth() { return state().stack.length; },
    selectTab, push, pop, showView, syncTitle,
    /** The phone layout took over (or the app starts in it): show the tab for what is on screen. */
    enter(currentId) {
      active = true;
      const wanted = TAB_OF[currentId] ?? "home";
      if (wanted !== tab) tab = wanted;
      render();
    },
    leave() { active = false; },
    start() {
      active = true;
      tab = "home";
      for (const item of TABS) tabState[item.id] = { stack: [], current: { id: item.root, payload: null } };
      render();
    },
    rerenderActions() { renderNavActions(state().current.id); },
  };
}
