import { ft } from "./feature-i18n.js";
import { api, trackKey } from "./api.js";
import { readPersonal, writePersonal,accountId } from "./personal.js";
import { libraryTracks } from "./likes.js";
import { h } from "./dom.js";
import { subscribe } from "./store.js";
import { toast } from "./toast.js";
import { errorText } from "./i18n.js";
export function tastes() {
  const raw = readPersonal("taste", {});
  return {
    tracks: Array.isArray(raw.tracks) ? raw.tracks : [],
    artists: Array.isArray(raw.artists) ? raw.artists : [],
    less: Array.isArray(raw.less) ? raw.less : [],
    novelty: Number.isInteger(raw.novelty)
      ? Math.max(0, Math.min(100, raw.novelty))
      : 50,
  };
}
export async function applyTaste() {
  const id=accountId();if(!id)return;
  await api.setTaste({
    ...tastes(),
    familiar: libraryTracks().map(trackKey).slice(0, 20000),
  },id);
}
export async function changeTaste(patch) {
  const next = { ...tastes(), ...patch };
  writePersonal("taste", next);
  await applyTaste();
}
export async function dislike(track, kind) {
  const taste = tastes();
  const key =
    kind === "artists" ? track.artist.trim().toLowerCase() : trackKey(track);
  await changeTaste({
    [kind]: [...new Set([...taste[kind], key])].slice(
      -(kind === "artists" ? 1000 : 5000),
    ),
  });
  toast(
    kind === "less"
      ? ft("Selena будет ставить этот трек реже")
      : ft("Исключено из подборок Selena"),
  );
}
export function installTastes() {
  subscribe("likes", () => applyTaste().catch(() => {}));
  subscribe("session", () => applyTaste().catch(() => {}));
  applyTaste().catch(() => {});
}
export function createTastePanel() {
  const count = h("span", { class: "set-note" });
  const label = h("output");
  const range = h("input", {
    type: "range",
    min: 0,
    max: 100,
    step: 10,
    "aria-label": ft("Доля новой музыки"),
    oninput: () => {
      label.textContent = ft("{n}% новой музыки",{n:range.value});
    },
    onchange: () =>
      changeTaste({ novelty: Number(range.value) }).catch((error) =>
        toast(errorText(error), { error: true }),
      ),
  });
  const exclusions = h("div", { class: "taste-exclusions" });
  const reset = h("button", {
    class: "btn",
    type: "button",
    text: ft("Сбросить предпочтения"),
    onclick: () =>
      changeTaste({ tracks: [], artists: [], less: [], novelty: 50 }).catch(
        (error) => toast(errorText(error), { error: true }),
      ),
  });
  const element = h(
    "div",
    { class: "taste-panel" },
    h("strong", { text: ft("Вкусы Selena") }),
    h("p", {
      class: "set-note",
      text: ft("Баланс применяется к доступным рекомендациям VK. Если знакомых или новых треков недостаточно, очередь дополняется остальными. Исключения действуют только в подборках."),
    }),
    h("div", { class: "set-inline" }, range, label),
    count,
    exclusions,
    reset,
  );
  function render() {
    const taste = tastes();
    range.value = String(taste.novelty);
    label.textContent = ft("{n}% новой музыки",{n:taste.novelty});
    count.textContent = ft("Исключено треков: {tracks} · исполнителей: {artists} · реже: {less}",{tracks:taste.tracks.length,artists:taste.artists.length,less:taste.less.length});
    exclusions.replaceChildren(
      ...[
        ...taste.artists.map((value) => ["artists", value]),
        ...taste.tracks.map((value) => ["tracks", value]),
        ...taste.less.map((value) => ["less", value]),
      ]
        .slice(0, 50)
        .map(([kind, value]) =>
          h("button", {
            class: "btn",
            type: "button",
            text: `${kind === "less" ? ft("Реже: ") : ""}${value} ×`,
            title: ft("Вернуть в рекомендации"),
            onclick: () =>
              changeTaste({
                [kind]: tastes()[kind].filter((item) => item !== value),
              }).catch((error) => toast(errorText(error), { error: true })),
          }),
        ),
    );
  }
  subscribe("personal.taste", render);
  subscribe("session", render);
  render();
  return element;
}
