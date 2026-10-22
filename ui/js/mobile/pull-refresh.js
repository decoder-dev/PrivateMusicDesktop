// Pull to refresh for the screens that load data (Home, Library, History...).
// A drag down from the very top of the screen moves the page with resistance and
// shows an arrow that turns into a spinner; letting go past the mark calls
// `onRefresh()` and holds the page open until its promise settles. Touch only:
// a phone is the only place a page is scrolled with a finger. The browser's own
// overscroll refresh is switched off in CSS (overscroll-behavior).

import { h } from "../dom.js";
import { icon } from "../icons.js";
import { t } from "../i18n.js";
import { isPhone, haptic } from "./layout.js";
import "./strings.js";

const MARK = 64;
const MAX = 120;
const HOLD = 52;

export function attachPullToRefresh(scroller, onRefresh) {
  const label = h("span", { class: "ptr-label" });
  const arrow = h("span", { class: "ptr-arrow", "aria-hidden": "true" }, icon("chevron", 18));
  const spinner = h("span", { class: "spinner", "aria-hidden": "true" });
  const indicator = h("div", { class: "ptr", "aria-hidden": "true" }, arrow, spinner, label);
  scroller.prepend(indicator);

  let startY = 0;
  let startX = 0;
  let pulling = false;
  let refreshing = false;
  let armed = false;

  const set = (px) => scroller.style.setProperty("--pull", `${px}px`);
  function state(next) {
    indicator.dataset.state = next;
    label.textContent = next === "loading" ? t("ptr.loading") : next === "ready" ? t("ptr.release") : t("ptr.pull");
  }
  state("idle");

  function settle() {
    scroller.classList.remove("pulling");
    set(0);
    pulling = false;
    armed = false;
    indicator.dataset.state = "idle";
  }

  scroller.addEventListener("touchstart", (event) => {
    if (!isPhone() || refreshing || event.touches.length !== 1 || scroller.scrollTop > 0) return;
    if (document.querySelector(".sheet-root, .np:not([hidden])")) return;
    startY = event.touches[0].clientY;
    startX = event.touches[0].clientX;
    pulling = true;
    armed = false;
  }, { passive: true });

  scroller.addEventListener("touchmove", (event) => {
    if (!pulling || refreshing) return;
    const dy = event.touches[0].clientY - startY;
    const dx = event.touches[0].clientX - startX;
    if (!armed) {
      // Decide once: a downward drag at the top is ours; anything else is scrolling.
      if (Math.abs(dx) > Math.abs(dy) || dy < -4 || scroller.scrollTop > 0) { pulling = false; return; }
      if (dy < 8) return;
      armed = true;
      scroller.classList.add("pulling");
    }
    if (event.cancelable) event.preventDefault();
    const pull = Math.min(MAX, dy * 0.5);
    set(pull);
    const ready = pull >= MARK;
    if (ready !== (indicator.dataset.state === "ready")) { state(ready ? "ready" : "idle"); if (ready) haptic(8); }
  }, { passive: false });

  async function release() {
    if (!pulling) return;
    if (!armed) { pulling = false; return; }
    const pull = parseFloat(scroller.style.getPropertyValue("--pull")) || 0;
    pulling = false;
    armed = false;
    if (pull < MARK) { settle(); return; }
    refreshing = true;
    scroller.classList.remove("pulling");
    set(HOLD);
    state("loading");
    try {
      await Promise.all([Promise.resolve(onRefresh()).catch(() => {}), new Promise((resolve) => setTimeout(resolve, 450))]);
    } finally {
      refreshing = false;
      settle();
    }
  }
  scroller.addEventListener("touchend", release);
  scroller.addEventListener("touchcancel", () => { if (!refreshing) settle(); });

  return { indicator, get refreshing() { return refreshing; } };
}
