import { ft } from "./feature-i18n.js";
import { toggleWindowFullscreen, isWindowFullscreen,setWindowMode } from "./window-mode.js";
import { applyPalette } from "./themes.js";
import { h, clear } from "./dom.js";
import { icon } from "./icons.js";
import { t, errorText, addStrings } from "./i18n.js";
import { api, trackKey } from "./api.js";
import { listen } from "./bridge.js";
import { store, set, publish, subscribe, withScope, own } from "./store.js";
import { toast } from "./toast.js";
import { installTooltips } from "./tooltip.js";
import { createPlayerBar } from "./player-bar.js";
import { setDefaultTrackMenu, setDefaultBatchAction } from "./tracks-screen.js";
import { saveForLater } from "./listen-later.js";
import { createAudioUploadView } from "./views/audio-upload.js";
import { createPlaylistImportView } from "./views/playlist-import.js";
import { rememberQueue } from "./queue-history.js";
import { createSmartView } from "./views/smart.js";
import { createMiniPlayer } from "./mini-player.js";
import { installPlaylistSync } from "./playlist-sync.js";
import { installTastes,dislike } from "./tastes.js";
import { createNowPlaying } from "./now-playing.js";
import { toggleLike,isLiked } from "./likes.js";
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
import { handleBackendEvent as networkEvent, isNetworkError, setReloader, initNetwork, resetNetwork } from "./network.js";
import { initLayout, isPhone, isAndroid, onLayoutChange, setPlatform } from "./mobile/layout.js";
import "./mobile/style.js";
import "./mobile/np-theme.js";
import { onBack } from "./mobile/back.js";
import { createPhoneShell } from "./mobile/phone-shell.js";
import { createHomeView } from "./mobile/home.js";
import { createLibraryTab } from "./mobile/library-tab.js";
import { createProfileView } from "./mobile/profile.js";
import { lazyView } from "./mobile/lazy.js";
import { attachPullToRefresh } from "./mobile/pull-refresh.js";
import { closeMenu } from "./menu.js";



addStrings({ ru: { "export.audio": "Сохранить аудиофайл…", "export.saved": "Аудиофайл сохранён" }, en: { "export.audio": "Save audio file…", "export.saved": "Audio file saved" } });

initLayout();
const root = document.getElementById("app");
// Сохраняйте собственные меню треков приложения; подавлять команды браузера веб-просмотра (Сохранить
// как, Печать, Обновить, инструменты разработчика) везде.
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

// Размытие и прозрачность предназначены для окна, на которое смотрят: упрощенный вид в настройках или
// окно на заднем плане на некоторое время делает хром плоским.
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

// ---------------------------------------------------------------- действия

async function play(tracks, index, { shuffle = false, radio = null } = {}) {
  try {
    rememberQueue();
    if (shuffle) await api.setShuffle(true);
    // Очередь из микса пополняется из него во время воспроизведения.
    if (radio) await api.playRadio(radio, tracks, index);
    else await api.playTracks(tracks, index);
  } catch (error) {
    toast(errorText(error), { error: true });
  }
}

/** Сердца повсюду заканчиваются здесь. */
export async function like(track) {
  if (!track) return;
  try {
    const result = await toggleLike(track);
    const id=store.session.profile?.id;
    if (result) toast(t(result === "added" ? "like.added" : "like.removed"),result==="removed"?{action:{label:t("queue.undo"),run:async()=>{if(store.session.profile?.id!==id || isLiked(track))return;try{await toggleLike(track);}catch(error){toast(errorText(error),{error:true});}}}}:{});
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

/** "More like this": трек, затем радио VK из него. */
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

/** Запрашивает имя и создает список воспроизведения; разрешает это или ноль. */
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

/** ⋯ / контекстное меню трека. `extra` добавляет элементы, специфичные для экрана. */
async function openTrackMenu(track, at, extra = []) {
  // Открытие меню не должно ждать VK или недоступной сети.
  const playlists = ownPlaylists();
  ensurePlaylists().catch(() => {});
  const targets = [
    ...playlists.map((playlist) => ({ label: playlist.title, action: "add-to", run: () => addToPlaylist(playlist, track) })),
    ...(playlists.length ? ["-"] : []),
    { label: t("menu.newPlaylist"), action: "new-playlist", icon: "plus", run: async () => { const p = await newPlaylist(); if (p) addToPlaylist(p, track); } },
  ];
  // У нескольких связанных исполнителей есть подменю; один или просто имя, простой предмет.
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
    ...(isAndroid() ? [] : [{ label: t("export.audio"), action: "export", icon: "download", run: async () => { try { if (await api.exportTrack(track)) toast(t("export.saved")); await refreshDownloads(); } catch (error) { toast(errorText(error), { error: true }); } } }]),
    { label: t("radio"), action: "radio", icon: "radio", run: () => playSimilar(track) },
    { label:ft("Предпочтения Selena"),action:"taste",icon:"sparkles",items:[
      {label:ft("Меньше такого"),action:"taste-less",run:()=>dislike(track,"less").catch(error=>toast(errorText(error),{error:true}))},
      {label:ft("Не предлагать этот трек"),action:"taste-track",run:()=>dislike(track,"tracks").catch(error=>toast(errorText(error),{error:true}))},
      {label:ft("Не предлагать исполнителя"),action:"taste-artist",run:()=>dislike(track,"artists").catch(error=>toast(errorText(error),{error:true}))},
    ]},
    ...(goTo.length ? ["-", ...goTo] : []),
    "-",
    { label: t("menu.addTo"), action: "add-to-playlist", icon: "playlist", items: targets },
    ...(store.player.track && trackKey(track) === trackKey(store.player.track) ? [
      { label: t("later.save"), action: "listen-later", icon: "history", run: () => {
        try { saveForLater(); toast(t("later.saved")); } catch (error) { toast(errorText(error), { error: true }); }
      } },
    ] : []),
    ...(extra.length ? ["-", ...extra] : []),
  ], { label: track.title });
}

