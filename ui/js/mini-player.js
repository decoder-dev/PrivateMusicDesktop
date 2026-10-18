import { ft } from "./feature-i18n.js";
import { h, formatTime } from "./dom.js";
import { api } from "./api.js";
import { icon } from "./icons.js";
import { store, subscribe } from "./store.js";
import { isLiked } from "./likes.js";
import { artBox } from "./ui-parts.js";
import { setWindowMode } from "./window-mode.js";
import { toast } from "./toast.js";
import { errorText } from "./i18n.js";
export function createMiniPlayer(onLike) {
  const safe = (fn) => () =>
    Promise.resolve()
      .then(fn)
      .catch((error) => toast(errorText(error), { error: true }));
  const art = h("div", { class: "mini-art" });
  const title = h("strong");
  const artist = h("span");
  const button = (label, glyph, run) =>
    h(
      "button",
      {
        type: "button",
        class: "icon-btn",
        "aria-label": label,
        title: label,
        onclick: safe(run),
      },
      icon(glyph, 20),
    );
  const toggle = button(ft("Воспроизведение / пауза"), "play", () => api.toggle());
  const like = button(ft("Избранное"), "heart", () => onLike(store.player.track));
  const seek = h("input", {
    type: "range",
    min: 0,
    max: 1,
    step: 0.1,
    value: 0,
    "aria-label": ft("Позиция воспроизведения"),
    onchange: (event) => safe(() => api.seek(Number(event.target.value)))(),
  });
  const time = h("span", { class: "mini-time" });
  const element = h(
    "section",
    { class: "mini-player", "aria-label": ft("Мини-плеер") },
    h(
      "div",
      { class: "mini-info" },
      art,
      h("div", { class: "mini-titles" }, title, artist),
      button(ft("Вернуться в приложение"), "expand", () => setWindowMode("medium")),
    ),
    h(
      "div",
      { class: "mini-transport" },
      button(ft("Предыдущий"), "prev", () => api.previous()),
      toggle,
      button(ft("Следующий"), "next", () => api.next()),
      like,
    ),
    h("div", { class: "mini-progress" }, time, seek),
  );
  toggle.classList.add("mini-play");
  let previous;
  function render() {
    const p = store.player;
    const key = p.track ? `${p.track.owner_id}_${p.track.track_id}` : null;
    if (key !== previous) {
      previous = key;
      art.replaceChildren(artBox(p.track?.artwork_url));
    }
    title.textContent = p.track?.title || ft("Выберите музыку");
    artist.textContent = p.track?.artist || "Private Music";
    toggle.replaceChildren(
      icon(
        ["playing", "buffering", "loading"].includes(p.status)
          ? "pause"
          : "play",
        24,
      ),
    );
    like.replaceChildren(icon(isLiked(p.track) ? "heart-on" : "heart", 20));
    like.setAttribute("aria-pressed", String(isLiked(p.track)));
    like.disabled = !p.track;
    seek.max = String(p.duration || 1);
    if (document.activeElement !== seek) seek.value = String(p.position || 0);
    time.textContent = formatTime(p.position);
  }
  subscribe("player", render);
  subscribe("likes", render);
  render();
  return element;
}
