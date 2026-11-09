// App style on every platform: "premium" (the default design, css/premium.css), "ios" (the classic look shared with
// the iPhone app) or "material" (Material 3). Выбор хранится на устройстве и ставится на <html data-style>.
const KEY = "pm-style";
export const STYLES = ["premium", "ios", "material"];
export const DEFAULT_STYLE = "premium";

export function currentStyle() {
  try { const value = localStorage.getItem(KEY); return STYLES.includes(value) ? value : DEFAULT_STYLE; } catch { return DEFAULT_STYLE; }
}
export function applyStyle(value = currentStyle()) {
  document.documentElement.dataset.style = value;
}
export function setStyle(value) {
  if (!STYLES.includes(value)) return;
  try { localStorage.setItem(KEY, value); } catch { /* память недоступна: действует до перезапуска */ }
  applyStyle(value);
}
applyStyle();
