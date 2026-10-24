import { toggleWindowFullscreen as changeWindowFullscreen, isWindowFullscreen } from "./window-mode.js";
import { h, formatTime } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { api } from "./api.js";
import { store, subscribe, own } from "./store.js";
import { createVisualizer, MODES } from "./visualizer.js";
import { createLyricsPanel } from "./lyrics-panel.js";
import { isLiked, isBusy } from "./likes.js";
import { hasArtist, openArtistOf } from "./navigation.js";
import { motionOk, onMotionChange, afterAnimation, morph } from "./motion.js";
import { coverPalette } from "./palette.js";
import { createEqualizerPanel } from "./equalizer.js";
import { openSheet } from "./mobile/sheet.js";
import { artBox, bindRange, syncRange } from "./ui-parts.js";
import { isPhone, isAndroid, onLayoutChange } from "./mobile/layout.js";
import { attachSheetDrag } from "./mobile/np-sheet.js";

const IDLE_MS = 3000;
const QUEUE_ROWS = 100;

/** Цвета для трека без обложки: стабильные, судя по названию. */
function fallbackColors(track) {
  const text = `${track?.artist ?? ""}${track?.title ?? ""}`;
  let hash = 0;
  for (let i = 0; i < text.length; i += 1) hash = (hash * 31 + text.charCodeAt(i)) >>> 0;
  const hue = hash % 360;
  return [`hsl(${hue} 70% 55%)`, `hsl(${(hue + 50) % 360} 70% 48%)`];
}

function toHex(color) {
  // Canvas и CSS принимают hsl(); визуализатору нужен #rrggbb, поэтому разрешите его с помощью
  // одноразового пикселя холста.
  if (color.startsWith("#")) return color;
  const probe = document.createElement("canvas").getContext("2d");
  probe.fillStyle = color;
  return probe.fillStyle;
}

/**
 * Полноэкранный вид "now playing": обложка в виде размытого фона, большие иллюстрации, транспорт и
 * живой спектр, взятый из того, что действительно играет.
 */
