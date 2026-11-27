// Swipes on the mini player, after MiniPlayerGesturePolicy in the iOS app:
// a short drag left is "next", right is "previous", up opens the player. The
// decision uses the distance travelled plus where the finger was heading
// (a flick that lifts early still counts), axis-dominance 1.2, and the card
// follows the finger a little (parallax) unless motion is reduced.

import { motionOk } from "../motion.js";
import { haptic, isPhone } from "./layout.js";
import { gestureAction, isMiniGestureTarget } from "./mini-gesture-policy.js";

export { gestureAction } from "./mini-gesture-policy.js";

const PARALLAX_X = 0.12;
const PARALLAX_Y = 0.08;
const PREDICT_MS = 160;

export function attachMiniGestures(element, { onNext, onPrevious, onOpen }) {
  let drag = null;
  let suppressClick = false;

  element.addEventListener("pointerdown", (event) => {
    if (!isPhone() || (event.pointerType === "mouse" && event.button !== 0)) return;
    suppressClick = false;
    // Transport buttons keep their taps. The cover also accepts swipes: it is
    // the largest natural place to start the upward gesture on a phone.
    if (!isMiniGestureTarget(event.target)) return;
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, samples: [{ x: event.clientX, y: event.clientY, t: performance.now() }], moved: false };
  });
  element.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) > 6) {
      drag.moved = true;
      try { element.setPointerCapture(event.pointerId); } catch { /* gone */ }
    }
    drag.samples.push({ x: event.clientX, y: event.clientY, t: performance.now() });
    if (drag.samples.length > 6) drag.samples.shift();
    if (motionOk() && drag.moved) {
      element.style.setProperty("--mx", `${dx * PARALLAX_X}px`);
      element.style.setProperty("--my", `${Math.min(dy * PARALLAX_Y, 0)}px`);
    }
  });

  function finish(event, cancelled) {
    if (!drag || event.pointerId !== drag.id) return;
    const { x, y, samples, moved } = drag;
    drag = null;
    element.style.removeProperty("--mx");
    element.style.removeProperty("--my");
    if (!moved || cancelled) return;
    const translation = { x: event.clientX - x, y: event.clientY - y };
    // Where the finger was heading: its recent speed carried on for a moment.
    const first = samples[0];
    const last = samples[samples.length - 1];
    const span = Math.max(1, last.t - first.t);
    const predicted = {
      x: translation.x + ((last.x - first.x) / span) * PREDICT_MS,
      y: translation.y + ((last.y - first.y) / span) * PREDICT_MS,
    };
    const action = gestureAction(translation, predicted);
    // A small unrecognised movement must not eat the next genuine tap.
    if (!action) return;
    suppressClick = true;
    haptic(8);
    if (action === "next") onNext();
    else if (action === "previous") onPrevious();
    else onOpen();
  }
  element.addEventListener("pointerup", (event) => finish(event, false));
  element.addEventListener("pointercancel", (event) => finish(event, true));
  // A recognised swipe is not a tap.
  element.addEventListener("click", (event) => {
    if (suppressClick && event.detail > 0) {
      suppressClick = false;
      event.stopImmediatePropagation(); event.preventDefault();
    }
  }, true);
}
