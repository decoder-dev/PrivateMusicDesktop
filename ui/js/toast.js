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
 * Короткое сообщение в стеклянной капсуле над панелью игрока. `action` ({ label, run }) делает баннер
 * отмены действия более продолжительным.
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
