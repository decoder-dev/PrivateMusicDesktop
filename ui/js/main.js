import { h, clear } from "./dom.js";
import { icon } from "./icons.js";
import { t, errorText } from "./i18n.js";
import { api } from "./api.js";
import { listen } from "./bridge.js";
import { store, set, publish, subscribe, withScope } from "./store.js";
import { toast } from "./toast.js";
import { createPlayerBar } from "./player-bar.js";
import { createNowPlaying } from "./now-playing.js";
import { toggleLike } from "./likes.js";
import { createLibraryView } from "./views/library.js";
import { createPlaylistsView } from "./views/playlists.js";
import { createSearchView } from "./views/search.js";
import { createQueueView } from "./views/queue.js";
import { createLoginView } from "./views/login.js";
import { createSettingsView } from "./views/settings.js";
import { loadSettings, saveSettings, saveQueue, loadQueue, clearQueue } from "./persist.js";

const root = document.getElementById("app");
const settings = loadSettings();
let shell = null;
let nowPlaying = null;
let login = null;
let unlisten = [];
let lastQueueSave = 0;

// ---------------------------------------------------------------- actions

async function play(tracks, index, { shuffle = false } = {}) {
  try {
    if (shuffle) await api.setShuffle(true);
    await api.playTracks(tracks, index);
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

/** Hearts everywhere end here. */
export async function like(track) {
  if (!track) return;
  try {
    const result = await toggleLike(track);
    if (result) toast(t(result === "added" ? "like.added" : "like.removed"));
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

async function onTrackAction(action, track) {
  if (action === "like") return like(track);
  try {
    if (action === "play-next") {
      await api.playNext(track);
      toast(t("playNext"));
    } else if (action === "play-last") {
      await api.playLast(track);
      toast(t("playLast"));
    }
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

function persistSettings(patch) {
  Object.assign(settings, patch);
  saveSettings(settings);
}

/** From the settings screen: saved, and pushed to whatever shows them now. */
function applySettings(patch) {
  persistSettings(patch);
  nowPlaying?.applySettings(patch);
}

// ------------------------------------------------------------------ shell

function buildShell() {
  const { value, dispose } = withScope(buildShellParts);
  return { ...value, dispose };
}

function buildShellParts() {
  const ctx = { play, onTrackAction, toast };
  const views = {
    library: createLibraryView(ctx),
    playlists: createPlaylistsView(ctx),
    search: createSearchView(ctx),
    queue: createQueueView(),
    settings: createSettingsView({ settings, onSettingsChange: applySettings, onSignOut: signOut, toast }),
  };
  const items = [
    ["library", "nav.library", "note"],
    ["playlists", "nav.playlists", "playlist"],
    ["search", "nav.search", "search"],
    ["queue", "nav.queue", "queue"],
    ["settings", "nav.settings", "settings"],
  ];

  const nav = h("nav", { class: "nav", "aria-label": t("app.name") },
    items.map(([id, label, glyph]) =>
      h("button", { type: "button", "data-view": id, onclick: () => show(id) },
        h("span", { class: "glyph" }, icon(glyph, 18)),
        h("span", { class: "label", text: t(label) }))));
  const name = h("div", { class: "name" });
  const account = h("div", { class: "account" }, name,
    h("button", { class: "link", type: "button", onclick: signOut }, t("account.signOut")));
  const main = h("main", { class: "main" }, ...Object.values(views).map((v) => v.element));
  nowPlaying = createNowPlaying({ settings, onSettingsChange: persistSettings, onLike: like });
  const bar = createPlayerBar({ onSettingsChange: persistSettings, onExpand: () => nowPlaying.show(), onLike: like });

  const element = h("div", { class: "shell" },
    h("aside", { class: "sidebar" }, h("div", { class: "brand", text: t("app.name") }), nav, h("div", { class: "spacer" }), account),
    main,
    h("footer", { class: "player" }, bar.element),
    nowPlaying.element);

  function show(id) {
    set("view", id);
    for (const [key, view] of Object.entries(views)) view.element.hidden = key !== id;
    for (const button of nav.querySelectorAll("button")) {
      if (button.dataset.view === id) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    views[id].show?.();
  }

  return {
    element, views, bar, show,
    setProfile(profile) {
      name.textContent = profile ? `${profile.first_name} ${profile.last_name}`.trim() : "";
      views.settings.setProfile(profile);
    },
  };
}

// ----------------------------------------------------------- screen routing

function showLogin() {
  nowPlaying?.close();
  nowPlaying = null;
  shell?.dispose();
  shell = null;
  clear(root);
  login ??= createLoginView({ onError: (error) => toast(errorText(error), { error: true }) });
  login.reset();
  login.showPersistenceWarning(!store.session.persistent);
  root.append(login.element);
}

async function showApp() {
  login = null;
  shell?.dispose();
  clear(root);
  shell = buildShell();
  root.append(shell.element);
  shell.show("library");
  api.profile().then((profile) => shell?.setProfile(profile)).catch(() => {});
  await applyStartupState();
}

/** Pushes saved settings to the engine, and brings back the last queue. */
async function applyStartupState() {
  try {
    api.setCloseToTray(settings.closeToTray).catch(() => {});
    if (settings.outputDevice) await api.setOutputDevice(settings.outputDevice).catch(() => {});
    await api.setVolume(settings.volume);
    await api.setShuffle(settings.shuffle);
    await api.setRepeat(settings.repeat);
    const state = await api.playerState();
    set("player", state);
    if (state.queue_len === 0) {
      const saved = loadQueue();
      if (saved) await api.loadQueue(saved.tracks, saved.index, saved.position);
    }
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

function signedOut() {
  for (const view of Object.values(shell?.views ?? {})) view.reset?.();
  clearQueue();
  set("queue", { tracks: [], current: null });
  showLogin();
}

async function signOut() {
  await api.logout();
  const session = await api.sessionState();
  set("session", session);
  signedOut();
}

// ------------------------------------------------------------ backend events

function onBackendEvent(event) {
  switch (event.type) {
    case "state": {
      const previousVolume = store.player.volume;
      set("player", event);
      scheduleQueueSave(event);
      if (event.volume !== previousVolume) persistSettings({ volume: event.volume });
      if (event.shuffle !== settings.shuffle || event.repeat !== settings.repeat) {
        persistSettings({ shuffle: event.shuffle, repeat: event.repeat });
      }
      break;
    }
    case "queue":
      set("queue", { tracks: event.tracks, current: event.current });
      break;
    case "spectrum":
      nowPlaying?.feed(event.bands);
      break;
    case "error":
      toast(errorText(event), { error: true });
      break;
    case "session_expired":
      api.sessionState().then((session) => {
        set("session", session);
        signedOut();
        toast(t("error.unauthorized"), { error: true });
      });
      break;
  }
}

/** Remembers where we are, but only every few seconds. */
function scheduleQueueSave(state) {
  const now = Date.now();
  if (now - lastQueueSave < 5000 || !state.track) return;
  lastQueueSave = now;
  saveQueue(store.queue.tracks, state.queue_index, state.position);
}

subscribe("queue", ({ tracks, current }) => {
  lastQueueSave = Date.now();
  saveQueue(tracks, current, 0);
});

// ----------------------------------------------------------------- shortcuts

function typing(event) {
  const el = event.target;
  return el instanceof HTMLInputElement && el.type !== "range" || el instanceof HTMLTextAreaElement;
}

document.addEventListener("keydown", (event) => {
  if (!shell || typing(event) || event.altKey) return;
  // A focused slider (volume, position) uses the arrow keys itself.
  if (event.target instanceof HTMLInputElement && event.target.type === "range" && event.key.startsWith("Arrow")) return;
  const mod = event.ctrlKey || event.metaKey;
  if (nowPlaying?.isOpen() && (event.key === "/" || (mod && (event.key === "f" || event.key === "F")))) return;
  switch (event.key) {
    case " ":
      if (event.target instanceof HTMLButtonElement) return; // a focused button handles Space itself
      event.preventDefault();
      api.toggle();
      break;
    case "ArrowRight":
      event.preventDefault();
      if (mod) api.next();
      else api.seek(Math.min(store.player.duration, store.player.position + 5));
      break;
    case "ArrowLeft":
      event.preventDefault();
      if (mod) api.previous();
      else api.seek(Math.max(0, store.player.position - 5));
      break;
    case "ArrowUp":
      if (mod) { event.preventDefault(); shell.bar.volumeUp(); }
      break;
    case "ArrowDown":
      if (mod) { event.preventDefault(); shell.bar.volumeDown(); }
      break;
    case "m":
    case "M":
      if (!mod) shell.bar.toggleMute();
      break;
    case "/":
      event.preventDefault();
      shell.show("search");
      shell.views.search.focus();
      break;
    case "f":
    case "F":
      if (!mod) {
        event.preventDefault();
        nowPlaying?.show();
      } else {
        event.preventDefault();
        shell.show("library");
        document.querySelector(".view:not([hidden]) input[type='search']")?.focus();
      }
      break;
  }
});

// -------------------------------------------------------------------- start

async function start() {
  unlisten = await Promise.all([
    listen("pm-event", onBackendEvent),
    listen("pm-login", async (result) => {
      login?.loginResult(String(result));
      if (result === "success") {
        set("session", await api.sessionState());
        await showApp();
      }
    }),
  ]);
  try {
    const session = await api.sessionState();
    set("session", session);
    if (session.signed_in) await showApp();
    else showLogin();
  } catch (error) {
    showLogin();
    toast(errorText(error), { error: true });
  }
}

start();
window.addEventListener("beforeunload", () => unlisten.forEach((fn) => fn()));