setDefaultTrackMenu((track, at) => openTrackMenu(track, at));
setDefaultBatchAction(async (action, tracks) => {
  const account=store.session.profile?.id;
  if (action === "queue") {
    for (const track of tracks) {
      if (!store.session.signed_in || store.session.profile?.id !== account) break;
      await api.playLast(track);
    }
    toast(t("playLast"));
  } else if (action === "playlist") {
    await ensurePlaylists();
    openMenu({x:Math.max(16,window.innerWidth/2-130),y:150},ownPlaylists().map(playlist=>({label:playlist.title,action:"batch-playlist",run:async()=>{
      let done=0;
      try {
        for (const track of tracks) {
          if (!store.session.signed_in || store.session.profile?.id !== account) break;
          await api.addToPlaylist(playlist,track); done++; changeCount(playlist,1);
        }
        toast(`${t("pl.added",{title:playlist.title})}: ${done}`);
      } catch(error) {toast(errorText(error),{error:true});}
    }})),{label:t("menu.addTo")});
  }
});

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

/** С экрана настроек: сохранено и перенесено туда, где они сейчас отображаются. */
function applySettings(patch) {
  persistSettings(patch);
  if ("theme" in patch || "accent" in patch || "flatChrome" in patch || "palette" in patch) applyAppearance();
  nowPlaying?.applySettings(patch);
}

// ------------------------------------------------------------------ оболочка

function buildShell() {
  const { value, dispose } = withScope(buildShellParts);
  return { ...value, dispose };
}

