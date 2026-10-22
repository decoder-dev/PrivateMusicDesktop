// The full-screen player is a sheet on the phone: it rises from the bottom, and a
// drag down from anywhere that is not a control or a scrolling list makes it
// follow the finger; far enough (or fast enough) and it closes, otherwise it
// springs back. Taps and the lists inside are left alone.

import { isPhone, haptic } from "./layout.js";

const DISMISS = 120;
const VELOCITY = 0.6;
const IGNORE = "button, input, textarea, a, .ly-body, .nq-list, .np-seek, [role=slider]";

export function attachSheetDrag(sheet, close) {
  let drag = null;

  sheet.addEventListener("pointerdown", (event) => {
    if (!isPhone() || (event.pointerType === "mouse" && event.button !== 0)) return;
    if (event.target.closest?.(IGNORE)) return;
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, last: event.clientY, t: performance.now(), v: 0, on: false };
  });

  sheet.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    const dy = event.clientY - drag.y;
    const dx = event.clientX - drag.x;
    if (!drag.on) {
      if (Math.abs(dy) < 8 && Math.abs(dx) < 8) return;
      // Only a mostly vertical, downward drag starts the sheet moving.
      if (dy <= 0 || Math.abs(dx) > Math.abs(dy)) { drag = null; return; }
      drag.on = true;
      sheet.classList.add("dragging");
      try { sheet.setPointerCapture(event.pointerId); } catch { /* gone */ }
    }
    const now = performance.now();
    drag.v = (event.clientY - drag.last) / Math.max(1, now - drag.t);
    drag.last = event.clientY;
    drag.t = now;
    sheet.style.setProperty("--dy", `${Math.max(0, dy)}px`);
  });

  function release(event, cancelled) {
    if (!drag || event.pointerId !== drag.id) return;
    const { on, v, y } = drag;
    drag = null;
    if (!on) return;
    sheet.classList.remove("dragging");
    const dy = Math.max(0, event.clientY - y);
    if (!cancelled && (dy > DISMISS || (v > VELOCITY && dy > 24))) {
      haptic(8);
      close();
    } else {
      sheet.style.setProperty("--dy", "0px");
    }
  }
  sheet.addEventListener("pointerup", (event) => release(event, false));
  sheet.addEventListener("pointercancel", (event) => release(event, true));
  // The drag must not turn into a tap on whatever was under the finger.
  sheet.addEventListener("click", (event) => {
    if (sheet.classList.contains("dragging")) { event.stopPropagation(); event.preventDefault(); }
  }, true);
}
