// Вид телефонной версии: «ios» (по умолчанию, как в приложении для iPhone) или «material» (Material 3).
// Выбор хранится на устройстве и ставится на <html data-style>; CSS (css/material.css) держится на нем.
const KEY = "pm-style";
export const STYLES = ["ios", "material"];

export function currentStyle() {
  try { const value = localStorage.getItem(KEY); return STYLES.includes(value) ? value : "ios"; } catch { return "ios"; }
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
