// App style on every platform. "stage" is the default (docs/design/system.md, css/stage.css).
// "premium" is the previous default, "ios" the classic look, "material" Material 3.
// Выбор хранится на устройстве и ставится на <html data-style>.
import { syncThemeColor } from "./layout.js";

const KEY = "pm-style";
export const STYLES = ["stage", "premium", "ios", "material"];
export const DEFAULT_STYLE = "stage";

export function currentStyle() {
  try { const value = localStorage.getItem(KEY); return STYLES.includes(value) ? value : DEFAULT_STYLE; } catch { return DEFAULT_STYLE; }
}
export function applyStyle(value = currentStyle()) {
  document.documentElement.dataset.style = value;
  syncThemeColor();
}
export function setStyle(value) {
  if (!STYLES.includes(value)) return;
  try { localStorage.setItem(KEY, value); } catch { /* память недоступна: действует до перезапуска */ }
  applyStyle(value);
}
applyStyle();
