import { api, loadAllPages } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { playlistCard } from "./playlists.js";

const fullName = (friend) => `${friend.first_name} ${friend.last_name}`.trim();

/**
 * VK refuses a friend's audio when they keep it private: that is a "vk"
 * error, and it gets a plain explanation instead of VK's wording.
 */
const friendError = (error) => (error?.kind === "vk" ? t("friends.restricted") : errorText(error));

function avatar(friend) {
  return friend.photo_url
    ? h("img", { class: "avatar", src: friend.photo_url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
    : h("span", { class: "avatar empty" }, icon("artist", 22));
}

/**
 * Friends' music: the friends, then one friend's tracks and playlists, then a
 * playlist's tracks. Each step has its own Back; playing works from every list.
 */
export function createFriendsView({ play, onTrackAction, toast }) {
  let friends = [];
  let loaded = false;
  let generation = 0;
  let friend = null;
  let tab = "tracks";
  let friendGeneration = 0;
  let playlistsFor = null; // the friend whose playlists are on screen
  let playlistGeneration = 0;

  // -------------------------------------------------------------- the friends
  const status = h("div", { class: "status", role: "status", hidden: true });
  const list = h("div", { class: "friend-list", role: "list" });
  const listScreen = h("section", { class: "view" },
    h("header", { class: "view-header" }, h("h1", { text: t("friends.title") })), status, list);

  function setStatus(text, { error = false } = {}) {
    status.textContent = text ?? "";
    status.hidden = !text;
    status.className = `status${error ? " error" : ""}`;
  }

  function renderFriends() {
    list.replaceChildren(...friends.map((person) =>
      h("button", { class: "friend", type: "button", role: "listitem", "data-friend": String(person.id), "aria-label": t("friends.open", { name: fullName(person) }), onclick: () => openFriend(person) },
        avatar(person),
        h("span", { class: "friend-name", text: fullName(person) }))));
  }

  async function load() {
    const mine = ++generation;
    setStatus(t("friends.loading"));
    try {
      const all = await loadAllPages((offset, count) => api.friends(offset, count), {
        pageSize: 200,
        isCancelled: () => mine !== generation,
        onProgress: (n, total, items) => {
          if (mine !== generation) return;
          friends = items.slice();
          renderFriends();
        },
      });
      if (all === null) return;
      loaded = true;
      friends = all;
      renderFriends();
      setStatus(all.length ? "" : t("friends.empty"));
    } catch (error) {
      if (mine !== generation) return;
      setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  // --------------------------------------------------------------- one friend
  const backToFriends = h("button", { class: "icon-btn", type: "button", title: t("friends.back"), "aria-label": t("friends.back"), onclick: () => showList() }, icon("back", 20));
  const friendPhoto = h("div", { class: "page-art round friend-photo" });
  const friendName = h("h1");
  const tabs = h("div", { class: "tabs", role: "tablist", "aria-label": t("friends.title") },
    ["tracks", "playlists"].map((id) => h("button", { type: "button", role: "tab", "data-tab": id, onclick: () => selectTab(id), text: t(`friends.${id}`) })));

  const tracks = createTracksScreen({
    title: t("friends.tracks"),
    heading: "h2",
    filterable: true,
    hooks: { emptyText: t("friends.noTracks"), onPlay: play, onAction: onTrackAction },
  });
  const playlistStatus = h("div", { class: "status", role: "status", hidden: true });
  const playlistGrid = h("div", { class: "grid friend-playlists" });
  const tracksPane = h("div", { class: "tab-pane", "data-pane": "tracks" }, tracks.element);
  const playlistsPane = h("div", { class: "tab-pane", "data-pane": "playlists", hidden: true }, playlistStatus, playlistGrid);
  const friendScreen = h("section", { class: "view", hidden: true },
    h("header", { class: "view-header page-head" }, backToFriends, friendPhoto, h("div", { class: "page-info" }, friendName)),
    h("div", { class: "tab-bar" }, tabs), tracksPane, playlistsPane);

  function selectTab(id) {
    tab = id;
    for (const button of tabs.children) button.setAttribute("aria-selected", String(button.dataset.tab === id));
    tracksPane.hidden = id !== "tracks";
    playlistsPane.hidden = id !== "playlists";
    if (id === "playlists" && playlistsFor !== friend) loadPlaylists();
  }

  async function openFriend(person) {
    friend = person;
    playlistsFor = null;
    friendName.textContent = fullName(person);
    friendPhoto.replaceChildren(person.photo_url
      ? h("img", { src: person.photo_url, alt: "", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" })
      : icon("artist", 28));
    listScreen.hidden = true;
    playlistScreen.hidden = true;
    friendScreen.hidden = false;
    playlistGrid.replaceChildren();
    setPlaylistStatus("");
    selectTab("tracks");
    const mine = ++friendGeneration;
    tracks.setTracks([]);
    tracks.setStatus(t("loading"));
    try {
      const all = await loadAllPages((offset, count) => api.friendTracks(person.id, offset, count), {
        isCancelled: () => mine !== friendGeneration,
        onProgress: (n, total, items) => {
          if (mine === friendGeneration) tracks.setProgress(items.slice(), total);
        },
      });
      if (all === null) return;
      tracks.clearStatus();
      tracks.setTracks(all);
    } catch (error) {
      if (mine !== friendGeneration) return;
      tracks.setStatus(friendError(error), { error: true });
    }
  }

  function setPlaylistStatus(text, { error = false } = {}) {
    playlistStatus.textContent = text ?? "";
    playlistStatus.hidden = !text;
    playlistStatus.className = `status${error ? " error" : ""}`;
  }

  async function loadPlaylists() {
    const person = friend;
    const mine = ++playlistGeneration;
    playlistGrid.replaceChildren();
    setPlaylistStatus(t("loading"));
    try {
      const all = await loadAllPages((offset, count) => api.friendPlaylists(person.id, offset, count), {
        pageSize: 100,
        isCancelled: () => mine !== playlistGeneration || person !== friend,
      });
      if (all === null) return;
      playlistsFor = person;
      playlistGrid.replaceChildren(...all.map((playlist) => playlistCard(playlist, openPlaylist)));
      setPlaylistStatus(all.length ? "" : t("friends.noPlaylists"));
    } catch (error) {
      if (mine !== playlistGeneration || person !== friend) return;
      setPlaylistStatus(friendError(error), { error: true });
    }
  }

  // ------------------------------------------------------------ one playlist
  let playlistToken = 0;
  const playlistTracks = createTracksScreen({
    title: "",
    hooks: {
      emptyText: t("library.empty"),
      onPlay: play,
      onAction: onTrackAction,
      back: h("button", { class: "icon-btn", type: "button", title: t("playlists.back"), "aria-label": t("playlists.back"), onclick: () => closePlaylist() }, icon("back", 20)),
    },
  });
  const playlistScreen = playlistTracks.element;
  playlistScreen.hidden = true;

  async function openPlaylist(playlist) {
    friendScreen.hidden = true;
    playlistScreen.hidden = false;
    playlistTracks.setTitle(playlist.title);
    playlistTracks.setTracks([]);
    playlistTracks.setStatus(t("loading"));
    const mine = ++playlistToken;
    try {
      const all = await loadAllPages((offset, count) => api.playlistTracks(playlist, offset, count), {
        isCancelled: () => mine !== playlistToken,
        onProgress: (n, total, items) => {
          if (mine === playlistToken) playlistTracks.setProgress(items.slice(), total);
        },
      });
      if (all === null) return;
      playlistTracks.clearStatus();
      playlistTracks.setTracks(all);
    } catch (error) {
      if (mine !== playlistToken) return;
      playlistTracks.setStatus(friendError(error), { error: true });
      toast(friendError(error), { error: true });
    }
  }

  function closePlaylist() {
    playlistToken++;
    playlistScreen.hidden = true;
    friendScreen.hidden = false;
  }

  function showList() {
    friendGeneration++;
    playlistGeneration++;
    playlistToken++;
    friend = null;
    friendScreen.hidden = true;
    playlistScreen.hidden = true;
    listScreen.hidden = false;
  }

  const element = h("div", { class: "view" }, listScreen, friendScreen, playlistScreen);
  return {
    element,
    show() {
      if (!loaded) load();
    },
    reset() {
      generation++;
      loaded = false;
      friends = [];
      list.replaceChildren();
      setStatus("");
      tracks.setTracks([]);
      playlistGrid.replaceChildren();
      showList();
    },
  };
}
