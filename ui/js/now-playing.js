import { h, formatTime } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { api } from "./api.js";
import { store, subscribe, own } from "./store.js";
import { createVisualizer, MODES } from "./visualizer.js";
import { createLyricsPanel } from "./lyrics-panel.js";
import { isLiked, isBusy } from "./likes.js";

const IDLE_MS = 3000;
const PALETTES = new Map();

/** Colours for a track without a usable cover: stable, from its name. */
function fallbackColors(track) {
  const text = `${track?.artist ?? ""}${track?.title ?? ""}`;
  let hash = 0;
  for (let i = 0; i < text.length; i += 1) hash = (hash * 31 + text.charCodeAt(i)) >>> 0;
  const hue = hash % 360;
  return [`hsl(${hue} 70% 55%)`, `hsl(${(hue + 50) % 360} 70% 48%)`];
}

function toHex(color) {
  // The canvas and CSS both accept hsl(); the visualizer wants #rrggbb, so
  // resolve through a throwaway canvas pixel.
  if (color.startsWith("#")) return color;
  const probe = document.createElement("canvas").getContext("2d");
  probe.fillStyle = color;
  return probe.fillStyle;
}

/**
 * The full-screen "now playing" view: the cover as a blurred backdrop, big
 * artwork, transport, and a live spectrum drawn from what is really playing.
 */
