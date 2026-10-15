import { createTracksScreen } from "../tracks-screen.js";
import { t } from "../i18n.js";
import { subscribe } from "../store.js";
import { downloads, refreshDownloads, formatSize } from "../downloads.js";

/** Everything on this computer, newest first. */
export function createDownloadsView({ play, onTrackAction }) {
  const screen = createTracksScreen({
    title: t("dl.title"),
    filterable: true,
    hooks: { emptyText: t("dl.empty"), onPlay: play, onAction: onTrackAction },
  });

  function render() {
    const { tracks, bytes } = downloads();
    screen.setTracks(tracks);
    if (tracks.length) screen.element.querySelector(".meta").textContent += ` · ${t("dl.size", { size: formatSize(bytes) })}`;
  }

  subscribe("downloads", render);
  return {
    element: screen.element,
    show() { refreshDownloads(); },
  };
}