function buildShellParts() {
  installPlaylistSync();
  installTastes();
  // Страницы исполнителя и альбома располагаются поверх всего, что было на экране: «Назад» возвращается
  // туда с тем же экраном, каким он был (список воспроизведения остается открытым, результаты поиска
  // сохраняются). Боковая панель начинается заново с чистого стека.
  let stack = [];
  let current = { id: "library", payload: null };
  // `push` and `openPlayer` are for the phone's screens (Home, Library, Profile).
  const ctx = {
    play, onTrackAction, toast, openTrackMenu, newPlaylist, back, like,
    push: (id, payload) => (phone.active ? phone.push(id, payload) : navigate(id, payload)),
    openPlayer: () => nowPlaying?.show(),
  };
  const libraryView = createLibraryView(ctx);
  ctx.reloadLibrary = () => libraryView.reload?.();
  const views = {
    library: libraryView,
    home: lazyView(() => createHomeView(ctx)),
    libraryHub: lazyView(() => createLibraryTab({ libraryView, push: ctx.push, newPlaylist, toast })),
    audioUpload: lazyView(() => createAudioUploadView(ctx)),
    playlistImport: lazyView(() => createPlaylistImportView(ctx)),
    profile: lazyView(() => createProfileView({ push: ctx.push, onSignOut: signOut })),
    forYou: createForYouView(ctx),
    downloads: createDownloadsView(ctx),
    playlists: createPlaylistsView(ctx),
    smart: createSmartView(ctx),
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
    ["smart", "nav.smart", "sparkles"],
    ["friends", "nav.friends", "friends"],
    ["search", "nav.search", "search"],
    ["queue", "nav.queue", "queue"],
    ["history", "nav.history", "history"],
    ["stats", "nav.stats", "chart"],
    ["downloads", "nav.downloads", "download"],
    ["settings", "nav.settings", "settings"],
  ];

  // Группы: библиотека, потом что играет, потом настройки.
  const groupEnds = new Set(["search", "downloads"]);
  const nav = h("nav", { class: "nav", "aria-label": t("app.name") },
    items.flatMap(([id, label, glyph]) => [
      // Метка также является именем и подсказкой: на узкой боковой панели отображаются только значки.
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
  // «Моя музыка» переезжает во вкладку «Библиотека» телефона; в остальных макетах это страница основной колонки.
  libraryView.embedded = () => isPhone();
  const main = h("main", { class: "main" }, ...Object.values(views).map((v) => v.element), h("div", { class:"player-dock" },bar.element));
  const phone = createPhoneShell({ main, views, display: (id, direction) => display(id, direction), activeId: () => store.view });
  main.prepend(phone.navbar);

  const element = h("div", { class: "shell" },
    h("aside", { class: "sidebar" },
      h("div", { class: "brand" }, h("span", { class: "brand-mark", "aria-hidden": "true" }, h("img", { src: "assets/app-icon.png", alt: "", width: "32", height: "32" })), h("span", { class: "brand-name", text: t("app.name") })),
      nav, h("div", { class: "spacer" }), account),
    main,
    phone.dock,
    nowPlaying.element,createMiniPlayer(like));

  function display(id, direction = "") {
    // Установите перед отображением вида: выдвинутые страницы сдвигаются внутрь, остальные оседают.
    if (direction) main.dataset.dir = direction;
    else delete main.dataset.dir;
    set("view", id);
    main.dataset.view = id;
    // Вкладка «Библиотека» телефона держит экран «Моя музыка» внутри себя: отдельной страницей он там не бывает.
    for (const [key, view] of Object.entries(views)) {
      if (view.embedded?.()) continue;
      view.element.hidden = key !== id;
    }
    // Страница, открытая с экрана, сохраняет свое место на боковой панели.
    const section = stack[0]?.id ?? id;
    for (const button of nav.querySelectorAll("button")) {
      if (button.dataset.view === section) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    views[id].show?.();
  }

  /**
   * На боковой панели: экран начинается заново с первой страницы, как если бы вы снова коснулись
   * вкладки.
   */
  function show(id) {
    if (phone.active) { phone.showView(id); return; }
    stack = [];
    current = { id, payload: null };
    display(id);
    views[id].root?.();
  }

  function navigate(id, payload) {
    nowPlaying?.close();
    if (phone.active) { phone.push(id, payload); return; }
    stack.push(current);
    current = { id, payload };
    display(id, "push");
    views[id].open(payload);
  }

  function back() {
    if (phone.active) { phone.pop(); return; }
    const previous = stack.pop() ?? { id: "library", payload: null };
    current = previous;
    display(previous.id, "back");
    if (previous.payload) views[previous.id].open(previous.payload);
  }

  setRouter({ openArtist: (artist) => navigate("artist", artist), openAlbum: (album) => navigate("album", album) });

  // ---- the phone layout: bottom tabs on the same views
  /** The wide layout has no Home, Library hub or Profile: the nearest sidebar screen stands in. */
  const WIDE_FOR = { home: "forYou", libraryHub: "library", profile: "settings" };
  function adapt(layout) {
    if (layout === "phone") { views.libraryHub.ensure(); attachPull(); }
    views.libraryHub.embed(layout === "phone", main);
    if (layout === "phone") {
      phone.enter(store.view);
      views.library.show?.(); // hearts need the library, whichever tab is open
    } else {
      phone.leave();
      show(WIDE_FOR[store.view] ?? store.view);
    }
  }
  own(onLayoutChange(adapt));
  // Rows of data that a pull down at the top of a screen reloads.
  let pullAttached = false;
  function attachPull() {
    if (pullAttached) return;
    pullAttached = true;
    for (const id of ["home", "libraryHub", "history", "forYou", "friends", "downloads"]) {
      attachPullToRefresh(views[id].element, () => (views[id].reload ?? views[id].show)?.call(views[id]));
    }
  }
  // Content clears the mini player only while there is a track for it.
  subscribe("player", (player) => { document.documentElement.dataset.mini = player.track ? "on" : "off"; });
  own(() => { delete document.documentElement.dataset.mini; });

  return {
    element, views, bar, show, phone,
    /** The first screen after signing in: Home on a phone, "My music" beside the sidebar. */
    start() {
      if (isPhone()) { views.libraryHub.ensure(); attachPull(); }
      views.libraryHub.embed(isPhone(), main);
      if (isPhone()) {
        phone.start();
        views.library.show?.();
      } else {
        show("library");
      }
    },
    setProfile(profile) {
      const full = profile ? `${profile.first_name} ${profile.last_name}`.trim() : "";
      name.textContent = full;
      avatar.textContent = [...full.split(/\s+/).filter(Boolean).slice(0, 2).map((word) => [...word][0].toUpperCase())].join("");
      views.settings.setProfile(profile);
      views.profile.setProfile(profile);
    },
  };
}

// ---------------------------------------------------------- Маршрутизация экрана

function showLogin() {
  if(document.documentElement.dataset.windowMode==="mini")setWindowMode("medium").catch(()=>{});
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
  resetNetwork();
  login = null;
  shell?.dispose();
  clear(root);
  shell = buildShell();
  setReloader(() => shell?.views[store.view]?.show?.());
  root.append(shell.element);
  shell.start();
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
 * Группа статистики прослушивает по часам собственных часов пользователя; только окно знает свой
 * часовой пояс, поэтому оно сообщает об этом движку (и снова, когда часы могли измениться, e.g. летнее
 * время или путешествие).
 */
function syncTimeZone() {
  api.setUtcOffset(-new Date().getTimezoneOffset()).catch(() => {});
}
window.addEventListener("focus", () => { if (shell) syncTimeZone(); });

/** Отправляет сохраненные настройки в движок и возвращает последнюю очередь. */
async function applyStartupState() {
  const activeShell = shell;
  try {
    // Окно, лоток и устройство вывода — дело рабочего стола; у телефона их нет.
    if (!isAndroid()) api.setCloseToTray(settings.closeToTray).catch(() => {});
    api.setTransitions(settings.prefetch,settings.gapless,settings.crossfade).catch(()=>{});
    api.setNormalization(settings.normalization).catch(()=>{});
    syncTimeZone();
    if (settings.outputDevice && !isAndroid()) await api.setOutputDevice(settings.outputDevice).catch(() => {});
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
  resetNetwork();
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

// ------------------------------------------------------------ Внутренние события

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
      // В случае сбоя соединения вместо всплывающего уведомления отображается карта VPN (network.js).
      if (!isNetworkError(event) && !["rate","service"].includes(event.kind)) toast(errorText(event), { error: true });
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

/** Помнит, где мы находимся, но только каждые несколько секунд. */
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

// ------------------------------------------------------------------ ярлыки

function typing(event) {
  const el = event.target;
  return el instanceof HTMLInputElement && el.type !== "range" || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement || el?.isContentEditable;
}

document.addEventListener("keydown", (event) => {
  // Горячие клавиши — для окна на компьютере; у телефона клавиатуры нет (физическая ничего не меняет).
  if (!shell || isAndroid() || typing(event) || event.altKey) return;
  // Сфокусированный ползунок (объем, положение) сам использует клавиши со стрелками.
  if (event.target instanceof HTMLInputElement && event.target.type === "range" && event.key.startsWith("Arrow")) return;
  const mod = event.ctrlKey || event.metaKey;
  if (nowPlaying?.isOpen() && (event.key === "/" || (mod && (event.key === "f" || event.key === "F")))) return;
  switch (event.key) {
    case " ":
      if (event.repeat) { event.preventDefault(); return; }
      if (event.target instanceof HTMLButtonElement) return; // кнопка с фокусом управляет самим пространством
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
      if (event.repeat) { event.preventDefault(); return; }
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

// ------------------------------------------------------- кнопка «Назад» Android
// Слои закрываются сверху вниз (панели и диалоги — в mobile/back.js и mobile/sheet.js): полноэкранный
// плеер, затем стек страниц. Если нажатие не обработано, приложение уходит в фон.
onBack(20, () => {
  if (!nowPlaying?.isOpen()) return false;
  nowPlaying.close();
  return true;
});
onBack(25, () => {
  const tuning = document.querySelector("details.mix-tuning[open]");
  if (!tuning) return false;
  tuning.open = false;
  return true;
});
onBack(30, () => !!shell?.phone.active && shell.phone.pop());

// -------------------------------------------------------------------- Начни

async function start() {
  // The backend says what it runs on (an Android build reports "android").
  api.appInfo().then((info) => setPlatform(info.platform)).catch(() => {});
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

// Ярлыки окон также работают за пределами текущего представления и внутри элементов управления формой.
document.addEventListener("keydown",event=>{
 if(isAndroid() || (event.key!=="F11" && !(event.key==="Escape" && isWindowFullscreen())))return;
 event.preventDefault();event.stopImmediatePropagation();
 toggleWindowFullscreen(event.key==="Escape"?false:undefined).catch(error=>toast(errorText(error),{error:true}));
},true);
