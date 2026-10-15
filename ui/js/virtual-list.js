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

  function render() {
    scheduled = false;
    // A list that is not on screen (display: none) draws nothing; the
    // ResizeObserver below redraws it the moment it becomes visible.
    if (!root.offsetParent) return;
    const viewport = root.clientHeight || 600;
    const first = Math.max(0, Math.floor(root.scrollTop / rowHeight) - 4);
    const last = Math.min(items.length, Math.ceil((root.scrollTop + viewport) / rowHeight) + 4);
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
  new ResizeObserver(schedule).observe(root);

  for (const type of ["click", "dblclick", "keydown"]) {
    root.addEventListener(type, (event) => {
      const row = event.target.closest?.(".row");
      if (!row || !windowEl.contains(row)) return;
      const index = Number(row.dataset.index);
      if (index >= 0 && index < items.length) onRowEvent?.(type, event, items[index], index);
    });
  }

  return {
    element: root,
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
