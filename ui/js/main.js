import { toggleWindowFullscreen, isWindowFullscreen } from "./window-mode.js";
import { applyPalette } from "./themes.js";
import { h, clear } from "./dom.js";
import { icon } from "./icons.js";
import { t, errorText, addStrings } from "./i18n.js";
import { api, trackKey } from "./api.js";
import { listen } from "./bridge.js";
import { store, set, publish, subscribe, withScope } from "./store.js";
import { toast } from "./toast.js";
import { installTooltips } from "./tooltip.js";
import { createPlayerBar } from "./player-bar.js";
import { setDefaultTrackMenu } from "./tracks-screen.js";
import { createNowPlaying } from "./now-playing.js";
import { toggleLike } from "./likes.js";
import { createLibraryView } from "./views/library.js";
import { createPlaylistsView } from "./views/playlists.js";
import { createSearchView } from "./views/search.js";
import { createQueueView } from "./views/queue.js";
import { createLoginView } from "./views/login.js";
import { createSettingsView } from "./views/settings.js";
import { createForYouView } from "./views/for-you.js";
import { createDownloadsView } from "./views/downloads.js";
import { createArtistView } from "./views/artist.js";
import { createAlbumView } from "./views/album.js";
import { createFriendsView } from "./views/friends.js";
import { createHistoryView } from "./views/history.js";
import { createStatsView } from "./views/stats.js";
import { setRouter, hasArtist, hasAlbum, linkedArtists, openArtist, openArtistOf, openAlbumOf } from "./navigation.js";
import { forgetAlbums } from "./my-albums.js";
import { toggleDownload, refreshDownloads } from "./downloads.js";
import { openMenu } from "./menu.js";
import { formDialog } from "./dialog.js";
import { ensurePlaylists, ownPlaylists, addPlaylist, changeCount, forgetPlaylists } from "./my-playlists.js";
import { loadSettings, saveSettings, saveQueue, loadQueue, clearQueue } from "./persist.js";
import { handleBackendEvent as networkEvent, isNetworkError, setReloader, initNetwork } from "./network.js";


addStrings({ ru: { "export.audio": "Сохранить аудиофайл…", "export.saved": "Аудиофайл сохранён" }, en: { "export.audio": "Save audio file…", "export.saved": "Audio file saved" } });