export function createNowPlaying({ settings, onSettingsChange, onLike, onMenu }) {
  let open = false;
  let queueOpen = false;
  let userChoseMode = MODES.includes(settings.visualizer);
  let dragging = false;
  let idleTimer = 0;
  let paletteToken = 0;
  let coverUrl = null;
  let windowFullscreen = false;
  let lyricsOn = settings.lyrics !== false;
  let mode = MODES.includes(settings.visualizer) ? settings.visualizer : motionOk() ? "bars" : "off";

  // Две сложенные друг на друга копии обложки размером 240 пикселей каждая, размытые и увеличенные; они
  // плавно исчезают при смене трека.
  const layers = [0, 1].map(() => h("img", { class: "np-bd", alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" }));
  let frontLayer = 0;
  let backdropToken = 0;
  const backdrop = h("div", { class: "np-backdrop" }, layers);
  const canvas = h("canvas", { class: "np-canvas", "aria-hidden": "true" });
  const coverBox = h("div", { class: "np-cover" });
  const title = h("div", { class: "np-title" });
  const artist = h("button", { class: "np-artist link-text", type: "button", title: t("artist.open"), disabled: true,
    onclick: () => { const track = store.player.track; close(); openArtistOf(track); } });
  const upNext = h("div", { class: "np-next" });
  const kickerAlbum = h("div", { class: "k2" });
  const likeButton = h("button", { class: "np-btn np-like", type: "button", onclick: () => onLike?.(store.player.track) }, icon("heart", 20));
  function renderLike() {
    const track = store.player.track;
    likeButton.hidden = !track;
    if (!track) return;
    const liked = isLiked(track);
    const label = liked ? t("like.remove") : t("like.add");
    likeButton.classList.toggle("on", liked);
    likeButton.title = label;
    likeButton.setAttribute("aria-label", label);
    likeButton.setAttribute("aria-pressed", String(liked));
    likeButton.disabled = isBusy(track);
    if(likeButton.dataset.liked!==String(liked)) {
      likeButton.dataset.liked=String(liked);
      likeButton.replaceChildren(icon(liked ? "heart-on" : "heart",20));
    }
  }

  const closeButton = h("button", { class: "np-btn", type: "button", title: t("np.close"), "aria-label": t("np.close"), onclick: () => close() }, icon("chevron", 22));
  const modeButton = h("button", { class: "np-btn np-mode", type: "button", onclick: () => cycleMode() }, icon("visualizer", 20));
  const lyricsButton = h("button", { class: "np-btn", type: "button", title: t("ly.toggle"), "aria-label": t("ly.toggle"), "aria-pressed": String(lyricsOn),
    onclick: () => (queueOpen ? showLyricsPanel() : setLyrics(!lyricsOn)) }, icon("lyrics", 20));
  const lyrics = createLyricsPanel();
  const windowButton = h("button", { class: "np-btn desktop-only", type: "button", title: t("np.fullscreen"), "aria-label": t("np.fullscreen"), "aria-pressed": "false",
    onclick: () => toggleWindowFullscreen() }, icon("expand", 20));

  const shuffleButton = h("button", { class: "np-btn", type: "button", title: t("shuffle.toggle"), "aria-label": t("shuffle.toggle"), "aria-pressed": "false",
    onclick: () => api.setShuffle(!store.player.shuffle) }, icon("shuffle", 20));
  const previousButton = h("button", { class: "np-btn", type: "button", title: t("previous"), "aria-label": t("previous"), onclick: () => api.previous() }, icon("prev", 24));
  const playButton = h("button", { class: "np-play", type: "button", title: t("play"), "aria-label": t("play"), onclick: () => api.toggle() }, icon("play", 28));
  const nextButton = h("button", { class: "np-btn", type: "button", title: t("next"), "aria-label": t("next"), onclick: () => api.next() }, icon("next", 24));
  const repeatButton = h("button", { class: "np-btn", type: "button", title: t("repeat.off"), "aria-label": t("repeat.off"),
    onclick: () => api.setRepeat({ off: "all", all: "one", one: "off" }[store.player.repeat] ?? "off") }, icon("repeat", 20));

  const elapsed = h("span", { text: "0:00" });
  const remaining = (seconds) => `-${formatTime(Math.max(0, seconds))}`;
  const total = h("span", { text: remaining(0) });
  const seek = h("input", {
    type: "range", min: "0", max: "0", step: "1", value: "0", "aria-label": t("seek"),
    oninput: () => { dragging = true; elapsed.textContent = formatTime(Number(seek.value)); total.textContent = remaining(Number(seek.max) - Number(seek.value)); },
    onchange: () => { api.seek(Number(seek.value)); dragging = false; lyrics.snap(Number(seek.value)); },
  });
  bindRange(seek);

  // Быстрые действия под транспортом: тексты песен, очередь и меню трека.
  const quick = (name, label, onclick, pressed) => {
    const button = h("button", { type: "button", onclick, ...(pressed === undefined ? {} : { "aria-pressed": String(pressed) }) },
      h("span", { class: "q-ico" }, icon(name, 18)), h("span", { text: label }));
    return button;
  };
  const quickLyrics = quick("lyrics", t("np.lyrics"), () => (queueOpen ? showLyricsPanel() : setLyrics(!lyricsOn)), lyricsOn);
  const quickQueue = quick("queue", t("np.queue"), () => setQueueOpen(!queueOpen), false);
  const quickMore = quick("more", t("np.more"), () => {
    const box = quickMore.getBoundingClientRect();
    const track = store.player.track;
    if (track) onMenu?.(track, { x: box.left, y: box.top });
  });

  const quickEq = quick("sliders", t("eq.title"), () => {
    const panel = createEqualizerPanel({ settings, onSettingsChange });
    openSheet({ label: t("eq.title"), className: "eq-sheet", onClose: panel.destroy, content: [h("div", { class: "sheet-title", text: t("eq.title") }), panel.element] });
    panel.render?.();
  });

  const queueList = h("div", { class: "nq-list" });
  const queuePanel = h("section", { class: "np-queue", "aria-label": t("np.queue"), hidden: true },
    h("div", { class: "nq-head", text: t("np.queue") }), queueList);
  queueList.addEventListener("click", (event) => {
    const row = event.target.closest?.(".nq-row");
    if (row) api.jump(Number(row.dataset.index));
  });

  // The sheet's grabber (phone layout only): drag it, or anything not a control, down to close.
  const grab = h("div", { class: "np-grab", "aria-hidden": "true" }, h("i"));
  const heading = h("div", { class: "np-heading" }, title);
  const transport = h("div", { class: "np-controls" }, shuffleButton, previousButton, playButton, nextButton, repeatButton, likeButton);
  const element = h("div", { class: "np", role: "dialog", "aria-modal": "true", "aria-label": t("np.open"), hidden: true },
    backdrop,
    canvas,
    grab,
    h("div", { class: "np-top" }, closeButton, h("div", { class: "np-kicker" }, h("div", { class: "k1", text: t("np.nowPlaying") }), kickerAlbum), lyricsButton, modeButton, windowButton),
    h("div", { class: "np-stage" },
      h("div", { class: "np-main" }, coverBox, h("div", { class: "np-info" }, heading, h("div", { class: "np-artist-row" }, artist), upNext)),
      lyrics.element, queuePanel),
    h("div", { class: "np-bottom" },
      h("div", { class: "np-seek" }, elapsed, seek, total),
      transport,
      h("div", { class: "np-quick" }, quickLyrics, quickQueue, quickEq, quickMore)));

  // One favourite button: beside the title on phones, after Repeat on desktop.
  const placeFavorite = () => (isPhone() ? heading : transport).append(likeButton);
  placeFavorite();
  own(onLayoutChange(placeFavorite));

  const visualizer = createVisualizer(canvas);
  visualizer.setMode(mode);
  visualizer.setAnchor(() => {
    const cover = coverBox.getBoundingClientRect();
    const frame = canvas.getBoundingClientRect();
    const stage=element.querySelector(".np-stage").getBoundingClientRect();
    const main=element.querySelector(".np-main").getBoundingClientRect();
    const info=element.querySelector(".np-info").getBoundingClientRect();
    const bounds={left:Math.max(stage.left+12,main.left)-frame.left,top:stage.top+8-frame.top,right:Math.min(stage.right-12,main.right)-frame.left,bottom:stage.bottom-8-frame.top};
    if (info.left >= cover.right-1) bounds.right=Math.min(bounds.right,info.left-frame.left-20);
    else if (info.top >= cover.bottom-1) bounds.bottom=Math.min(bounds.bottom,info.top-frame.top-16);
    return { x:cover.left+cover.width/2-frame.left,y:cover.top+cover.height/2-frame.top,radius:cover.width/2,bounds };
  });

  visualizer.setPlotBounds(() => {
    const frame=canvas.getBoundingClientRect();
    const info=element.querySelector(".np-info").getBoundingClientRect();
    const bottom=element.querySelector(".np-bottom").getBoundingClientRect();
    const top=info.bottom+8-frame.top;
    const lane=seek.getBoundingClientRect();
    return {top,height:Math.max(0,bottom.top-frame.top-8-top),left:lane.left-frame.left,width:lane.width};
  });

  // -------------------------------------------------- Режимы

  function renderMode() {
    const previousMode = element.dataset.mode;
    const label = t("np.mode", { mode: t(`np.mode.${mode}`) });
    modeButton.title = label;
    modeButton.setAttribute("aria-label", label);
    element.dataset.mode = mode;
    visualizer.redraw();
    if (open && previousMode && previousMode !== mode && motionOk()) {
      canvas.getAnimations().forEach(animation => animation.cancel());
      canvas.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 200, easing: "ease-out" });
    }
  }

  function syncEngine() {
    // Движок анализирует звук только тогда, когда кто-то смотрит на результат.
    api.setVisualizer(open && mode !== "off" && !document.hidden && (!isPhone() || element.dataset.side === "none")).catch(() => {});
  }

  function cycleMode() {
    mode = MODES[(MODES.indexOf(mode) + 1) % MODES.length];
    userChoseMode = true;
    visualizer.setMode(mode);
    onSettingsChange?.({ visualizer: mode });
    renderMode();
    syncEngine();
  }

  /** Тексты песен и очередь находятся на одной боковой панели; очередь выигрывает, пока она открыта. */
  function renderSide() {
    const side = queueOpen ? "queue" : lyricsOn ? "lyrics" : "none";
    element.dataset.side = side;
    element.dataset.lyrics = lyricsOn ? "on" : "off";
    lyrics.setVisible(side === "lyrics");
    queuePanel.hidden = side !== "queue";
    lyricsButton.setAttribute("aria-pressed", String(side === "lyrics"));
    quickLyrics.setAttribute("aria-pressed", String(side === "lyrics"));
    quickQueue.setAttribute("aria-pressed", String(side === "queue"));
    if (side === "queue") renderQueuePanel();
    visualizer.redraw();
    if (open) syncEngine();
  }

  function setLyrics(on) {
    lyricsOn = on;
    renderSide();
    onSettingsChange?.({ lyrics: on });
    if (on && open) {
      lyrics.setTrack(store.player.track);
      lyrics.snap(store.player.position || 0);
    }
  }

  function showLyricsPanel() {
    queueOpen = false;
    setLyrics(true);
  }

  function setQueueOpen(on) {
    queueOpen = on;
    renderSide();
  }

  function renderQueuePanel() {
    if (!open || queuePanel.hidden) return;
    const { tracks, current } = store.queue;
    queueList.replaceChildren();
    if (!tracks.length || current == null) {
      queueList.append(h("div", { class: "nq-empty", text: t("np.queue.empty") }));
      return;
    }
    const row = (track, index) => {
      const button = h("button", { type: "button", class: `nq-row${index === current ? " current" : ""}`, "data-index": String(index), ...(index === current ? { "aria-current": "true" } : {}) },
        artBox(track.artwork_url, { size: 16 }), h("span", { class: "t" }, h("b", { text: track.title }), h("span", { text: track.artist })));
      return button;
    };
    queueList.append(h("div", { class: "nq-label", text: t("np.queue.now") }), row(tracks[current], current));
    const upcoming = tracks.slice(current + 1, current + 1 + QUEUE_ROWS);
    if (upcoming.length) queueList.append(h("div", { class: "nq-label", text: t("np.queue.next") }), ...upcoming.map((track, i) => row(track, current + 1 + i)));
  }

  // -------------------------------------------------------------- Палитра

  function applyColors(primary, secondary) {
    element.style.setProperty("--np-a", primary);
    element.style.setProperty("--np-b", secondary);
    visualizer.setColors(toHex(primary), toHex(secondary));
  }

  function layoutBackdrop() {
    backdrop.style.setProperty("--bd-scale", String(Math.max(window.innerWidth, window.innerHeight) / 240 * 1.3));
  }

  /** Новая обложка затеняется поверх старой. */
  async function setBackdrop(url) {
    const mine = ++backdropToken;
    if (!url) {
      for (const layer of layers) layer.classList.remove("show");
      return;
    }
    const next = layers[1 - frontLayer];
    next.src = url;
    try { await next.decode(); } catch {
      if (mine === backdropToken) for (const layer of layers) layer.classList.remove("show");
      return;
    }
    if (mine !== backdropToken) return;
    next.classList.add("show");
    layers[frontLayer].classList.remove("show");
    frontLayer = 1 - frontLayer;
  }

  async function updateCover(track) {
    const url = track?.artwork_url ?? null;
    if (url === coverUrl && coverBox.firstChild) return;
    coverUrl = url;
    const token = ++paletteToken;
    if (url) {
      let failed = false;
      const image = h("img", { src: url, alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" });
      image.addEventListener("error", () => { failed = true; if (token === paletteToken) coverBox.replaceChildren(h("div", { class: "np-placeholder", "aria-hidden": "true", text: (track?.artist || track?.title || "♪").slice(0, 1) })); });
      // Старая обложка остается до тех пор, пока не раскодируется новая.
      // Новая обложка проявляется поверх старой (кроссфейд), затем старая убирается.
      const swap = () => {
        if (token !== paletteToken || failed) return;
        const old = [...coverBox.children];
        if (!old.length || motionOk() === false) { coverBox.replaceChildren(image); return; }
        image.classList.add("np-fade-in");
        coverBox.append(image);
        setTimeout(() => { if (token === paletteToken) { for (const node of old) node.remove(); image.classList.remove("np-fade-in"); } }, 520);
      };
      if (coverBox.firstChild) image.decode().catch(() => {}).finally(swap); else swap();
    } else {
      coverBox.replaceChildren(h("div", { class: "np-placeholder", "aria-hidden": "true", text: (track?.artist || track?.title || "♪").slice(0, 1) }));
    }
    setBackdrop(url);
    const [a, b] = fallbackColors(track);
    applyColors(a, b);
    if (!url) return;
    try {
      const palette = await coverPalette(url);
      if (token !== paletteToken) return;
      applyColors(palette.primary, palette.secondary);
    } catch {
      // Сохраняйте цвета, сделанные из названия.
    }
  }

  // -------------------------------------------------------------- рендеринг

  function render(player) {
    const track = player.track;
    title.textContent = track ? track.title : t("nothing");
    artist.textContent = track ? track.artist : "";
    artist.disabled = !hasArtist(track);
    kickerAlbum.textContent = track ? `${track.artist} — ${track.title}` : "";
    updateCover(track);
    if (lyricsOn) {
      lyrics.setTrack(track);
      lyrics.update(player.position || 0);
    }

    const playing = player.status === "playing" || player.status === "buffering";
    element.dataset.playing = String(player.status === "playing" || player.status === "buffering");
    const showPause = playing || player.status === "loading";
    const spinning = player.status === "buffering" || player.status === "loading";
    const changedGlyph = playButton.dataset.glyph !== (showPause ? "pause" : "play");
    playButton.dataset.glyph = showPause ? "pause" : "play";
    if (changedGlyph || playButton.classList.contains("is-buffering") !== spinning) playButton.replaceChildren(icon(showPause ? "pause" : "play", 28), ...(spinning ? [h("span", { class: "spin-over", "aria-hidden": "true" }, h("span", { class: "spinner" }))] : []));
    playButton.classList.toggle("is-buffering", spinning);
    if (changedGlyph && playButton.dataset.ready) morph(playButton.firstChild);
    playButton.dataset.ready = "1";
    const label = showPause ? t("pause") : t("play");
    playButton.title = label;
    playButton.setAttribute("aria-label", label);
    shuffleButton.classList.toggle("on", player.shuffle);
    shuffleButton.setAttribute("aria-pressed", String(player.shuffle));
    repeatButton.classList.toggle("on", player.repeat !== "off");
    const repeatLabel = t(`repeat.${player.repeat}`);
    repeatButton.title = repeatLabel;
    repeatButton.setAttribute("aria-label", repeatLabel);
    if (repeatButton.dataset.repeat !== player.repeat) {
      repeatButton.dataset.repeat = player.repeat;
      repeatButton.replaceChildren(icon("repeat", 20), ...(player.repeat === "one" ? [h("span", { class: "np-one", text: "1" })] : []));
    }
    quickMore.disabled = !track;

    const duration = Math.max(0, player.duration || 0);
    seek.max = String(Math.max(duration, 1));
    if (!dragging) {
      seek.value = String(Math.min(player.position || 0, duration || 0));
      elapsed.textContent = formatTime(player.position || 0);
      total.textContent = remaining(duration - (player.position || 0));
    }
    syncRange(seek);
    seek.disabled = !track;
    previousButton.disabled = nextButton.disabled = playButton.disabled = !track && player.queue_len === 0;

    if (!playing && open) visualizer.settle();
    wake();
  }

  function renderQueue() {
    const { tracks, current } = store.queue;
    const next = current != null ? tracks[current + 1] : null;
    upNext.textContent = next ? `${t("np.next")}: ${next.artist} — ${next.title}` : "";
    renderQueuePanel();
  }

  // ----------------------------------------------------------- Простой, ключи

  /** Элементы управления скрываются во время воспроизведения музыки, а указатель неподвижен. */
  function wake() {
    clearTimeout(idleTimer);
    element.classList.remove("idle");
    // A finger has no "still pointer": on the phone the controls stay.
    if (open && store.player.status === "playing" && !isPhone()) {
      idleTimer = setTimeout(() => element.classList.add("idle"), IDLE_MS);
    }
  }

  element.addEventListener("pointermove", wake);
  element.addEventListener("pointerdown", wake);
  element.addEventListener("dblclick", (event) => {
    if (isPhone()) return;
    if (event.target === element || event.target === backdrop || event.target === canvas || event.target.closest(".np-stage")) toggleWindowFullscreen();
  });

  async function toggleWindowFullscreen(force) {
    const next = force ?? !isWindowFullscreen();
    try {
      await changeWindowFullscreen(next);
      windowFullscreen = next;
    } catch {
      windowFullscreen = false;
    }
    windowButton.setAttribute("aria-pressed", String(windowFullscreen));
    windowButton.replaceChildren(icon(windowFullscreen ? "collapse" : "expand", 20));
  }

  function onKey(event) {
    if (!open || event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === "l" || event.key === "L") {
      event.preventDefault();
      setLyrics(!lyricsOn);
    } else if (event.key === "v" || event.key === "V") {
      event.preventDefault();
      cycleMode();
    } else if (event.key === "F11") {
      event.preventDefault();
      toggleWindowFullscreen();
    } else if (event.key === "f" || event.key === "F") {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
    wake();
  }
  const onVisibility = () => {
    if (!open) return;
    if (document.hidden) visualizer.stop(); else visualizer.start();
    syncEngine();
  };
  document.addEventListener("keydown", onKey, true);
  document.addEventListener("visibilitychange", onVisibility);
  own(() => {
    document.removeEventListener("keydown", onKey, true);
    document.removeEventListener("visibilitychange", onVisibility);
    visualizer.destroy();
  });

  // ----------------------------------------------------------- open/close

  function show() {
    if (open) return;
    open = true;
    // The phone's player opens on the cover; lyrics and the queue are one tap away.
    if (isPhone()) { lyricsOn = false; queueOpen = false; }
    element.classList.remove("closing");
    document.documentElement.dataset.np = "open";
    element.style.removeProperty("--dy");
    element.classList.remove("dragging");
    element.hidden = false;
    layoutBackdrop();
    renderMode();
    renderSide();
    visualizer.start();
    render(store.player);
    renderLike();
    renderQueue();
    syncEngine();
    wake();
    playButton.focus({ preventScroll: true });
  }

  function close() {
    if (!open) return;
    open = false;
    clearTimeout(idleTimer);
    element.classList.remove("idle");
    visualizer.stop();
    lyrics.forget();
    syncEngine();
    if (isWindowFullscreen()) toggleWindowFullscreen(false);
    // Простыня оседает; при выключении движения он запускается сразу.
    element.classList.add("closing");
    afterAnimation(element, () => {
      if (open) return;
      element.hidden = true;
      element.classList.remove("closing");
      delete document.documentElement.dataset.np;
    }, 450);
  }

  const redrawLayout = new ResizeObserver(() => { if (open) visualizer.redraw(); });
  redrawLayout.observe(coverBox);
  redrawLayout.observe(element.querySelector(".np-info"));
  redrawLayout.observe(element.querySelector(".np-bottom"));
  redrawLayout.observe(element.querySelector(".np-stage"));
  own(() => redrawLayout.disconnect());
  const onWindowMode = () => { windowFullscreen=isWindowFullscreen();windowButton.setAttribute("aria-pressed",String(windowFullscreen));windowButton.replaceChildren(icon(windowFullscreen?"collapse":"expand",20)); };
  window.addEventListener("pm-window-mode",onWindowMode);
  own(()=>window.removeEventListener("pm-window-mode",onWindowMode));
  attachSheetDrag(element, () => close());
  const onResize = () => { if (open) layoutBackdrop(); };
  window.addEventListener("resize", onResize);
  // Уменьшение движения можно включить во время работы приложения: затем остановите спектр.
  const stopWatching = onMotionChange((ok) => {
    if (ok || mode === "off" || userChoseMode) return;
    mode = "off";
    visualizer.setMode(mode);
    renderMode();
    syncEngine();
  });
  own(() => { window.removeEventListener("resize", onResize); stopWatching(); });

  subscribe("player", (player) => { if (open) { render(player); renderLike(); } });
  subscribe("likes", () => { if (open) renderLike(); });
  subscribe("queue", () => { if (open) renderQueue(); });

  return {
    element,
    show,
    close,
    toggle: () => (open ? close() : show()),
    isOpen: () => open,
    /** Экран настроек изменил значение по умолчанию. */
    applySettings(patch) {
      if (patch.visualizer && MODES.includes(patch.visualizer) && patch.visualizer !== mode) {
        mode = patch.visualizer;
        userChoseMode = true;
        visualizer.setMode(mode);
        renderMode();
        syncEngine();
      }
      if (typeof patch.lyrics === "boolean" && patch.lyrics !== lyricsOn) setLyrics(patch.lyrics);
    },
    /** От двигателя ~30 раз в секунду при открытом состоянии. */
    feed(bands) { if (open && !document.hidden && mode !== "off" && (!isPhone() || element.dataset.side === "none") && Array.isArray(bands)) visualizer.feed(bands); },
  };
}
