import { h } from "./dom.js";
import { t } from "./i18n.js";
import { api, trackKey } from "./api.js";
import { motionOk } from "./motion.js";

const CACHE_LIMIT = 60;
/** Строка считается текущей незадолго до ее отметки, поэтому она загорается во время пения. */
const LEAD_SECONDS = 0.25;
/**
 * После того, как пользователь прокручивает вручную, представление остается на месте в течение этого
 * времени.
 */
const HAND_SCROLL_MS = 4000;

/**
 * Столбец текста в полноэкранном режиме: временные строки, следующие за песней (щелкните одну из них,
 * чтобы перейти туда), или простой текст, если время не установлено.
 */
export function createLyricsPanel() {
  const cache = new Map();
  let token = 0;
  let current = null; // трек, который показывается
  let lines = []; // [{ время, узел }]
  let active = -1;
  let handScrollUntil = 0;
  let autoFollow = readAutoFollow();

  const body = h("div", { class: "ly-body" });
  const source = h("div", { class: "ly-source" });
  const currentButton = h("button", { class: "ly-control", type: "button", text: t("ly.current"), hidden: true, onclick: () => snap(lastPosition) });
  const followButton = h("button", { class: "ly-control", type: "button", onclick: () => setAutoFollow(!autoFollow) });
  const controls = h("div", { class: "ly-controls" }, source, currentButton, followButton);
  const element = h("section", { class: "ly", "aria-label": t("ly.label"), hidden: true }, body, controls);
  let lastPosition = 0;

  function renderControls() {
    followButton.textContent = autoFollow ? t("ly.followOn") : t("ly.followOff");
    followButton.setAttribute("aria-pressed", String(autoFollow));
    currentButton.hidden = autoFollow || lines.length === 0;
  }

  function setAutoFollow(on) {
    autoFollow = on;
    try { localStorage.setItem("lyrics.autoFollow", String(on)); } catch { /* хранилище можно отключить */ }
    renderControls();
    if (on) snap(lastPosition);
  }

  const markHandScroll = () => { handScrollUntil = Date.now() + HAND_SCROLL_MS; };
  body.addEventListener("wheel", markHandScroll, { passive: true });
  body.addEventListener("touchmove", markHandScroll, { passive: true });
  body.addEventListener("pointerdown", markHandScroll);

  function message(text, retry) {
    lines = [];
    active = -1;
    source.textContent = "";
    body.replaceChildren(h("div", { class: "ly-message" }, text,
      retry ? h("button", { class: "np-chip", type: "button", onclick: retry, text: t("ly.retry") }) : null));
    renderControls();
  }

  function show(lyrics) {
    active = -1;
    source.textContent = t("ly.source", { source: lyrics.source });
    if (lyrics.lines.length) {
      lines = lyrics.lines.map((line, index) => ({
        time: line.time,
        node: h("button", {
          type: "button", class: line.text ? "ly-line" : "ly-line gap", "data-i": String(index),
          onclick: () => api.seek(line.time),
        }, line.text || "♪"),
      }));
      body.replaceChildren(h("div", { class: "ly-pad" }), ...lines.map((l) => l.node), h("div", { class: "ly-pad" }));
      body.classList.remove("plain");
    } else {
      lines = [];
      body.classList.add("plain");
      body.replaceChildren(h("div", { class: "ly-text", text: lyrics.text }));
      body.scrollTop = 0;
    }
    renderControls();
  }

  async function load(track) {
    const key = trackKey(track);
    const mine = ++token;
    if (cache.has(key)) {
      const cached = cache.get(key);
      if (cached) show(cached); else message(t("ly.none"));
      return;
    }
    message(t("ly.loading"));
    try {
      const lyrics = await api.lyrics(track);
      if (cache.size >= CACHE_LIMIT) cache.clear();
      cache.set(key, lyrics ?? null);
      if (mine !== token) return;
      if (lyrics) show(lyrics); else message(t("ly.none"));
    } catch (error) {
      if (mine !== token) return;
      const unreachable = ["offline", "timeout", "network"].includes(error && error.kind);
      message(t(unreachable ? "ly.errorNetwork" : "ly.error"), () => load(track));
    }
  }

  /** Индекс исполняемой строки по адресу `position` (-1 перед первой). */
  function indexAt(position) {
    let low = 0;
    let high = lines.length - 1;
    let found = -1;
    while (low <= high) {
      const mid = (low + high) >> 1;
      if (lines[mid].time <= position + LEAD_SECONDS) { found = mid; low = mid + 1; } else high = mid - 1;
    }
    return found;
  }

  function follow(position, { force = false } = {}) {
    lastPosition = position;
    if (!lines.length) return;
    const index = indexAt(position);
    if (index === active && !force) return;
    if (active >= 0) lines[active]?.node.classList.remove("active");
    active = index;
    if (index < 0) return;
    const node = lines[index].node;
    node.classList.add("active");
    if ((!autoFollow || Date.now() < handScrollUntil) && !force) return;
    const top = node.offsetTop - body.clientHeight / 2 + node.offsetHeight / 2;
    body.scrollTo({ top, behavior: !motionOk() || force ? "auto" : "smooth" });
  }

  function snap(position) {
    handScrollUntil = 0;
    follow(position, { force: true });
  }

  return {
    element,
    /** Показывает текст песни `track` (или удаляет его, если его нет). */
    setTrack(track) {
      if (!track) { token += 1; current = null; message(""); return; }
      if (current && trackKey(current) === trackKey(track)) return;
      current = track;
      load(track);
    },
    setVisible(on) { element.hidden = !on; },
    update(position) { follow(position); },
    /** После поиска или появления панели: прыгайте, не планируя. */
    snap,
    forget() { current = null; token += 1; },
  };
}

function readAutoFollow() {
  try { return localStorage.getItem("lyrics.autoFollow") !== "false"; }
  catch { return true; }
}