const root = document.getElementById("app");
// Keep the application's own track menus; suppress the webview's browser
// commands (Save as, Print, Reload, developer tools) everywhere else.
document.addEventListener("contextmenu", (event) => event.preventDefault(), { capture: true });
document.addEventListener("keydown", (event) => {
  const key = event.key.toLowerCase();
  if (["F5", "F12"].includes(event.key) || ((event.ctrlKey || event.metaKey) && (["r", "p", "s", "u"].includes(key) || (event.shiftKey && ["i", "j", "c"].includes(key))))) event.preventDefault();
}, { capture: true });
const settings = loadSettings();
function applyAppearance() {
  if (settings.theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = settings.theme;
  document.documentElement.dataset.accent = settings.accent;
  applyPalette(settings);
  applyChrome();
}

// Blur and translucency are for a window that is being looked at: the simplified
// look in Settings, or a window in the background for a while, makes the chrome flat.
let backgrounded = false;
let backgroundTimer = 0;
function applyChrome() {
  const flat = settings.flatChrome || backgrounded;
  if (flat) document.documentElement.dataset.chrome = "flat";
  else delete document.documentElement.dataset.chrome;
}
function setBackgrounded(value) {
  clearTimeout(backgroundTimer);
  if (!value) { backgrounded = false; applyChrome(); return; }
  backgroundTimer = setTimeout(() => { backgrounded = true; applyChrome(); }, document.hidden ? 0 : 5000);
}
window.addEventListener("blur", () => setBackgrounded(true));
window.addEventListener("focus", () => setBackgrounded(false));
document.addEventListener("visibilitychange", () => setBackgrounded(document.hidden));
applyAppearance();
let shell = null;
let nowPlaying = null;
let login = null;
let unlisten = [];
let lastQueueSave = 0;

// ---------------------------------------------------------------- actions

async function play(tracks, index, { shuffle = false, radio = null } = {}) {
  try {
    if (shuffle) await api.setShuffle(true);
    // A queue from a mix keeps refilling from it while it plays.
    if (radio) await api.playRadio(radio, tracks, index);
    else await api.playTracks(tracks, index);
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

async function download(track) {
  try {
    const result = await toggleDownload(track);
    if (result === "added") toast(t("dl.done", { title: track.title }));
    if (result === "removed") toast(t("dl.removed"));
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

/** "More like this": the track, then VK's radio from it. */
async function playSimilar(track) {
  toast(t("radio.loading"));
  try {
    const similar = await api.similar(track);
    if (!similar.length) return toast(t("radio.none"));
    await api.playRadio({ kind: "similar", seed: track }, [track, ...similar], 0);
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

/** Asks for a name and creates a playlist; resolves to it or null. */
export async function newPlaylist() {
  const values = await formDialog({
    title: t("pl.new"),
    confirm: t("pl.create"),
    fields: [
      { name: "title", label: t("pl.name"), required: true, max: 128 },
      { name: "description", label: t("pl.description"), multiline: true, max: 2000 },
    ],
  });
  if (!values) return null;
  try {
    const playlist = await api.createPlaylist(values.title, values.description);
    addPlaylist(playlist);
    toast(t("pl.created", { title: playlist.title }));
    return playlist;
  } catch (error) {
    toast(errorText(error), { error: true });
    return null;
  }
}

async function addToPlaylist(playlist, track) {
  try {
    await api.addToPlaylist(playlist, track);
    changeCount(playlist, 1);
    toast(t("pl.added", { title: playlist.title }));
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

/** The ⋯ / right-click menu of a track. `extra` adds screen-specific items. */
async function openTrackMenu(track, at, extra = []) {
  // Opening a menu must not wait for VK or for an unavailable network.
  const playlists = ownPlaylists();
  ensurePlaylists().catch(() => {});
  const targets = [
    ...playlists.map((playlist) => ({ label: playlist.title, action: "add-to", run: () => addToPlaylist(playlist, track) })),
    ...(playlists.length ? ["-"] : []),
    { label: t("menu.newPlaylist"), action: "new-playlist", icon: "plus", run: async () => { const p = await newPlaylist(); if (p) addToPlaylist(p, track); } },
  ];
  // Several linked artists get a sub-menu; one, or just a name, a plain item.
  const artists = linkedArtists(track);
  const goTo = [
    artists.length > 1
      ? { label: t("menu.goArtist"), action: "go-artist", icon: "artist", items: artists.map((artist) => ({ label: artist.name, action: "go-artist-item", run: () => openArtist(artist) })) }
      : hasArtist(track) ? { label: t("menu.goArtist"), action: "go-artist", icon: "artist", run: () => openArtistOf(track) } : null,
    hasAlbum(track) ? { label: t("menu.goAlbum"), action: "go-album", icon: "album", run: () => openAlbumOf(track) } : null,
  ].filter(Boolean);
  openMenu(at, [
    { label: t("playNext"), action: "play-next", icon: "play-next", run: () => onTrackAction("play-next", track) },
    { label: t("playLast"), action: "play-last", icon: "queue-add", run: () => onTrackAction("play-last", track) },
    { label: t("export.audio"), action: "export", icon: "download", run: async () => { try { if (await api.exportTrack(track)) toast(t("export.saved")); await refreshDownloads(); } catch (error) { toast(errorText(error), { error: true }); } } },
    { label: t("radio"), action: "radio", icon: "radio", run: () => playSimilar(track) },
    ...(goTo.length ? ["-", ...goTo] : []),
    "-",
    { label: t("menu.addTo"), action: "add-to-playlist", icon: "playlist", items: targets },
    ...(extra.length ? ["-", ...extra] : []),
  ], { label: track.title });
}

setDefaultTrackMenu((track, at) => openTrackMenu(track, at));

async function onTrackAction(action, track) {
  if (action === "like") return like(track);
  if (action === "radio") return playSimilar(track);
  if (action === "download") return download(track);
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
  if ("theme" in patch || "accent" in patch || "flatChrome" in patch || "palette" in patch) applyAppearance();
  nowPlaying?.applySettings(patch);
}

// ------------------------------------------------------------------ shell

function buildShell() {
  const { value, dispose } = withScope(buildShellParts);
  return { ...value, dispose };
}

function buildShellParts() {
  // Artist and album pages stack on top of whatever was on screen: Back
  // returns there, with that screen as it was (a playlist stays open, a
  // search keeps its results). The sidebar starts over from a clean stack.
  let stack = [];
  let current = { id: "library", payload: null };
  const ctx = { play, onTrackAction, toast, openTrackMenu, newPlaylist, back };
  const views = {
    library: createLibraryView(ctx),
    forYou: createForYouView(ctx),
    downloads: createDownloadsView(ctx),
    playlists: createPlaylistsView(ctx),
    friends: createFriendsView(ctx),
    history: createHistoryView(ctx),
    stats: createStatsView(ctx),
    search: createSearchView(ctx),
    queue: createQueueView(),
    settings: createSettingsView({ settings, onSettingsChange: applySettings, onSignOut: signOut, toast }),
    artist: createArtistView(ctx),
    album: createAlbumView(ctx),
  };
  const items = [
    ["library", "nav.library", "library"],
    ["forYou", "nav.forYou", "home"],
    ["playlists", "nav.playlists", "playlist"],
    ["friends", "nav.friends", "friends"],
    ["search", "nav.search", "search"],
    ["queue", "nav.queue", "queue"],
    ["history", "nav.history", "history"],
    ["stats", "nav.stats", "chart"],
    ["downloads", "nav.downloads", "download"],
    ["settings", "nav.settings", "settings"],
  ];

  // Groups: the library, then what is playing, then the settings.
  const groupEnds = new Set(["search", "downloads"]);
  const nav = h("nav", { class: "nav", "aria-label": t("app.name") },
    items.flatMap(([id, label, glyph]) => [
      // The label is also the name and tooltip: the narrow sidebar shows icons only.
      h("button", { type: "button", "data-view": id, title: t(label), "aria-label": t(label), onclick: () => show(id) },
        h("span", { class: "glyph" }, icon(glyph, 18)),
        h("span", { class: "label", text: t(label) })),
      groupEnds.has(id) ? h("div", { class: "nav-sep", "aria-hidden": "true" }) : null,
    ]).filter(Boolean));
  const avatar = h("span", { class: "avatar-lite", "aria-hidden": "true" });
  const name = h("div", { class: "name" });
  const account = h("div", { class: "account" }, avatar,
    h("div", { class: "who" }, name,
      h("button", { class: "link", type: "button", title: t("account.signOut"), "aria-label": t("account.signOut"), onclick: signOut },
        h("span", { class: "ico" }, icon("logout", 16)), h("span", { class: "txt", text: t("account.signOut") }))));
  nowPlaying = createNowPlaying({ settings, onSettingsChange: persistSettings, onLike: like, onMenu: (track, at) => openTrackMenu(track, at) });
  const bar = createPlayerBar({ onSettingsChange: persistSettings, onExpand: () => nowPlaying.show(), onLike: like });
  const main = h("main", { class: "main" }, ...Object.values(views).map((v) => v.element), h("div", { class:"player-dock" },bar.element));

  const element = h("div", { class: "shell" },
    h("aside", { class: "sidebar" },
      h("div", { class: "brand" }, h("span", { class: "brand-mark", "aria-hidden": "true" }, h("img", { src: "assets/app-icon.png", alt: "", width: "32", height: "32" })), h("span", { class: "brand-name", text: t("app.name") })),
      nav, h("div", { class: "spacer" }), account),
    main,
    nowPlaying.element);

  function display(id, direction = "") {
    // Set before the view is shown: pages that are pushed slide in, the rest settle.
    if (direction) main.dataset.dir = direction;
    else delete main.dataset.dir;
    set("view", id);
    for (const [key, view] of Object.entries(views)) view.element.hidden = key !== id;
    // A page opened from a screen keeps that screen's place in the sidebar.
    const section = stack[0]?.id ?? id;
    for (const button of nav.querySelectorAll("button")) {
      if (button.dataset.view === section) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    views[id].show?.();
  }

  /** From the sidebar: the screen starts over at its first page, like tapping a tab again. */
  function show(id) {
    stack = [];
    current = { id, payload: null };
    display(id);
    views[id].root?.();
  }

  function navigate(id, payload) {
    nowPlaying?.close();
    stack.push(current);
    current = { id, payload };
    display(id, "push");
    views[id].open(payload);
  }

  function back() {
    const previous = stack.pop() ?? { id: "library", payload: null };
    current = previous;
    display(previous.id, "back");
    if (previous.payload) views[previous.id].open(previous.payload);
  }

  setRouter({ openArtist: (artist) => navigate("artist", artist), openAlbum: (album) => navigate("album", album) });

  return {
    element, views, bar, show,
    setProfile(profile) {
      const full = profile ? `${profile.first_name} ${profile.last_name}`.trim() : "";
      name.textContent = full;
      avatar.textContent = [...full.split(/\s+/).filter(Boolean).slice(0, 2).map((word) => [...word][0].toUpperCase())].join("");
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
  setReloader(() => shell?.views[store.view]?.show?.());
  root.append(shell.element);
  shell.show("library");
  const activeShell = shell;
  api.profile().then((profile) => {
    if (shell !== activeShell || !store.session.signed_in) return;
    store.session = { ...store.session, profile };
    publish("session");
    shell?.setProfile(profile);
  }).catch(() => {});
  refreshDownloads();
  ensurePlaylists().catch(() => {});
  await applyStartupState();
}

/**
 * The statistics group listening by the hour of the user's own clock; only
 * the window knows its time zone, so it tells the engine (and again when the
 * clock may have changed, e.g. daylight saving time or travel).
 */
function syncTimeZone() {
  api.setUtcOffset(-new Date().getTimezoneOffset()).catch(() => {});
}
window.addEventListener("focus", () => { if (shell) syncTimeZone(); });

/** Pushes saved settings to the engine, and brings back the last queue. */
async function applyStartupState() {
  const activeShell = shell;
  try {
    api.setCloseToTray(settings.closeToTray).catch(() => {});
    syncTimeZone();
    if (settings.outputDevice) await api.setOutputDevice(settings.outputDevice).catch(() => {});
    if (settings.eq.on) api.setEqualizer(settings.eq.gains).catch(() => {});
    await api.setVolume(settings.volume);
    await api.setShuffle(settings.shuffle);
    await api.setRepeat(settings.repeat);
    if (!store.session.signed_in) return;
    const state = await api.playerState();
    if (shell !== activeShell || !store.session.signed_in) return;
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
  forgetPlaylists();
  forgetAlbums();
  set("queue", { tracks: [], current: null });
  showLogin();
}

async function signOut() {
  try { await api.logout(); } catch (error) { toast(errorText(error), { error: true }); return; }
  const session = await api.sessionState();
  set("session", session);
  signedOut();
}

// ------------------------------------------------------------ backend events

function onBackendEvent(event) {
  networkEvent(event);
  switch (event.type) {
    case "sleep_timer": set("sleep", event); break;
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
      // A connection failure gets the VPN card (network.js) instead of a toast.
      if (!isNetworkError(event)) toast(errorText(event), { error: true });
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
  const sameTrack = tracks[current] && store.player.track && trackKey(tracks[current]) === trackKey(store.player.track);
  saveQueue(tracks, current, sameTrack ? store.player.position : 0);
});

// ----------------------------------------------------------------- shortcuts

function typing(event) {
  const el = event.target;
  return el instanceof HTMLInputElement && el.type !== "range" || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement || el?.isContentEditable;
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

installTooltips();
initNetwork();
start();
window.addEventListener("beforeunload", () => unlisten.forEach((fn) => fn()));

// Window shortcuts also work outside the now-playing view and inside form controls.
document.addEventListener("keydown",event=>{
 if(event.key!=="F11" && !(event.key==="Escape" && isWindowFullscreen()))return;
 event.preventDefault();event.stopImmediatePropagation();
 toggleWindowFullscreen(event.key==="Escape"?false:undefined).catch(error=>toast(errorText(error),{error:true}));
},true);
