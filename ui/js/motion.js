// Motion is read live: the person can switch "reduce motion" on while the app
// runs, so nothing here is captured once at load.

const query = matchMedia("(prefers-reduced-motion: no-preference)");

/** True when the system allows animation right now. */
export const motionOk = () => query.matches;

/** Calls `fn(motionOk)` whenever the system setting changes. */
export function onMotionChange(fn) {
  const handler = () => fn(query.matches);
  query.addEventListener("change", handler);
  return () => query.removeEventListener("change", handler);
}

/**
 * Runs `done` once when the animation that was just started on `element` ends.
 * With motion off, or when no animation is running, it runs at once, so
 * closing logic never waits for something that will not happen.
 */
export function afterAnimation(element, done, fallback = 450) {
  if (!motionOk() || !element.getAnimations?.().length) {
    done();
    return;
  }
  let finished = false;
  const finish = () => {
    if (finished) return;
    finished = true;
    element.removeEventListener("animationend", onEnd);
    clearTimeout(timer);
    done();
  };
  const onEnd = (event) => { if (event.target === element) finish(); };
  element.addEventListener("animationend", onEnd);
  const timer = setTimeout(finish, fallback);
}

/** Plays a one-shot enter animation class on an element that was just shown. */
export function enter(element) {
  if (!motionOk()) return;
  element.classList.remove("enter");
  void element.offsetWidth;
  element.classList.add("enter");
  element.addEventListener("animationend", () => element.classList.remove("enter"), { once: true });
}

/** A one-shot glyph swap (play/pause, heart): the new glyph pops in. Only when the glyph actually changed. */
export function morph(element) {
  if (!motionOk() || !element) return;
  element.classList.add("morph");
  element.addEventListener("animationend", () => element.classList.remove("morph"), { once: true });
}
