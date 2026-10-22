// Touch gestures on the rows of a track list, the way the iOS TrackRow behaves:
//
//   long press   -> the track's menu (a bottom action sheet), with a haptic tick
//   swipe right  -> Play next; keep going and it becomes Play last (the two
//                   leading swipe actions of TrackRow)
//   swipe left   -> like / unlike
//
// Rows are recreated while the list scrolls, so nothing is stored on them: the
// gesture lives here, in one set of listeners on the list, and reports through
// bubbling events (`pm-row-longpress`, `pm-row-swipe`) that the list forwards to
// its screen. Vertical movement belongs to the browser (rows are touch-action:
// pan-y), so scrolling is never fought.

import { icon } from "../icons.js";
import { t } from "../i18n.js";
import { isPhone, haptic } from "./layout.js";

const SLOP = 8;
const LONG_PRESS_MS = 480;
const SWIPE_START = 10;
const COMMIT = 72;
const FAR = 176;
const MAX_PULL = 240;

let swallowClickUntil = 0;
let lastPointerType = "mouse";

function underlay(side) {
  const glyph = side === "lead" ? "play-next" : "heart";
  const bg = document.createElement("span");
  bg.className = "swipe-bg";
  bg.dataset.side = side;
  bg.setAttribute("aria-hidden", "true");
  const ico = document.createElement("span");
  ico.className = "swipe-ico";
  ico.append(icon(glyph, 20));
  const label = document.createElement("span");
  label.className = "swipe-label";
  bg.append(ico, label);
  return bg;
}

export function attachRowGestures(list) {
  let gesture = null;

  const rowAt = (index) => list.querySelector(`.row[data-index="${index}"]`);

  function end() {
    if (!gesture) return;
    clearTimeout(gesture.timer);
    const { row, bg } = gesture;
    if (gesture.mode === "swipe") {
      row.removeAttribute("data-swipe");
      row.removeAttribute("data-stage");
      row.style.removeProperty("--sx");
      bg?.remove();
    }
    gesture = null;
  }

  function stageFor(dx) {
    const distance = Math.abs(dx);
    if (distance >= FAR && dx > 0) return "far";
    return distance >= COMMIT ? "commit" : "near";
  }

  function paint(g, dx) {
    const side = dx > 0 ? "lead" : "trail";
    if (!g.bg || g.bg.dataset.side !== side) {
      g.bg?.remove();
      g.bg = underlay(side);
      // The underlay sits behind the row in the list (the row itself slides over it).
      g.bg.style.setProperty("top", `${g.row.offsetTop}px`);
      g.bg.style.setProperty("height", `${g.row.offsetHeight}px`);
      g.row.parentElement.prepend(g.bg);
    }
    // Past the far mark the row follows the finger more slowly.
    const pulled = Math.sign(dx) * Math.min(MAX_PULL, Math.abs(dx) > FAR ? FAR + (Math.abs(dx) - FAR) * 0.4 : Math.abs(dx));
    g.row.dataset.swipe = side;
    g.row.style.setProperty("--sx", `${pulled}px`);
    const stage = stageFor(dx);
    if (stage !== g.stage) {
      if (stage !== "near") haptic(8);
      g.stage = stage;
    }
    g.row.dataset.stage = stage;
    g.bg.classList.toggle("near", stage === "near");
    if (side === "lead" && stage === "far") g.bg.dataset.far = ""; else delete g.bg.dataset.far;
    const liked = g.row.dataset.liked === "1";
    g.bg.querySelector(".swipe-label").textContent = side === "trail"
      ? t(liked ? "like.remove" : "like.add")
      : t(stage === "far" ? "playLast" : "playNext");
    const glyph = side === "trail" ? (liked ? "heart-on" : "heart") : stage === "far" ? "queue-add" : "play-next";
    const ico = g.bg.querySelector(".swipe-ico");
    if (ico.dataset.glyph !== glyph) {
      ico.dataset.glyph = glyph;
      ico.replaceChildren(icon(glyph, 20));
    }
  }

  list.addEventListener("pointerdown", (event) => {
    lastPointerType = event.pointerType;
    if (!isPhone() || (event.pointerType === "mouse" && event.button !== 0)) return;
    const row = event.target.closest?.(".row");
    if (!row || !list.contains(row)) return;
    end();
    gesture = { id: event.pointerId, row, index: Number(row.dataset.index), x: event.clientX, y: event.clientY, mode: "pending", stage: "near", bg: null, timer: 0 };
    // The ⋯ and heart buttons are plain taps.
    if (event.target.closest(".icon-btn")) { gesture.mode = "tap"; return; }
    const g = gesture;
    g.timer = setTimeout(() => {
      if (gesture !== g || g.mode !== "pending") return;
      g.mode = "long";
      swallowClickUntil = Date.now() + 700;
      haptic(14);
      const box = g.row.getBoundingClientRect();
      g.row.dispatchEvent(new CustomEvent("pm-row-longpress", { bubbles: true, detail: { x: g.x, y: box.bottom } }));
    }, LONG_PRESS_MS);
  });

  list.addEventListener("pointermove", (event) => {
    const g = gesture;
    if (!g || event.pointerId !== g.id) return;
    const dx = event.clientX - g.x;
    const dy = event.clientY - g.y;
    if (g.mode === "pending") {
      if (Math.hypot(dx, dy) > SLOP) clearTimeout(g.timer);
      if (Math.abs(dx) > SWIPE_START && Math.abs(dx) > Math.abs(dy) * 1.3) {
        g.mode = "swipe";
        try { list.setPointerCapture(event.pointerId); } catch { /* the pointer may already be gone */ }
      } else if (Math.abs(dy) > SLOP * 2) {
        end(); // a scroll: the browser has it
        return;
      }
    }
    if (g.mode === "swipe") {
      event.preventDefault?.();
      paint(g, dx);
    }
  });

  function finish(event, cancelled) {
    const g = gesture;
    if (!g || event.pointerId !== g.id) return;
    if (g.mode === "swipe" && !cancelled) {
      const dx = event.clientX - g.x;
      swallowClickUntil = Date.now() + 500;
      const stage = stageFor(dx);
      const row = rowAt(g.index) ?? g.row;
      end();
      if (stage !== "near") {
        row.dispatchEvent(new CustomEvent("pm-row-swipe", { bubbles: true, detail: { dir: dx > 0 ? "lead" : "trail", far: stage === "far" } }));
      }
      return;
    }
    end();
  }
  list.addEventListener("pointerup", (event) => finish(event, false));
  list.addEventListener("pointercancel", (event) => finish(event, true));

  // A finger that did a long press or a swipe must not also "tap" the row.
  list.addEventListener("click", (event) => {
    if (Date.now() < swallowClickUntil) {
      event.stopImmediatePropagation();
      event.preventDefault();
    }
  }, true);
  // Android reports a long press as a context menu as well; the timer above already handled it.
  list.addEventListener("contextmenu", (event) => {
    if (isPhone() && lastPointerType !== "mouse") {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }, true);
}
