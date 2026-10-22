// Bottom sheets: the phone's replacement for pop-up menus. A sheet rises from
// the bottom edge over a dimmed scrim, follows a finger dragged down from its
// grabber and goes away on a tap outside, a swipe down or the Back button.
// `openActionSheet` is the iOS context-menu / action-sheet look for the item
// lists the desktop shows as pop-up menus (same items, same callbacks).

import { h } from "../dom.js";
import { icon } from "../icons.js";
import { t } from "../i18n.js";
import { afterAnimation } from "../motion.js";
import { onBack } from "./back.js";
import { haptic } from "./layout.js";

const open = []; // newest last

const DISMISS_DISTANCE = 88;
const DISMISS_VELOCITY = 0.55; // px per ms

/** Makes `handle` drag `sheet` downwards; `close()` when let go far or fast enough. */
export function dragToDismiss(handle, sheet, close, { threshold = DISMISS_DISTANCE } = {}) {
  let drag = null;
  handle.addEventListener("pointerdown", (event) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    drag = { id: event.pointerId, y: event.clientY, last: event.clientY, t: performance.now(), v: 0 };
    sheet.classList.add("dragging");
    try { handle.setPointerCapture(event.pointerId); } catch { /* gone already */ }
  });
  handle.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    const now = performance.now();
    const dy = Math.max(0, event.clientY - drag.y);
    drag.v = (event.clientY - drag.last) / Math.max(1, now - drag.t);
    drag.last = event.clientY;
    drag.t = now;
    sheet.style.setProperty("--dy", `${dy}px`);
  });
  const release = (event, cancelled) => {
    if (!drag || event.pointerId !== drag.id) return;
    const dy = Math.max(0, event.clientY - drag.y);
    const fast = drag.v > DISMISS_VELOCITY;
    drag = null;
    sheet.classList.remove("dragging");
    if (!cancelled && (dy > threshold || (fast && dy > 12))) close();
    else sheet.style.setProperty("--dy", "0px");
  };
  handle.addEventListener("pointerup", (event) => release(event, false));
  handle.addEventListener("pointercancel", (event) => release(event, true));
}

/** Opens an empty sheet and gives back its parts. `content` is appended to the scrolling body. */
export function openSheet({ label = "", className = "", content = [], onClose } = {}) {
  let closed = false;
  const grab = h("div", { class: "sheet-grab", "aria-hidden": "true" }, h("i"));
  const body = h("div", { class: "sheet-body" }, ...content);
  const sheet = h("div", { class: `sheet glass strong${className ? ` ${className}` : ""}`, role: "dialog", "aria-modal": "true", "aria-label": label }, grab, body);
  const scrim = h("div", { class: "sheet-scrim" });
  const root = h("div", { class: "sheet-root" }, scrim, sheet);
  // A sheet opened by a long press appears under the finger that is still down: the tap
  // that finger ends with must not press whatever it lands on.
  const openedAt = Date.now();
  root.addEventListener("click", (event) => {
    if (Date.now() - openedAt < 250) { event.stopPropagation(); event.preventDefault(); }
  }, true);

  function close() {
    if (closed) return;
    closed = true;
    const at = open.indexOf(api);
    if (at >= 0) open.splice(at, 1);
    root.classList.add("closing");
    afterAnimation(sheet, () => {
      root.remove();
      if (!open.length && !document.querySelector("dialog[open]")) document.body.classList.remove("overlay-open");
      onClose?.();
    }, 380);
  }

  const api = { element: sheet, body, close, root };
  scrim.addEventListener("click", close);
  dragToDismiss(grab, sheet, close);
  document.body.classList.add("overlay-open");
  document.body.append(root);
  open.push(api);
  sheet.focus({ preventScroll: true });
  return api;
}

export const sheetIsOpen = () => open.length > 0;
export function closeSheets() {
  for (const sheet of [...open].reverse()) sheet.close();
}

onBack(5, () => {
  const top = open[open.length - 1];
  if (!top) return false;
  top.close();
  return true;
});

/**
 * The menu items of the desktop pop-up as an action sheet. Items:
 * { label, action, icon?, items? (drill into a page), disabled?, destructive?, run } or "-".
 */
export function openActionSheet(items, { label = "", onClose } = {}) {
  const trail = [{ title: label, items }];
  let sheet = null;

  function pageOf(entry, index) {
    const list = entry.items.map((item) => {
      if (item === "-") return h("div", { class: "sheet-sep", role: "separator" });
      return h("button", {
        type: "button", role: "menuitem", class: "sheet-item", disabled: item.disabled,
        "data-action": item.action ?? null, "data-destructive": item.destructive ? "" : null,
        "aria-haspopup": item.items ? "menu" : null,
        onclick: () => {
          if (item.items) { trail.push({ title: item.label, items: item.items }); render(); return; }
          haptic(6);
          sheet.close();
          item.run?.();
        },
      },
      item.icon ? h("span", { class: "sheet-ico" }, icon(item.icon, 20)) : h("span", { class: "sheet-gap" }),
      h("span", { class: "sheet-label", text: item.label }),
      item.items ? h("span", { class: "chev" }, icon("chevronRight", 14)) : null);
    });
    const head = index > 0
      ? h("button", { type: "button", class: "sheet-back", onclick: () => { trail.pop(); render(); } }, icon("back", 18), h("span", { text: entry.title }))
      : (entry.title ? h("div", { class: "sheet-title", text: entry.title }) : null);
    return [head, h("div", { class: "sheet-list", role: "menu" }, list)].filter(Boolean);
  }

  function render() {
    sheet.body.replaceChildren(...pageOf(trail[trail.length - 1], trail.length - 1));
    sheet.body.scrollTop = 0;
  }

  sheet = openSheet({ label, className: "action-sheet", onClose });
  const baseClose = sheet.close;
  render();
  // Back steps out of a drilled page before it closes the sheet.
  sheet.close = baseClose;
  sheet.back = () => {
    if (trail.length > 1) { trail.pop(); render(); return true; }
    return false;
  };
  return sheet;
}

onBack(4, () => {
  const top = open[open.length - 1];
  return !!top?.back?.();
});
