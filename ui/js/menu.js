import { h } from "./dom.js";
import { icon } from "./icons.js";

let current = null;

export function closeMenu() {
  current?.close();
}

/**
 * A small pop-up menu at a point. items: { label, action, icon?, items? (a
 * sub-menu), disabled? } or "-" for a separator. Arrow keys, Enter and
 * Escape work; a click outside closes it.
 */
export function openMenu({ x, y }, items, { label } = {}) {
  closeMenu();
  const stack = [];

  function build(list, at) {
    const buttons = [];
    const element = h("div", { class: "menu", role: "menu", "aria-label": label ?? "" },
      list.map((item) => {
        if (item === "-") return h("div", { class: "menu-sep", role: "separator" });
        const button = h("button", {
          type: "button", role: "menuitem", class: "menu-item", disabled: item.disabled,
          "data-action": item.action ?? null, "aria-haspopup": item.items ? "menu" : null,
          onclick: (event) => {
            event.stopPropagation();
            if (item.items) return openSub(item, button);
            close();
            item.run?.();
          },
        },
        item.icon ? icon(item.icon, 16) : h("span", { class: "menu-gap" }),
        h("span", { class: "menu-label", text: item.label }),
        item.items ? icon("chevronRight", 14) : null);
        buttons.push(button);
        return button;
      }));
    document.body.append(element);
    // Keep the menu on screen.
    const box = element.getBoundingClientRect();
    const left = Math.max(8, Math.min(at.x, window.innerWidth - box.width - 8));
    const top = Math.max(8, Math.min(at.y, window.innerHeight - box.height - 8));
    element.style.left = `${left}px`;
    element.style.top = `${top}px`;
    const level = { element, buttons };
    stack.push(level);
    buttons.find((b) => !b.disabled)?.focus({ preventScroll: true });
    return level;
  }

  function openSub(item, button) {
    while (stack.length > 1 && stack[stack.length - 1].element !== button.closest(".menu")) stack.pop().element.remove();
    const box = button.getBoundingClientRect();
    build(item.items, { x: box.right + 2, y: box.top - 4 });
  }

  function close() {
    for (const level of stack.splice(0)) level.element.remove();
    document.removeEventListener("pointerdown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
    window.removeEventListener("blur", close);
    window.removeEventListener("resize", close);
    if (current?.close === close) current = null;
  }

  function onOutside(event) {
    if (!stack.some((level) => level.element.contains(event.target))) close();
  }

  function onKey(event) {
    const level = stack[stack.length - 1];
    if (!level) return;
    const enabled = level.buttons.filter((b) => !b.disabled);
    const index = enabled.indexOf(document.activeElement);
    if (event.key === "Escape" || (event.key === "ArrowLeft" && stack.length > 1)) {
      event.preventDefault();
      event.stopPropagation();
      if (stack.length > 1) stack.pop().element.remove();
      else close();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      const step = event.key === "ArrowDown" ? 1 : -1;
      enabled[(index + step + enabled.length) % enabled.length]?.focus();
    } else if (event.key === "ArrowRight" && document.activeElement?.getAttribute("aria-haspopup")) {
      event.preventDefault();
      event.stopPropagation();
      document.activeElement.click();
    } else if (event.key === "Tab") {
      close();
    }
  }

  build(items, { x, y });
  document.addEventListener("pointerdown", onOutside, true);
  document.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", close);
  window.addEventListener("resize", close);
  current = { close };
  return current;
}
