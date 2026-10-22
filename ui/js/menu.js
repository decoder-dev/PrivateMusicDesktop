import { h } from "./dom.js";
import { icon } from "./icons.js";
import { isPhone } from "./mobile/layout.js";
import { openActionSheet } from "./mobile/sheet.js";

let current = null;

export function closeMenu() {
  current?.close();
}

/**
 * Небольшое всплывающее меню в точке. элементы: { метка, действие, значок?, элементы? (подменю),
 * отключено?, разрушительно? } или "-" для разделителя. Клавиши со стрелками, Enter и Escape работают;
 * щелчок снаружи закрывает его. Подменю открывается рядом со своим родительским элементом слева, когда
 * правая часть заполнена, и никогда не закрывает его.
 */
export function openMenu({ x, y }, items, { label } = {}) {
  closeMenu();
  // A phone has no pointer to anchor a pop-up to: the same items rise as a bottom sheet.
  if (isPhone()) {
    const sheet = openActionSheet(items, { label, onClose: () => { if (current === sheet) current = null; } });
    current = sheet;
    return sheet;
  }
  const stack = [];
  document.body.classList.add("overlay-open");

  function build(list, at, { parent = null } = {}) {
    const buttons = [];
    const element = h("div", { class: `menu glass strong${parent ? " sub" : ""}`, role: "menu", "aria-label": label ?? "" },
      list.map((item) => {
        if (item === "-") return h("div", { class: "menu-sep", role: "separator" });
        const button = h("button", {
          type: "button", role: "menuitem", class: "menu-item", disabled: item.disabled,
          "data-action": item.action ?? null, "aria-haspopup": item.items ? "menu" : null,
          "data-destructive": item.destructive ? "" : null,
          onclick: (event) => {
            event.stopPropagation();
            if (item.items) return openSub(item, button);
            close();
            item.run?.();
          },
        },
        item.icon ? icon(item.icon, 16) : h("span", { class: "menu-gap" }),
        h("span", { class: "menu-label", text: item.label }),
        item.items ? h("span", { class: "chev" }, icon("chevronRight", 12)) : null);
        buttons.push(button);
        return button;
      }));
    document.body.append(element);
    // Держите меню на экране; подменю переворачивается на левую сторону родительского элемента. Размер
    // макета, а не нарисованного прямоугольника: анимация всплывающего окна масштабирует меню во время
    // его измерения.
    const box = { width: element.offsetWidth, height: element.offsetHeight };
    const margin = 8;
    let left = at.x;
    if (parent && left + box.width > window.innerWidth - margin) left = parent.left - box.width - 2;
    left = Math.max(margin, Math.min(left, window.innerWidth - box.width - margin));
    const top = Math.max(margin, Math.min(at.y, window.innerHeight - box.height - margin));
    element.style.left = `${left}px`;
    element.style.top = `${top}px`;
    // Всплывающее окно растет с того места, где оно было открыто.
    element.style.setProperty("--ox", `${Math.max(0, Math.min(box.width, at.x - left))}px`);
    element.style.setProperty("--oy", `${Math.max(0, Math.min(box.height, at.y - top))}px`);
    const level = { element, buttons };
    stack.push(level);
    buttons.find((b) => !b.disabled)?.focus({ preventScroll: true });
    return level;
  }

  function openSub(item, button) {
    while (stack.length > 1 && stack[stack.length - 1].element !== button.closest(".menu")) stack.pop().element.remove();
    const parentMenu = button.closest(".menu");
    const parentBox = { left: parentMenu.offsetLeft, right: parentMenu.offsetLeft + parentMenu.offsetWidth };
    const box = button.getBoundingClientRect();
    build(item.items, { x: parentBox.right + 2, y: box.top - 6 }, { parent: parentBox });
  }

  function close() {
    for (const level of stack.splice(0)) level.element.remove();
    document.body.classList.remove("overlay-open");
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
      if (stack.length > 1) {
        stack.pop().element.remove();
        stack[stack.length - 1].buttons.find((b) => b.getAttribute("aria-haspopup"))?.focus();
      } else close();
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
