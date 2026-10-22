import { h, formatTime } from "../dom.js";
import { artBox } from "../ui-parts.js";
import { icon } from "../icons.js";
import { t, tn, errorText } from "../i18n.js";
import { subscribe } from "../store.js";
import { listenLater, resumeLater, clearListenLater } from "../listen-later.js";

export function createListenLaterCard({ toast }) {
  let busy = false;
  const artwork = h("span", { class: "later-art" });
  const title = h("strong", { class: "later-title" });
  const detail = h("span", { class: "later-detail" });
  const play = h("button", { type: "button", class: "later-play", "aria-label": t("later.resume"), onclick: async () => {
    if (busy) return;
    busy = true; play.disabled = true;
    try { await resumeLater(); }
    catch (error) { toast(errorText(error), { error: true }); }
    finally { busy = false; play.disabled = false; }
  } }, artwork, h("span", { class: "later-text" }, title, detail), icon("play", 22));
  const remove = h("button", { type: "button", class: "later-remove", "aria-label": t("later.remove"), onclick: () => {
    try { clearListenLater(); } catch (error) { toast(errorText(error), { error: true }); }
  } }, icon("close", 18));
  const element = h("section", { class: "later-section", hidden: true },
    h("h2", { class: "shelf-title", text: t("later.title") }), h("div", { class: "later-card" }, play, remove),
    h("p", { class: "later-note", text: t("later.note") }));
  function render() {
    const entry = listenLater();
    element.hidden = !entry;
    if (!entry) return;
    const track = entry.tracks[entry.index];
    artwork.replaceChildren(artBox(track.artwork_url, { size: 24 }));
    title.textContent = track.title;
    detail.textContent = `${track.artist ?? ""} · ${formatTime(entry.position || 0)} · ${tn("library.count", entry.tracks.length)}`;
  }
  subscribe("personal.listenLater", render);
  subscribe("session", render);
  render();
  return { element };
}
