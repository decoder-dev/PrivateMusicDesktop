import { api, loadAllPages } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { setLibrary, forgetLibrary, onLibraryChange } from "../likes.js";
import { trackKey } from "../api.js";

export function createLibraryView({ play, onTrackAction, toast }) {
  let loaded = false;
  let loading = false;
  let generation = 0;

  const screen = createTracksScreen({
    title: t("library.title"),
    filterable: true,
    hooks: {
      emptyText: t("library.empty"),
      onPlay: play,
      onAction: onTrackAction,
      pinLiked: false, // everything here is liked; the heart shows on hover only
      onRetry: () => { loaded = false; loading = false; load(); },
    },
  });
  let tracks = [];

  onLibraryChange(({ added, removed }) => {
    if (!loaded) return;
    if (added) tracks = [added, ...tracks.filter((t) => trackKey(t) !== trackKey(added))];
    if (removed) tracks = tracks.filter((t) => trackKey(t) !== trackKey(removed));
    screen.setTracks(tracks);
  });

  async function load() {
    if (loading) return;
    loading = true;
    const mine = ++generation;
    screen.setStatus(t("loading"));
    try {
      const all = await loadAllPages((offset, count) => api.library(offset, count), {
        isCancelled: () => mine !== generation,
        onProgress: (n, total, items) => {
          // Show the first page at once; later pages extend the same list.
          if (mine === generation) screen.setProgress(items.slice(), total);
        },
      });
      if (all === null) return;
      loaded = true;
      tracks = all;
      setLibrary(all);
      screen.clearStatus();
      screen.setTracks(all);
    } catch (error) {
      if (mine === generation) screen.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    } finally {
      if (mine === generation) loading = false;
    }
  }

  return {
    element: screen.element,
    show() {
      if (!loaded) load();
    },
    reload() {
      loaded = false;
      loading = false;
      load();
    },
    reset() {
      generation++;
      loaded = false;
      loading = false;
      tracks = [];
      forgetLibrary();
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}
