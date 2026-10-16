import { createTracksScreen } from "../tracks-screen.js";
import { t, tn } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { subscribe } from "../store.js";
import { downloads, refreshDownloads, formatSize } from "../downloads.js";

/** Everything on this computer, newest first. */
export function createDownloadsView({ play, onTrackAction }) {
  const screen = createTracksScreen({
    title: t("dl.title"),
    filterable: true,
    hooks: { emptyText: t("dl.empty"), art: "download", onPlay: play, onAction: onTrackAction },
  });

  // A summary card above the list: how much room the offline music takes.
  const sizeText = h("div", { class: "dl-size tnum" });
  const countText = h("div", { class: "dl-count" });
  const summary = h("div", { class: "dl-summary", hidden: true },
    h("span", { class: "dl-tile", "aria-hidden": "true" }, icon("download", 22)),
    h("div", { class: "dl-text" }, sizeText, countText));
  screen.element.insertBefore(summary, screen.element.querySelector(".status"));

  function render() {
    const { tracks, bytes } = downloads();
    screen.setTracks(tracks);
    summary.hidden = tracks.length === 0;
    sizeText.textContent = formatSize(bytes);
    countText.textContent = `${tn("library.count", tracks.length)} · ${t("dl.offline")}`;
    if (tracks.length) screen.element.querySelector(".meta").textContent += ` · ${t("dl.size", { size: formatSize(bytes) })}`;
  }

  subscribe("downloads", render);
  return {
    element: screen.element,
    show() { refreshDownloads(); },
  };
}