export function createNowPlaying({ settings, onSettingsChange, onLike }) {
  const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let open = false;
  let dragging = false;
  let idleTimer = 0;
  let paletteToken = 0;
  let coverUrl = null;
  let windowFullscreen = false;
  let lyricsOn = settings.lyrics !== false;
  let mode = MODES.includes(settings.visualizer) ? settings.visualizer : reduceMotion ? "off" : "bars";

  const backdrop = h("div", { class: "np-backdrop" });
  const canvas = h("canvas", { class: "np-canvas", "aria-hidden": "true" });
  const coverBox = h("div", { class: "np-cover" });
  const title = h("div", { class: "np-title" });
  const artist = h("div", { class: "np-artist" });
  const upNext = h("div", { class: "np-next" });
  const likeButton = h("button", { class: "np-btn np-like", type: "button", onclick: () => onLike?.(store.player.track) }, icon("heart", 22));
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
    likeButton.replaceChildren(icon(liked ? "heart-on" : "heart", 22));
  }

  const closeButton = h("button", { class: "np-btn", type: "button", title: t("np.close"), "aria-label": t("np.close"), onclick: () => close() }, icon("chevron", 22));
  const modeButton = h("button", { class: "np-btn", type: "button", onclick: () => cycleMode() }, icon("visualizer", 20));
  const lyricsButton = h("button", { class: "np-btn", type: "button", title: t("ly.toggle"), "aria-label": t("ly.toggle"), "aria-pressed": String(lyricsOn),
    onclick: () => setLyrics(!lyricsOn) }, icon("lyrics", 20));
  const lyrics = createLyricsPanel();
  const windowButton = h("button", { class: "np-btn", type: "button", title: t("np.fullscreen"), "aria-label": t("np.fullscreen"), "aria-pressed": "false",
    onclick: () => toggleWindowFullscreen() }, icon("expand", 20));

  const shuffleButton = h("button", { class: "np-btn", type: "button", title: t("shuffle.toggle"), "aria-label": t("shuffle.toggle"), "aria-pressed": "false",
    onclick: () => api.setShuffle(!store.player.shuffle) }, icon("shuffle", 20));
  const previousButton = h("button", { class: "np-btn", type: "button", title: t("previous"), "aria-label": t("previous"), onclick: () => api.previous() }, icon("prev", 24));
  const playButton = h("button", { class: "np-play", type: "button", title: t("play"), "aria-label": t("play"), onclick: () => api.toggle() }, icon("play", 30));
  const nextButton = h("button", { class: "np-btn", type: "button", title: t("next"), "aria-label": t("next"), onclick: () => api.next() }, icon("next", 24));
  const repeatButton = h("button", { class: "np-btn", type: "button", title: t("repeat.off"), "aria-label": t("repeat.off"),
    onclick: () => api.setRepeat({ off: "all", all: "one", one: "off" }[store.player.repeat] ?? "off") }, icon("repeat", 20));

  const elapsed = h("span", { text: "0:00" });
  const total = h("span", { text: "0:00" });
  const seek = h("input", {
    type: "range", min: "0", max: "0", step: "1", value: "0", "aria-label": t("seek"),
    oninput: () => { dragging = true; elapsed.textContent = formatTime(Number(seek.value)); },
    onchange: () => { api.seek(Number(seek.value)); dragging = false; lyrics.snap(Number(seek.value)); },
  });

  const element = h("div", { class: "np", role: "dialog", "aria-modal": "true", "aria-label": t("np.open"), hidden: true },
    backdrop,
    canvas,
    h("div", { class: "np-top" }, closeButton, h("div", { class: "np-spacer" }), lyricsButton, modeButton, windowButton),
    h("div", { class: "np-stage" }, coverBox, h("div", { class: "np-info" }, title, h("div", { class: "np-artist-row" }, artist, likeButton), upNext), lyrics.element),
    h("div", { class: "np-bottom" },
      h("div", { class: "np-seek" }, elapsed, seek, total),
      h("div", { class: "np-controls" }, shuffleButton, previousButton, playButton, nextButton, repeatButton)));

  const visualizer = createVisualizer(canvas);
  visualizer.setMode(mode);
  visualizer.setAnchor(() => {
    const cover = coverBox.getBoundingClientRect();
    const frame = canvas.getBoundingClientRect();
    return { x: cover.left + cover.width / 2 - frame.left, y: cover.top + cover.height / 2 - frame.top, radius: cover.width / 2 };
  });

  // ---------------------------------------------------------------- modes

  function renderMode() {
    const label = t("np.mode", { mode: t(`np.mode.${mode}`) });
    modeButton.title = label;
    modeButton.setAttribute("aria-label", label);
    element.dataset.mode = mode;
  }

  function syncEngine() {
    // The engine analyses audio only while someone is looking at the result.
    api.setVisualizer(open && mode !== "off" && !document.hidden).catch(() => {});
  }

  function cycleMode() {
    mode = MODES[(MODES.indexOf(mode) + 1) % MODES.length];
    visualizer.setMode(mode);
    onSettingsChange?.({ visualizer: mode });
    renderMode();
    syncEngine();
  }

  function setLyrics(on) {
    lyricsOn = on;
    lyricsButton.setAttribute("aria-pressed", String(on));
    element.dataset.lyrics = on ? "on" : "off";
    lyrics.setVisible(on);
    onSettingsChange?.({ lyrics: on });
    if (on && open) {
      lyrics.setTrack(store.player.track);
      lyrics.snap(store.player.position || 0);
    }
  }

  // -------------------------------------------------------------- palette

  function applyColors(primary, secondary) {
    element.style.setProperty("--np-a", primary);
    element.style.setProperty("--np-b", secondary);
    visualizer.setColors(toHex(primary), toHex(secondary));
  }

  async function updateCover(track) {
    const url = track?.artwork_url ?? null;
    if (url === coverUrl && coverBox.firstChild) return;
    coverUrl = url;
    const token = ++paletteToken;
    if (url) {
      coverBox.replaceChildren(h("img", { src: url, alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" }));
      backdrop.replaceChildren(h("img", { src: url, alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" }));
    } else {
      coverBox.replaceChildren(h("div", { class: "np-placeholder" }, icon("note", 96)));
      backdrop.replaceChildren();
    }
    const [a, b] = fallbackColors(track);
    applyColors(a, b);
    if (!url) return;
    try {
      if (!PALETTES.has(url)) {
        const palette = await api.artworkPalette(url);
        if (PALETTES.size > 100) PALETTES.clear();
        PALETTES.set(url, palette);
      }
      if (token !== paletteToken) return;
      const palette = PALETTES.get(url);
      applyColors(palette.primary, palette.secondary);
    } catch {
      // Keep the colours made from the name.
    }
  }

  // --------------------------------------------------------------- render

  function render(player) {
    const track = player.track;
    title.textContent = track ? track.title : t("nothing");
    artist.textContent = track ? track.artist : "";
    updateCover(track);
    if (lyricsOn) {
      lyrics.setTrack(track);
      lyrics.update(player.position || 0);
    }

    const playing = player.status === "playing" || player.status === "buffering";
    const showPause = playing || player.status === "loading";
    playButton.replaceChildren(icon(showPause ? "pause" : "play", 30));
    const label = showPause ? t("pause") : t("play");
    playButton.title = label;
    playButton.setAttribute("aria-label", label);
    shuffleButton.classList.toggle("on", player.shuffle);
    shuffleButton.setAttribute("aria-pressed", String(player.shuffle));
    repeatButton.classList.toggle("on", player.repeat !== "off");
    const repeatLabel = t(`repeat.${player.repeat}`);
    repeatButton.title = repeatLabel;
    repeatButton.setAttribute("aria-label", repeatLabel);
    repeatButton.replaceChildren(icon("repeat", 20), ...(player.repeat === "one" ? [h("span", { class: "np-one", text: "1" })] : []));

    const duration = Math.max(0, player.duration || 0);
    seek.max = String(Math.max(duration, 1));
    total.textContent = formatTime(duration);
    if (!dragging) {
      seek.value = String(Math.min(player.position || 0, duration || 0));
      elapsed.textContent = formatTime(player.position || 0);
    }
    seek.disabled = !track;
    previousButton.disabled = nextButton.disabled = playButton.disabled = !track && player.queue_len === 0;

    if (!playing && open) visualizer.settle();
    wake();
  }

  function renderQueue() {
    const { tracks, current } = store.queue;
    const next = current != null ? tracks[current + 1] : null;
    upNext.textContent = next ? `${t("np.next")}: ${next.artist} — ${next.title}` : "";
  }

  // ----------------------------------------------------------- idle, keys

  /** Controls hide while music plays and the pointer is still. */
  function wake() {
    clearTimeout(idleTimer);
    element.classList.remove("idle");
    if (open && store.player.status === "playing") {
      idleTimer = setTimeout(() => element.classList.add("idle"), IDLE_MS);
    }
  }

  element.addEventListener("pointermove", wake);
  element.addEventListener("pointerdown", wake);
  element.addEventListener("dblclick", (event) => {
    if (event.target === element || event.target === backdrop || event.target === canvas || event.target.closest(".np-stage")) toggleWindowFullscreen();
  });

  async function toggleWindowFullscreen(force) {
    const next = force ?? !windowFullscreen;
    try {
      await api.setFullscreen(next);
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
  const onVisibility = () => { if (open) syncEngine(); };
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
    element.hidden = false;
    renderMode();
    element.dataset.lyrics = lyricsOn ? "on" : "off";
    lyrics.setVisible(lyricsOn);
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
    element.hidden = true;
    clearTimeout(idleTimer);
    visualizer.stop();
    lyrics.forget();
    syncEngine();
    if (windowFullscreen) toggleWindowFullscreen(false);
  }

  subscribe("player", (player) => { if (open) { render(player); renderLike(); } });
  subscribe("likes", () => { if (open) renderLike(); });
  subscribe("queue", () => { if (open) renderQueue(); });

  return {
    element,
    show,
    close,
    toggle: () => (open ? close() : show()),
    isOpen: () => open,
    /** The settings screen changed a default. */
    applySettings(patch) {
      if (patch.visualizer && MODES.includes(patch.visualizer) && patch.visualizer !== mode) {
        mode = patch.visualizer;
        visualizer.setMode(mode);
        renderMode();
        syncEngine();
      }
      if (typeof patch.lyrics === "boolean" && patch.lyrics !== lyricsOn) setLyrics(patch.lyrics);
    },
    /** From the engine, ~30 times a second while open. */
    feed(bands) { if (open && mode !== "off" && Array.isArray(bands)) visualizer.feed(bands); },
  };
}
