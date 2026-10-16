// Tooltips: one delegated listener turns the native `title` of a control into a
// small glass label with an optional shortcut hint (`data-kbd`). It shows after
// a short rest of the mouse, never for touch, and puts the title back when it
// hides, so screen readers and the keyboard keep working as before.

const DELAY = 450;
let timer = 0;
let active = null; // { target, text, bubble }

const mac = /Mac|iPhone|iPad/.test(navigator.platform ?? "");
/** "Ctrl+Right" written for this platform: Cmd on a Mac. */
export function shortcut(keys) {
  return keys.replace("Ctrl", mac ? "⌘" : "Ctrl").replace("Right", "→").replace("Left", "←").replace("Up", "↑").replace("Down", "↓");
}

function hide() {
  clearTimeout(timer);
  timer = 0;
  if (!active) return;
  const { target, text, bubble } = active;
  active = null;
  bubble.remove();
  if (target.isConnected && !target.hasAttribute("title") && target.dataset.tip === text) target.setAttribute("title", text);
}

function show(target) {
  const text = target.getAttribute("title") || target.dataset.tip;
  if (!text) return;
  target.dataset.tip = text;
  target.removeAttribute("title"); // no second, native tooltip
  const bubble = document.createElement("div");
  bubble.className = "tooltip glass strong";
  bubble.setAttribute("role", "tooltip");
  const label = document.createElement("span");
  label.textContent = text;
  bubble.append(label);
  if (target.dataset.kbd) {
    for (const part of shortcut(target.dataset.kbd).split("+")) {
      const key = document.createElement("kbd");
      key.textContent = part;
      bubble.append(key);
    }
  }
  document.body.append(bubble);
  const box = target.getBoundingClientRect();
  const size = bubble.getBoundingClientRect();
  const margin = 8;
  const above = box.top - size.height - 8 >= margin;
  const left = Math.max(margin, Math.min(box.left + box.width / 2 - size.width / 2, window.innerWidth - size.width - margin));
  bubble.style.left = `${Math.round(left)}px`; // CSSOM: allowed under the CSP
  bubble.style.top = `${Math.round(above ? box.top - size.height - 8 : box.bottom + 8)}px`;
  bubble.classList.toggle("below", !above);
  active = { target, text, bubble };
}

export function installTooltips() {
  document.addEventListener("pointerover", (event) => {
    if (event.pointerType !== "mouse") return;
    const target = event.target instanceof Element ? event.target.closest("[title], [data-tip]") : null;
    if (active && target === active.target) return;
    hide();
    if (!target || target.closest(".menu, .toast")) return;
    timer = setTimeout(() => show(target), DELAY);
  });
  document.addEventListener("pointerout", (event) => {
    if (active && !active.target.contains(event.relatedTarget)) hide();
    else if (!active) { clearTimeout(timer); timer = 0; }
  });
  for (const type of ["pointerdown", "keydown", "wheel", "blur"]) document.addEventListener(type, hide, { capture: true, passive: true });
  window.addEventListener("blur", hide);
}
