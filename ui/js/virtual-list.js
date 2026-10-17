import { own } from "./store.js";
import { h } from "./dom.js";

/**
 * A list that only keeps the rows on screen in the DOM, so a library of tens
 * of thousands of tracks scrolls as smoothly as ten.
 *
 * `renderRow(item, index)` returns the row element. Clicks are handled by one
 * delegated listener; rows carry `data-index`.
 */
export function createVirtualList({ rowHeight, renderRow, onRowEvent }) {
  const spacer = h("div", { class: "spacer" });
  const windowEl = h("div", { class: "window" });
  spacer.append(windowEl);
  const root = h("div", { class: "list", role: "listbox", tabindex: "-1" }, spacer);
  let items = [];
  let scheduled = false;
  let header = null;

  function render() {
    scheduled = false;
    // A list that is not on screen (display: none) draws nothing; the
    // ResizeObserver below redraws it the moment it becomes visible.
    if (!root.offsetParent) return;
    const viewport = root.clientHeight || 600;
    const offset = header?.offsetHeight ?? 0;
    const top = Math.max(0, root.scrollTop - offset);
    const first = Math.max(0, Math.floor(top / rowHeight) - 4);
    const last = Math.min(items.length, Math.max(4, Math.ceil((root.scrollTop + viewport - offset) / rowHeight) + 4));
    spacer.style.height = `${items.length * rowHeight}px`;
    windowEl.style.top = `${first * rowHeight}px`;
    const rows = [];
    for (let index = first; index < last; index++) {
      const row = renderRow(items[index], index);
      row.dataset.index = String(index);
      rows.push(row);
    }
    windowEl.replaceChildren(...rows);
  }

  function schedule() {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(render);
  }

  root.addEventListener("scroll", schedule, { passive: true });
  const observer = new ResizeObserver(schedule);
  observer.observe(root);
  own(() => observer.disconnect());

  for (const type of ["click", "dblclick", "keydown", "contextmenu"]) {
    root.addEventListener(type, (event) => {
      const row = event.target.closest?.(".row");
      if (!row || !windowEl.contains(row)) return;
      const index = Number(row.dataset.index);
      if (index >= 0 && index < items.length) onRowEvent?.(type, event, items[index], index);
    });
  }

  return {
    element: root,
    setHeader(node) {
      if (header) { observer.unobserve(header); header.remove(); }
      header = node;
      if (header) { root.prepend(header); observer.observe(header); }
      root.setAttribute("role", header ? "region" : "listbox");
      spacer.setAttribute("role", header ? "listbox" : "presentation");
      schedule();
    },
    scrollToRows() { root.scrollTop = header?.offsetHeight ?? 0; },
    setItems(next) {
      items = next;
      render();
    },
    /** Re-renders visible rows (selection or data changed). */
    refresh: render,
    scrollToTop() {
      root.scrollTop = 0;
    },
    get length() {
      return items.length;
    },
  };
}
