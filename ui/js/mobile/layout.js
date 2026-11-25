// Which interface the window shows, and the hooks the phone shell needs from the
// system. Everything is derived from three things: the platform (the Android
// shell announces itself in the user agent; `?platform=android` does the same
// for tests and for trying the phone UI in a desktop browser), the width of the
// window and whether the pointer is a finger. The result lives on <html> so CSS
// can follow it without a line of script:
//
//   data-platform = "android" | "desktop"
//   data-layout   = "phone" | "wide"
//   data-orientation = "portrait" | "landscape"   (phone layout only matters)
//   data-keyboard = "open"                          (a text field has the focus)
//
// The wide layout is the desktop window with its sidebar and is never touched by
// the phone code: a narrow desktop window with a mouse keeps it.

const PHONE_MAX_WIDTH = 700;
const params = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
const root = document.documentElement;
const listeners = new Set();

function detectPlatform() {
  const forced = params.get("platform");
  if (forced === "android" || forced === "desktop") return forced;
  return /Android/i.test(navigator.userAgent ?? "") ? "android" : "desktop";
}

let platform = detectPlatform();
export const currentPlatform = () => platform;
export const isAndroid = () => platform === "android";

/** The backend knows best what it runs on (`app_info.platform`); the page follows it. */
export function setPlatform(value) {
  // An explicit ?platform= (tests, trying the phone UI in a desktop browser) wins.
  if ((value !== "android" && value !== "desktop") || value === platform || params.get("platform")) return;
  platform = value;
  apply();
}

function detectLayout() {
  const forced = params.get("layout");
  if (forced === "phone" || forced === "wide") return forced;
  if (platform === "android") return "phone";
  // A window that was merely made narrow with a mouse keeps the desktop layout.
  return window.innerWidth <= PHONE_MAX_WIDTH && matchMedia("(pointer: coarse)").matches ? "phone" : "wide";
}

let layout = "wide";
export const currentLayout = () => layout;
export const isPhone = () => layout === "phone";

/** Calls `fn(layout)` whenever the layout changes (rotation, resize). Returns an unsubscribe. */
export function onLayoutChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function apply() {
  const next = detectLayout();
  root.dataset.platform = platform;
  root.dataset.orientation = window.innerWidth > window.innerHeight ? "landscape" : "portrait";
  if (next === layout && root.dataset.layout) return;
  layout = next;
  root.dataset.layout = next;
  for (const fn of [...listeners]) fn(next);
}

// ------------------------------------------------------------ the keyboard

const TEXT_INPUT = "input:not([type=range]):not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]), textarea, [contenteditable=true]";

function textFieldFocused() {
  const el = document.activeElement;
  return !!el && el !== document.body && el.matches?.(TEXT_INPUT);
}

function syncKeyboard() {
  const open = isPhone() && textFieldFocused();
  if (open) root.dataset.keyboard = "open";
  else delete root.dataset.keyboard;
  // How much of the window the on-screen keyboard covers, for sheets that must stay above it.
  const vv = window.visualViewport;
  const covered = vv ? Math.max(0, Math.round(window.innerHeight - vv.height - vv.offsetTop)) : 0;
  root.style.setProperty("--kb", `${open ? covered : 0}px`);
}

// ------------------------------------------------- status bar / theme-color

let themeMeta = null;
function hexColor(value) {
  const text = String(value).trim();
  if (/^#[0-9a-f]{6}$/i.test(text)) return text.toLowerCase();
  const match = text.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
  if (!match) return null;
  return `#${match.slice(1, 4).map((part) => Number(part).toString(16).padStart(2, "0")).join("")}`;
}

/** Keeps <meta name="theme-color"> equal to the page background so the Android status bar blends in. */
export function syncThemeColor() {
  themeMeta ??= document.querySelector('meta[name="theme-color"]:not([media])') ?? (() => {
    const meta = document.createElement("meta");
    meta.name = "theme-color";
    document.head.append(meta);
    return meta;
  })();
  // Quiet stage (and every other style) paints the page itself. The status bar follows that colour.
  const variable = getComputedStyle(document.documentElement).getPropertyValue("--page-top").trim();
  const color = hexColor(variable) ?? hexColor(getComputedStyle(document.body).backgroundColor);
  if (color && themeMeta.content !== color) themeMeta.content = color;
}

export function initLayout() {
  apply();
  syncKeyboard();
  syncThemeColor();
  // The first call can run before the style attribute's colours are computed.
  requestAnimationFrame(syncThemeColor);
  window.addEventListener("load", syncThemeColor);
  window.addEventListener("resize", () => { apply(); syncKeyboard(); });
  window.addEventListener("orientationchange", apply);
  matchMedia("(pointer: coarse)").addEventListener?.("change", apply);
  matchMedia("(prefers-color-scheme: dark)").addEventListener?.("change", syncThemeColor);
  document.addEventListener("focusin", syncKeyboard);
  document.addEventListener("focusout", () => setTimeout(syncKeyboard, 0));
  window.visualViewport?.addEventListener("resize", syncKeyboard);
  new MutationObserver(() => requestAnimationFrame(syncThemeColor)).observe(root, { attributes: true, attributeFilter: ["data-theme", "data-style", "data-palette"] });
}

/** A short tick for the moments the iOS app gives haptics (like, long press). Silent where unsupported. */
export function haptic(ms = 10) {
  try {
    if (root.dataset.haptics!=="off" && isPhone() && typeof navigator.vibrate === "function") navigator.vibrate(ms);
  } catch {
    // Vibration needs a user gesture and may be blocked: never a problem.
  }
}
