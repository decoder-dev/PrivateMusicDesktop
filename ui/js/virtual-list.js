import { own } from "./store.js";
import { h } from "./dom.js";
import { isPhone } from "./mobile/layout.js";
import { attachRowGestures } from "./mobile/row-gestures.js";

/**
 * Список, который в DOM сохраняет на экране только строки, поэтому библиотека из десятков тысяч треков
 * прокручивается так же плавно, как десять.
 *
 * `renderRow(item, index)` возвращает элемент строки. Клики обрабатываются одним делегированным
 * прослушивателем; строки содержат `data-index`.
 *
 * На широком макете список сам себе прокрутка. На телефоне прокручивается весь экран (крупный
 * заголовок, кнопки и строки двигаются вместе, как SwiftUI List), поэтому список измеряет себя
 * относительно экрана, в котором стоит: то же окно строк, другой скроллер.
 */
export function createVirtualList({ rowHeight, renderRow, onRowEvent }) {
  const spacer = h("div", { class: "spacer" });
  const windowEl = h("div", { class: "window" });
  spacer.append(windowEl);
  const root = h("div", { class: "list", role: "listbox", tabindex: "-1" }, spacer);
  let items = [];
  let scheduled = false;
  let header = null;
  let pageScroller = null; // экран, который прокручивается на телефоне
  const nearEnd = [];

  /** Элемент, по прокрутке которого решается, какие строки рисовать. */
  const scroller = () => (isPhone() ? root.closest(".main > .view") ?? root : root);

  function watch() {
    const next = scroller();
    const outer = next === root ? null : next;
    if (outer === pageScroller) return;
    pageScroller?.removeEventListener("scroll", schedule);
    pageScroller = outer;
    if (outer) {
      outer.addEventListener("scroll", schedule, { passive: true });
      observer.observe(outer);
    }
  }

  /** Где начинаются строки внутри прокручиваемого содержимого экрана (после шапки списка). */
  function rowsOffset(scrolling) {
    return spacer.getBoundingClientRect().top - scrolling.getBoundingClientRect().top + scrolling.scrollTop;
  }

  function render() {
    scheduled = false;
    // Высота списка известна, даже когда он скрыт: при показе размер меняется и ResizeObserver перерисовывает.
    spacer.style.height = `${items.length * rowHeight}px`;
    // Список, которого нет на экране (отображение: нет), ничего не рисует; ResizeObserver ниже
    // перерисовывает его в тот момент, когда он становится видимым.
    if (!root.offsetParent) return;
    watch();
    const scrolling = scroller();
    const own = scrolling === root;
    let top;
    let viewport;
    if (own) {
      viewport = root.clientHeight || 600;
      top = Math.max(0, root.scrollTop - (header?.offsetHeight ?? 0));
    } else {
      viewport = scrolling.clientHeight || 600;
      top = Math.max(0, scrolling.scrollTop - rowsOffset(scrolling));
      // Конец окна считается от верха экрана, а не от верха списка.
    }
    const first = Math.max(0, Math.floor(top / rowHeight) - 4);
    const last = Math.max(first, Math.min(items.length, Math.max(4, Math.ceil((top + viewport) / rowHeight) + 4)));
    spacer.style.height = `${items.length * rowHeight}px`;
    windowEl.style.top = `${first * rowHeight}px`;
    const rows = [];
    for (let index = first; index < last; index++) {
      const row = renderRow(items[index], index);
      row.dataset.index = String(index);
      rows.push(row);
    }
    windowEl.replaceChildren(...rows);
    // Широкий макет слушает собственную прокрутку списка (см. экраны); помощь нужна только режиму прокрутки страницы.
    if (!own) {
      for (const { margin, fn } of nearEnd) {
        if (top + viewport > items.length * rowHeight - margin) fn();
      }
    }
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

  for (const type of ["click", "dblclick", "keydown", "contextmenu", "pm-row-longpress", "pm-row-swipe"]) {
    root.addEventListener(type, (event) => {
      const row = event.target.closest?.(".row");
      if (!row || !windowEl.contains(row)) return;
      const index = Number(row.dataset.index);
      if (index >= 0 && index < items.length) onRowEvent?.(type, event, items[index], index);
    });
  }
  attachRowGestures(root);

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
    scrollToRows() {
      const scrolling = scroller();
      if (scrolling === root) root.scrollTop = header?.offsetHeight ?? 0;
      else scrolling.scrollTop = rowsOffset(scrolling);
    },
    setItems(next) {
      items = next;
      render();
      // Панели пустого списка и загрузки могут скрыть и снова показать список за один кадр.
      // ResizeObserver объединяет такие изменения и может не уведомить о них.
      if (!root.offsetParent) schedule();
    },
    /** Повторно отображает видимые строки (выбор или данные изменены). */
    refresh: render,
    scrollToTop() {
      const scrolling = scroller();
      if (scrolling === root) { root.scrollTop = 0; return; }
      // Часть длинного экрана: назад к началу списка, но не дальше его шапки.
      const offset = root.getBoundingClientRect().top - scrolling.getBoundingClientRect().top + scrolling.scrollTop;
      if (scrolling.scrollTop > offset) scrolling.scrollTop = offset;
    },
    /** On the phone layout: calls `fn` (on every draw) while the end of the list is within `margin` px of the screen. */
    onNearEnd(fn, margin = 400) {
      nearEnd.push({ fn, margin });
    },
    get length() {
      return items.length;
    },
  };
}
