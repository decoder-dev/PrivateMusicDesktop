import { toast } from "./toast.js";
import { h, clear, fold } from "./dom.js";
import { icon } from "./icons.js";
import { t, tn, addStrings, errorText } from "./i18n.js";
import { createVirtualList } from "./virtual-list.js";
import { trackRow, ROW_HEIGHT } from "./track-row.js";
import { store, subscribe } from "./store.js";
import { trackKey } from "./api.js";
import { isLiked, isBusy } from "./likes.js";
import { isDownloaded, isDownloading, downloadAll, downloadJob, cancelDownloads } from "./downloads.js";
import { openArtistOf } from "./navigation.js";
import { artBox, paintStatus, pageHero, searchField, headerOnScroll, setAmbience, fillPanel } from "./ui-parts.js";

/**
 * Общий экран для каждого списка треков: заголовок, дополнительный фильтр, строка состояния и
 * виртуальный список.
 *
 * хуки: onPlay(visibleTracks, index, { shuffle }) onAction(action, track, index) кнопки наведения
 * rowActions [[icon, action, label], …] или undefined для пары по умолчанию isSelected(track, index)
 * по умолчанию: воспроизводимая дорожка onRetry() кнопка "try again" под ошибкой назад, элементы
 * headerExtra для заголовок
 *
 * `heading` — это тег заголовка: "h2", если список является частью страницы. `hero` (вместе с
 * `kicker`) превращает заголовок в карточку героя с обложкой. `playable: false` оставляет кнопки
 * «Воспроизвести» и «Перемешать» отключенными.
 */
// Меню трека используется каждым списком, если только на экране нет собственного (устанавливается
// основным).
addStrings({ ru: { "batch.download": "Скачать все", "batch.cancel": "Остановить ({done}/{total})", "batch.done": "Обработано: {done}. Ошибок: {failed}" }, en: { "batch.download": "Download all", "batch.cancel": "Stop ({done}/{total})", "batch.done": "Processed: {done}. Failed: {failed}" } });
let defaultMenu = null;
export function setDefaultTrackMenu(fn) { defaultMenu = fn; }
let defaultBatch = null;
export function setDefaultBatchAction(fn) { defaultBatch = fn; }
addStrings({ ru: { "select.start":"Выбрать", "select.count":"Выбрано: {n}", "select.all":"Все", "select.clear":"Снять выделение", "select.queue":"В очередь", "select.playlist":"В плейлист", "select.download":"Скачать", "select.remove":"Удалить выбранное" }, en: { "select.start":"Select", "select.count":"Selected: {n}", "select.all":"All", "select.clear":"Clear selection", "select.queue":"Queue", "select.playlist":"Playlist", "select.download":"Download", "select.remove":"Remove selected" } });

