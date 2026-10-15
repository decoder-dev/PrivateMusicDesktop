import { api, loadAllPages } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";

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
    },
  });

  async function load() {
    if (loading) return;
    loading = true;
    const mine = ++generation;
    screen.setStatus(t("loading"));
    try {
      const tracks = await loadAllPages((offset, count) => api.library(offset, count), {
        isCancelled: () => mine !== generation,
        onProgress: (n, total, items) => {
          // Show the first page at once; later pages extend the same list.
          if (mine === generation) screen.setProgress(items.slice(), total);
        },
      });
      if (tracks === null) return;
      loaded = true;
      screen.clearStatus();
      screen.setTracks(tracks);
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
      screen.setTracks([]);
      screen.clearStatus();
    },
  };
}
