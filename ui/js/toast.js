import { h } from "./dom.js";
import { icon } from "./icons.js";
import { afterAnimation } from "./motion.js";

let current = null;
let timer = null;

function dismiss(element = current, { immediate = false } = {}) {
  if (!element) return;
  if (element === current) { current = null; clearTimeout(timer); }
  if (immediate) { element.remove(); return; }
  element.classList.add("closing");
  afterAnimation(element, () => element.remove(), 300);
}

/**
 * A short message in a glass capsule above the player bar. An `action`
 * ({ label, run }) makes it an undo banner that stays longer.
 */
export function toast(message, { error = false, action = null } = {}) {
  dismiss(current, { immediate: true });
  const element = h("div", {
    class: `toast glass strong${error ? " error" : ""}${action ? " undo" : ""}`, role: error ? "alert" : "status",
  },
  error ? icon("alert", 16) : null,
  h("span", { class: "toast-text", text: message }),
  action ? h("button", { class: "toast-action", type: "button", text: action.label, onclick: () => { dismiss(element); action.run(); } }) : null);
  current = element;
  document.body.append(element);
  timer = setTimeout(() => dismiss(element), action ? 8000 : error ? 6000 : 3000);
}