export function createTracksScreen({ title, filterable = false, playable = true, heading: level = "h1", hero = false, kicker = "", hooks }) {
  let all = [];
  let visible = [];
  let query = "";
  let lastActivation = { key: "", at: 0 };
  const selected = new Set();
  let selecting = false;
  let anchor = 0;
  let batchBusy = false;
  const selectionCount = h("strong");
  const selectionBar = h("div", { class:"selection-bar", hidden:true, role:"toolbar", "aria-label":t("select.start") }, selectionCount);
  const selectionButton = h("button", {class:"btn select-toggle",type:"button",hidden:true,text:t("select.start"),"aria-pressed":"false",onclick:()=>{ selecting=!selecting; selected.clear(); renderSelection(); }});
  const chosen = () => [...new Map(visible.filter(track => selected.has(trackKey(track))).map(track=>[trackKey(track),track])).values()];
  function renderSelection() {
    selectionBar.hidden = !selecting;
    selectionButton.setAttribute("aria-pressed",String(selecting));
    selectionCount.textContent = t("select.count",{n:chosen().length});
    selectionBar.querySelectorAll("[data-batch]").forEach(button => { button.disabled=batchBusy || !chosen().length;button.hidden=button.dataset.batch==="remove" && hooks.canBatchRemove && !hooks.canBatchRemove(); });
    list.refresh();
  }
  async function runBatch(action) {
    const items=chosen();
    if (batchBusy || !items.length) return;
    batchBusy=true; renderSelection();
    try {
      if (action === "remove") await hooks.onBatchRemove?.(items);
      else if (action === "download") await downloadAll(items);
      else await defaultBatch?.(action,items);
    } catch(error) { toast(errorText(error),{error:true}); }
    finally { batchBusy=false; renderSelection(); }
  }
  selectionBar.append(h("button",{class:"btn",type:"button",text:t("select.all"),onclick:()=>{ visible.forEach(track=>selected.add(trackKey(track))); renderSelection(); }}));
  for (const action of ["queue","playlist","download",...(hooks.onBatchRemove ? ["remove"] : [])]) selectionBar.append(h("button",{class:"btn",type:"button","data-batch":action,text:t(`select.${action}`),onclick:()=>runBatch(action)}));
  selectionBar.append(h("button",{class:"btn",type:"button",text:t("select.clear"),onclick:()=>{selected.clear(); selecting=false; renderSelection();}}));

  const heading = h(level, { text: title });
  const meta = h("span", { class: "meta" });
  const filterPart = filterable
    ? searchField({
        placeholder: t("library.filterShort"), label: t("library.filter"),
        oninput: (event) => { query = fold(event.target.value.trim()); applyFilter(); },
      })
    : null;
  const playButton = playable
    ? h("button", { class: "btn primary", type: "button", hidden: true, onclick: () => visible.length && hooks.onPlay(visible, 0, { shuffle: false }) }, icon("play", 14), t("play"))
    : null;
  const shuffleButton = playable
    ? h("button", { class: "icon-btn circle-btn", type: "button", hidden: true, title: t("shuffle"), "aria-label": t("shuffle"),
        onclick: () => visible.length && hooks.onPlay(visible, Math.floor(Math.random() * visible.length), { shuffle: true }) }, icon("shuffle", 16))
    : null;
  const batchButton = playable ? h("button", { class:"btn",type:"button",text:t("batch.download"),hidden:true,onclick:async () => {
    if (downloadJob()?.running) { cancelDownloads(); return; }
    try {
      const result=await downloadAll(visible.slice());
      if (result) toast(t("batch.done",{done:result.completed,failed:result.failed}),{error:result.failed>0});
    } catch(error) { toast(errorText(error),{error:true}); }
  } }) : null;
  subscribe("downloadJob", () => {
    const job=downloadJob();
    if (batchButton) {
      batchButton.textContent=job?.running ? t("batch.cancel",{done:job.completed,total:job.total}) : t("batch.download");
      batchButton.disabled=!!(job?.running && job.cancelled);
    }
  });
  subscribe("session",session => { if (!session.signed_in) cancelDownloads(); });
  const status = h("div", { class: "status", role: "status", hidden: true });
  const empty = h("div", { class: "empty-state", hidden: true });
  const retry = hooks.onRetry ? h("button", { class: "btn retry", type: "button", onclick: () => hooks.onRetry(), text: t("fy.retryTune") }) : null;

  const list = createVirtualList({
    rowHeight: ROW_HEIGHT,
    renderRow: (track, index) => {
      const row = trackRow(track, {
        index,
        selected: hooks.isSelected ? hooks.isSelected(track, index) : !!store.player.track && trackKey(store.player.track) === trackKey(track),
        paused: store.player.status !== "playing" && store.player.status !== "buffering" && store.player.status !== "loading",
        actions: hooks.rowActions,
        liked: isLiked(track),
        busy: isBusy(track),
        pinLiked: hooks.pinLiked ?? true,
        downloaded: isDownloaded(track),
        downloading: isDownloading(track),
      });
      row.classList.toggle("multi-selected",selected.has(trackKey(track)));
      if (selecting) row.setAttribute("aria-selected",String(selected.has(trackKey(track))));
      return row;
    },
    onRowEvent: (type, event, track, index) => {
      const button = event.target.closest?.("[data-action]");
      if (!button && ((type === "click" && (selecting || event.ctrlKey || event.metaKey || event.shiftKey)) || (type === "keydown" && event.key === " " && selecting))) {
        event.preventDefault(); event.stopPropagation(); selecting=true;
        if (event.shiftKey) for (let i=Math.min(anchor,index);i<=Math.max(anchor,index);i++) selected.add(trackKey(visible[i]));
        else { const key=trackKey(track); if (selected.has(key)) selected.delete(key); else selected.add(key); anchor=index; }
        renderSelection(); return;
      }
      if (type === "contextmenu") {
        event.preventDefault();
        (hooks.onMenu ?? defaultMenu)?.(track, { x: event.clientX, y: event.clientY }, index);
      } else if (type === "click" && button?.dataset.action === "menu") {
        event.stopPropagation();
        const box = button.getBoundingClientRect();
        (hooks.onMenu ?? defaultMenu)?.(track, { x: box.right - 4, y: box.bottom + 2 }, index);
      } else if (type === "click" && button?.dataset.action === "artist") {
        event.stopPropagation();
        openArtistOf(track);
      } else if (type === "click" && button) {
        event.stopPropagation();
        hooks.onAction?.(button.dataset.action, track, index);
      } else if (type === "pm-row-longpress") {
        // Касание: долгое нажатие открывает то же меню, что и правый щелчок, в виде нижней панели.
        (hooks.onMenu ?? defaultMenu)?.(track, { x: event.detail.x, y: event.detail.y }, index);
      } else if (type === "pm-row-swipe") {
        // Свайп вправо: «Далее», дальше: «В конец». Свайп влево: нравится.
        const { dir, far } = event.detail;
        hooks.onAction?.(dir === "lead" ? (far ? "play-last" : "play-next") : "like", track, index);
      } else if (type === "click" && !button && event.detail <= 1) {
        const key = `${trackKey(track)}|${index}`;
        const now = performance.now();
        if (lastActivation.key === key && now - lastActivation.at < 350) return;
        lastActivation = { key, at: now };
        hooks.onPlay?.(visible, index, { shuffle: false });
      } else if (type === "keydown" && event.key === "Enter" && !button) {
        event.preventDefault();
        hooks.onPlay?.(visible, index, { shuffle: false });
      }
    },
  });
  list.element.setAttribute("aria-multiselectable","true");
  list.element.addEventListener("keydown",event=>{
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") { event.preventDefault();event.stopPropagation();selecting=true;visible.forEach(track=>selected.add(trackKey(track)));renderSelection(); }
    if (event.key === "Escape" && selecting) { event.preventDefault();event.stopPropagation();selected.clear();selecting=false;renderSelection(); }
  });

  const heroArt = hero ? h("span", { class: "hero-art page-art" }, artBox(null, { glyph: "playlist", size: 40 })) : null;
  const note = hero ? h("div", { class: "page-note", hidden: true }) : null;
  const header = hero
    ? pageHero({
        back: hooks.back, art: heroArt,
        body: [h("div", { class: "kicker", text: kicker }), heading, meta, note],
        actions: [playButton, shuffleButton, batchButton, hooks.headerExtra].filter(Boolean),
      })
    : h("header", { class: "view-header" }, hooks.back ?? null,
        h("div", { class: "vh-text" }, heading, meta), hooks.headerExtra ?? null, filterPart?.element ?? null,
        playable ? h("div", { class: "vh-actions" }, playButton, shuffleButton, batchButton) : null);
  headerOnScroll(header, list.element);

  const element = h("section", { class: "view" }, header, status, empty, list.element);
  header.append(selectionButton);
  header.after(selectionBar);

  function applyFilter({ reset = true } = {}) {
    visible = query
      ? all.filter((track) => fold(track.title).includes(query) || fold(track.artist).includes(query))
      : all;
    list.setItems(visible);
    const available=new Set(visible.map(trackKey));
    anchor=Math.min(anchor,Math.max(0,visible.length-1));
    for (const key of selected) if (!available.has(key)) selected.delete(key);
    renderSelection();
    if (reset) list.scrollToTop();
    renderEmpty();
  }

  /** Панели заменяют (пустой) список; список отображается снова, как только есть что перечислить. */
  function renderState() {
    const panel = (!status.hidden && !status.classList.contains("progress")) || !empty.hidden;
    list.element.parentElement?.classList.toggle("state-on", panel);
  }

  function renderEmpty() {
    const show = visible.length === 0 && !status.textContent;
    empty.hidden = !show;
    if (show) fillPanel(empty, all.length === 0 ? (hooks.emptyText ?? "") : t("library.noMatches"), all.length === 0 ? hooks.art : "search");
    // Никаких мертвых кнопок: когда не во что играть, они вообще не отображаются.
    for (const button of [playButton, shuffleButton, batchButton,selectionButton]) if (button) button.hidden = visible.length === 0;
    renderState();
  }

  // Сохраняйте подсветку "playing" и глиф паузы текущими, не перестраивая что-либо еще.
  let lastKey = null;
  subscribe("player", (player) => {
    const key = `${player.track ? trackKey(player.track) : ""}|${player.status === "paused"}`;
    if (key !== lastKey) {
      lastKey = key;
      list.refresh();
    }
  });

  subscribe("likes", () => list.refresh());
  subscribe("downloads", () => list.refresh());

  return {
    element,
    list,
    header,
    setTitle(text) { heading.textContent = text; },
    setKicker(text) { const node = header.querySelector(".kicker"); if (node) node.textContent = text; },
    setCover(url) { heroArt?.replaceChildren(artBox(url, { fallbackText: "" })); if (hero) setAmbience(header, url); },
    setNote(text) { if (note) { note.textContent = text ?? ""; note.hidden = !text; } },
    setTracks(tracks, { preserveScroll = false } = {}) {
      all = tracks;
      applyFilter({ reset: !preserveScroll });
      meta.textContent = tracks.length ? tn("library.count", tracks.length) : "";
    },
    /** Показывает прогрессивную загрузку, не выбрасывая список. */
    setProgress(tracks, total) {
      all = tracks;
      visible = query ? all.filter((tr) => fold(tr.title).includes(query) || fold(tr.artist).includes(query)) : all;
      list.setItems(visible);
      paintStatus(status, t("library.loaded", { n: tracks.length, total }), { extra: "progress" });
      renderEmpty();
    },
    setStatus(text, { error = false } = {}) {
      paintStatus(status, text, { error });
      // Неудачную загрузку можно повторить; кнопка живет внутри панели.
      if (error && text && retry) status.append(retry);
      renderEmpty();
    },
    clearStatus() {
      paintStatus(status, "");
      renderEmpty();
    },
    get visible() { return visible; },
    refresh: () => list.refresh(),
  };
}

export { clear };
