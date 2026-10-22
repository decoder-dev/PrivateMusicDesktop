// A short, plain list of track rows (Home's recommendations): the same rows and
// gestures as the long virtualised lists, without the window, for a handful of tracks.

import { h } from "../dom.js";
import { trackRow } from "../track-row.js";
import { store, subscribe } from "../store.js";
import { trackKey } from "../api.js";
import { isLiked, isBusy } from "../likes.js";
import { isDownloaded, isDownloading } from "../downloads.js";
import { openArtistOf } from "../navigation.js";
import { attachRowGestures } from "./row-gestures.js";

export function createCompactTrackList({ play, onAction, onMenu }) {
  let tracks = [];
  const element = h("div", { class: "compact-list", role: "listbox" });

  function render() {
    const playing = store.player.status === "playing" || store.player.status === "buffering" || store.player.status === "loading";
    element.replaceChildren(...tracks.map((track, index) => {
      const row = trackRow(track, {
        selected: !!store.player.track && trackKey(store.player.track) === trackKey(track),
        paused: !playing,
        liked: isLiked(track), busy: isBusy(track), pinLiked: true,
        downloaded: isDownloaded(track), downloading: isDownloading(track),
      });
      row.dataset.index = String(index);
      return row;
    }));
  }

  element.addEventListener("click", (event) => {
    const row = event.target.closest(".row");
    if (!row) return;
    const index = Number(row.dataset.index);
    const track = tracks[index];
    const button = event.target.closest("[data-action]");
    if (!track) return;
    if (button?.dataset.action === "menu") {
      const box = button.getBoundingClientRect();
      onMenu(track, { x: box.right, y: box.bottom });
    } else if (button?.dataset.action === "artist") openArtistOf(track);
    else if (button) onAction(button.dataset.action, track, index);
    else play(tracks, index, { shuffle: false });
  });
  for (const type of ["pm-row-longpress", "pm-row-swipe"]) {
    element.addEventListener(type, (event) => {
      const row = event.target.closest(".row");
      const track = tracks[Number(row?.dataset.index)];
      if (!track) return;
      if (type === "pm-row-longpress") onMenu(track, { x: event.detail.x, y: event.detail.y });
      else onAction(event.detail.dir === "lead" ? (event.detail.far ? "play-last" : "play-next") : "like", track, Number(row.dataset.index));
    });
  }
  attachRowGestures(element);

  let lastKey = null;
  subscribe("player", (player) => {
    const key = `${player.track ? trackKey(player.track) : ""}|${player.status}`;
    if (key !== lastKey) { lastKey = key; render(); }
  });
  subscribe("likes", render);
  subscribe("downloads", render);

  return {
    element,
    get tracks() { return tracks; },
    setTracks(next) { tracks = next; render(); },
  };
}
