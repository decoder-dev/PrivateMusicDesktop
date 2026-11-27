const MIN_DISTANCE = 18;
const HORIZONTAL = 58;
const VERTICAL = 42;
const DOMINANCE = 1.2;

/** The iOS policy: "next" | "previous" | "open" | null for a drag. */
export function gestureAction(translation, predicted) {
  const settled = Math.hypot(translation.x, translation.y);
  const heading = Math.hypot(predicted.x, predicted.y);
  const t = heading > settled ? predicted : translation;
  const horizontal = Math.abs(t.x);
  const vertical = Math.abs(t.y);
  if (Math.max(horizontal, vertical) < MIN_DISTANCE) return null;
  if (vertical >= horizontal * DOMINANCE) return t.y <= -VERTICAL ? "open" : null;
  if (horizontal >= vertical * DOMINANCE) {
    if (t.x <= -HORIZONTAL) return "next";
    if (t.x >= HORIZONTAL) return "previous";
  }
  return null;
}

/** Cover swipes are allowed; transport and other native controls keep their taps. */
export function isMiniGestureTarget(target) {
  const control = target?.closest?.("button, input, a, [role=button]");
  return !control || control.classList.contains("cover-button");
}
