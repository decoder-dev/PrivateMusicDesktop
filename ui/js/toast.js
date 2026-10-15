import { h } from "./dom.js";

let current = null;
let timer = null;

/** A short message at the bottom. No animation: it appears and disappears. */
export function toast(message, { error = false } = {}) {
  current?.remove();
  clearTimeout(timer);
  current = h("div", { class: `toast${error ? " error" : ""}`, role: error ? "alert" : "status", text: message });
  document.body.append(current);
  timer = setTimeout(() => current?.remove(), error ? 6000 : 3000);
}
