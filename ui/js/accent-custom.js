// A custom accent colour: any colour the person picks becomes a light-theme and a dark-theme accent that keep text
// readable. The light variant is darkened until white text on it, and it as text on the light ground, reach 4.5:1;
// the dark variant is lightened until it reaches 4.5:1 on the dark ground (black text sits on it, as the presets do).

export const DEFAULT_CUSTOM_ACCENT = "#1c7a45";

export function normalizeHex(value) {
  const text = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (/^#[0-9a-f]{6}$/.test(text)) return text;
  if (/^#[0-9a-f]{3}$/.test(text)) return `#${[...text.slice(1)].map((c) => c + c).join("")}`;
  return null;
}

const toRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const toHex = (rgb) => `#${rgb.map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0")).join("")}`;
const channel = (v) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
const luminance = (rgb) => 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
export function contrast(a, b) {
  const [x, y] = [luminance(toRgb(a)), luminance(toRgb(b))].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
}
const mix = (rgb, target, amount) => rgb.map((v, i) => v + (target[i] - v) * amount);

/** Moves the colour towards `target` in small steps until `ok` holds (at most to the target itself). */
function adjust(hex, target, ok) {
  const rgb = toRgb(hex);
  for (let step = 0; step <= 40; step++) {
    const candidate = toHex(mix(rgb, target, step / 40));
    if (ok(candidate)) return candidate;
  }
  return toHex(target);
}

/** {light, dark} accent pair for a picked colour; null for an invalid colour. */
export function customAccentPair(value) {
  const hex = normalizeHex(value);
  if (!hex) return null;
  const light = adjust(hex, [0, 0, 0], (c) => contrast(c, "#ffffff") >= 4.5 && contrast(c, "#f1f3f0") >= 4.5);
  const dark = adjust(hex, [255, 255, 255], (c) => contrast(c, "#121212") >= 4.5 && contrast(c, "#000000") >= 7);
  return { light, dark };
}

/** Applies (or clears) the custom accent on <html>: the CSS reads --a-l / --a-d like a preset. */
export function applyCustomAccent(root, accent, value) {
  const pair = accent === "custom" ? customAccentPair(value) ?? customAccentPair(DEFAULT_CUSTOM_ACCENT) : null;
  if (pair) {
    root.style.setProperty("--a-l", pair.light);
    root.style.setProperty("--a-d", pair.dark);
  } else {
    root.style.removeProperty("--a-l");
    root.style.removeProperty("--a-d");
  }
  return pair;
}
