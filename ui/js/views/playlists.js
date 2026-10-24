import { createCoverEditor } from "../playlist-cover.js";
import { isAndroid } from "../mobile/layout.js";
import { accountId } from "../personal.js";
import { discoveryField, discoveryChoice } from "../playlist-discovery.js";
import { collectionShareLink } from "../share-links.js";
import { linkControls } from "../sharing.js";
import { ft } from "../feature-i18n.js";
import { playlistLink } from "../playlist-link.js";
import { api } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { errorText, t, tn, addStrings } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { subscribe } from "../store.js";
import { formDialog, confirmDialog } from "../dialog.js";
import { loadAllPages } from "../api.js";
import { allPlaylists, ensurePlaylists, isOwn, replacePlaylist, dropPlaylist, changeCount, same, forgetPlaylists, addPlaylist } from "../my-playlists.js";
import { ensureAlbums, followedAlbums } from "../my-albums.js";
import { albumCard } from "../album-card.js";
import { openAlbum } from "../navigation.js";
import { artBox, paintStatus, backButton, headerOnScroll } from "../ui-parts.js";
import { enter } from "../motion.js";
import { isSynced,setPlaylistSync,syncPlaylists } from "../playlist-sync.js";
import { removeSelected } from "../batch-remove.js";
const sameTrack=(a,b)=>a.owner_id===b.owner_id && a.track_id===b.track_id;

addStrings({ ru: { "pl.openLink": "Открыть по ссылке", "pl.link": "Ссылка на плейлист VK", "pl.linkInvalid": "Вставьте ссылку VK вида vk.com/music/playlist/-123_456 или vk.com/audio_playlist-123_456.", "pl.shared": "Плейлист по ссылке", "pl.follow": "Добавить в мои плейлисты", "pl.followed": "Плейлист добавлен" }, en: { "pl.openLink": "Open link", "pl.link": "VK playlist link", "pl.linkInvalid": "Paste a VK playlist link, such as vk.com/music/playlist/-123_456.", "pl.shared": "Shared playlist", "pl.follow": "Add to my playlists", "pl.followed": "Playlist added" } });

export function playlistCard(playlist, open) {
  const art = h("span", { class: "card-art art" }, artBox(playlist.artwork_url, { className: "fill", fallbackText: "" }), h("span", { class: "play-chip" }, icon("play", 12)));
  return h("button", { class: "card collection-card playlist-card", type: "button", onclick: () => open(playlist) },
    art,
    h("div", { class: "name", text: playlist.title }),
    h("div", { class: "count", text: tn("playlists.tracks", playlist.count) }));
}

export function createPlaylistsView({ play, onTrackAction, toast, openTrackMenu, newPlaylist, back }) {
  let loaded = false;
  // A playlist opened from a card on another screen (the phone's navigation stack): Back leaves the view.
  let pushed = false;
  let generation = 0;
  let detailGeneration = 0;
  let current = null; // открытый плейлист
  let tracks = [];
  let sharedAlbum = null;

  const status = h("div", { class: "status", role: "status", hidden: true });
  const grid = h("div", { class: "grid" });
  const header = h("header", { class: "view-header" }, h("div", { class: "vh-text" }, h("h1", { text: t("playlists.title") })),
    h("button", { class: "btn", type: "button", onclick: openLink, text: t("pl.openLink") }),
    h("button", { class: "btn primary", type: "button", onclick: () => newPlaylist() }, icon("plus", 14), t("pl.new")));
  // Следящие релизы ниже плейлистов.
  const albumsStatus = h("div", { class: "status", role: "status", hidden: true });
  const albumsGrid = h("div", { class: "grid followed-albums" });
  const shelves = h("div", { class: "page-scroll" }, status, grid,
    h("h2", { class: "shelf-title", text: t("albums.title") }), albumsStatus, albumsGrid);
  const listScreen = h("section", { class: "view" }, header, shelves);
  headerOnScroll(header, shelves);

  const coverEditor=createCoverEditor({getPlaylist:()=>current,isSame:(a,b)=>!!a && !!b && same(a,b),toast,
    onBusy:busy=>{coverButton.disabled=busy;},
    onSaved:async playlist=>{const account=accountId();forgetPlaylists();await ensurePlaylists();if(account===accountId() && current && same(current,playlist)){const fresh=allPlaylists().find(item=>same(item,playlist));if(fresh){current=fresh;detail.setCover(fresh.artwork_url);renderOwner();}}}});
  const coverButton=h("button",{class:"icon-btn circle-btn playlist-cover",type:"button",title:t("cover.title"),"aria-label":t("cover.title"),onclick:()=>{if(current && isOwn(current))coverEditor.open(current);}},icon("image",18));
  const editButton = h("button", { class: "btn", type: "button", onclick: () => edit(), text: t("pl.edit") });
  const syncButton=h("button",{class:"btn",type:"button",text:ft("Сохранять офлайн"),onclick:async()=>{if(!current)return;try{setPlaylistSync(current,!isSynced(current));renderOwner();await syncPlaylists();}catch(error){toast(errorText(error),{error:true});}}});
  const deleteButton = h("button", { class: "btn danger-text", type: "button", onclick: () => remove(), text: t("pl.delete") });
  const followButton = h("button", { class:"btn",type:"button",text:t("pl.follow"),hidden:true,onclick:followShared });
  const links=linkControls(()=>collectionShareLink("playlist",current),()=>current?.title,toast);
  const detail = createTracksScreen({
    title: "",
    hero: true,
    kicker: t("playlists.kicker"),
    hooks: {
      emptyText: t("library.empty"),
      onPlay: play,
      onAction: onTrackAction,
      canBatchRemove:()=>!!current && isOwn(current),
      onBatchRemove:async items=>{const playlist=current;if(!playlist || !isOwn(playlist))return;try{await removeSelected(items,async track=>{await api.removeFromPlaylist(playlist,track);changeCount(playlist,-1);tracks=tracks.filter(other=>!sameTrack(other,track));});}finally{if(current===playlist)detail.setTracks(tracks);}},
      onMenu: (track, at, index) => openTrackMenu(track, at, current && isOwn(current)
        ? [{ label: t("menu.removeFrom"), action: "remove-from-playlist", icon: "trash", destructive: true, run: () => removeTrack(track, index) }]
        : []),
      back: backButton(t("playlists.back"), () => leave()),
      headerExtra: h("div", { class: "set-inline pl-owner" }, editButton, deleteButton, followButton,syncButton,coverButton,links.copy,links.share),
    },
  });
  detail.element.hidden = true;

  const element = h("div", { class: "view" }, listScreen, detail.element);

  function renderGrid() {
    const list = allPlaylists();
    grid.replaceChildren(...list.map((p) => playlistCard(p, open)));
    if (loaded) {
      paintStatus(status, list.length ? "" : t("playlists.empty"), { art: "playlist" });
    }
  }

  function renderOwner() {
    links.refresh();
    syncButton.textContent=current && isSynced(current)?ft("Офлайн-синхронизация включена"):ft("Сохранять офлайн");
    syncButton.setAttribute("aria-pressed",String(!!current && isSynced(current)));
    const own = current && isOwn(current);
    editButton.hidden = deleteButton.hidden = !own;
    coverButton.hidden=!own || !isAndroid();
    followButton.hidden = !sharedAlbum || own || sharedAlbum.is_followed;
  }

  subscribe("playlists", () => {
    renderGrid();
    if (current) {
      const fresh = allPlaylists().find((p) => same(p, current));
      if (fresh) { current = fresh; detail.setTitle(fresh.title); detail.setCover(fresh.artwork_url); detail.setNote(fresh.description ?? ""); }
    }
  });
  subscribe("session", renderOwner);

  let albumsLoaded = false;
  function renderAlbums() {
    if (!albumsLoaded) return;
    const list = followedAlbums();
    albumsGrid.replaceChildren(...list.map((album) => albumCard(album, openAlbum)));
    paintStatus(albumsStatus, list.length ? "" : t("albums.empty"), { extra: "compact" });
  }
  subscribe("albums", renderAlbums);

  async function loadAlbums() {
    const mine = generation;
    paintStatus(albumsStatus, t("loading"), { kind: "cards" });
    try {
      await ensureAlbums();
      if (mine !== generation) return;
      albumsLoaded = true;
      renderAlbums();
    } catch (error) {
      if (mine !== generation) return;
      paintStatus(albumsStatus, errorText(error), { error: true, extra: "compact" });
    }
  }

  function leave() {
    coverEditor.cancel();
    if (pushed) { pushed = false; detailGeneration++; current = null; back?.(); } else showList();
  }

  function showList() {
    coverEditor.cancel();
    detailGeneration++;
    current = null;
    sharedAlbum = null;
    detail.element.hidden = true;
    listScreen.hidden = false;
    enter(listScreen);
  }

  async function open(playlist, shared = null) {
    // Возврат к открытому плейлисту, который ещё на экране, сохраняет место.
    if (pushed && current && same(current, playlist) && !detail.element.hidden) return;
    coverEditor.cancel();
    sharedAlbum = shared;
    current = playlist;
    renderOwner();
    listScreen.hidden = true;
    detail.element.hidden = false;
    enter(detail.element);
    detail.setTitle(playlist.title);
    detail.setCover(playlist.artwork_url);
    detail.setNote(playlist.description ?? "");
    detail.setTracks([]);
    detail.setStatus(t("loading"));
    const mine = ++detailGeneration;
    try {
      const all = await loadAllPages((offset,count) => api.playlistTracks(playlist,offset,count), {
        isCancelled: () => mine !== detailGeneration,
        onProgress: (n, total, items) => {
          if (mine === detailGeneration) detail.setProgress(items.slice(), total);
        },
      });
      if (all === null) return;
      tracks = all;
      detail.clearStatus();
      detail.setTracks(all);
    } catch (error) {
      if (mine === detailGeneration) detail.setStatus(errorText(error), { error: true });
      toast(errorText(error), { error: true });
    }
  }

  async function openLink() {
    const mine = generation;
    const values = await formDialog({ title:t("pl.openLink"),confirm:t("pl.openLink"),fields:[{name:"url",label:t("pl.link"),required:true,max:2048}] });
    if (!values || mine !== generation) return;
    let album;
    try { album=playlistLink(values.url); } catch { toast(t("pl.linkInvalid"),{error:true}); return; }
    album.title=t("pl.shared");
    try {
      const resolved=await api.playlistById(album);
      if (mine !== generation) return;
      open({playlist_id:resolved.album_id,owner_id:resolved.owner_id,title:resolved.title,count:resolved.count,access_key:resolved.access_key,artwork_url:resolved.artwork_url,description:resolved.description},resolved);
    } catch(error) { if (mine === generation) toast(errorText(error),{error:true}); }
  }

  async function followShared() {
    if (!sharedAlbum || !current) return;
    const album=sharedAlbum, playlist=current, mine=detailGeneration;
    followButton.disabled=true;
    try {
      await api.followAlbum(album,true);
      if (mine !== detailGeneration) return;
      sharedAlbum={...album,is_followed:true};
      addPlaylist(playlist);
      renderOwner();
      toast(t("pl.followed"));
    } catch(error) { if (mine === detailGeneration) toast(errorText(error),{error:true}); }
    finally { followButton.disabled=false; }
  }

  async function edit() {
    const account = accountId();
    const playlist = current;
    if (!playlist) return;
    const values = await formDialog({
      title: t("pl.edit"),
      confirm: t("pl.save"),
      fields: [
        { name: "title", label: t("pl.name"), value: playlist.title, required: true, max: 128 },
        { name: "description", label: t("pl.description"), value: playlist.description ?? "", multiline: true, max: 2000 },
        discoveryField(),
      ],
    });
    if (!values || !account || account !== accountId() || current !== playlist) return;
    try {
      const updated = await api.editPlaylist(playlist, values.title, values.description, discoveryChoice(values.discovery));
      if (account === accountId()) replacePlaylist(updated);
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function remove() {
    const playlist = current;
    if (!playlist) return;
    if (!(await confirmDialog({ text: t("pl.deleteConfirm", { title: playlist.title }), confirm: t("pl.delete"), danger: true }))) return;
    try {
      await api.deletePlaylist(playlist);
      dropPlaylist(playlist);
      leave();
      toast(t("pl.deleted"));
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function removeTrack(track, index) {
    const playlist = current;
    try {
      await api.removeFromPlaylist(playlist, track);
      if (playlist !== current) return;
      tracks = tracks.filter((_, i) => i !== index);
      detail.setTracks(tracks);
      changeCount(playlist, -1);
      toast(t("pl.removed", { title: playlist.title }), {
        action: { label: t("toast.undo"), run: () => undoRemove(playlist, track, index) },
      });
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  /** Баннер отмены: возвращает трек (VK может добавить его) и сообщает об этом. */
  async function undoRemove(playlist, track, index) {
    try {
      await api.addToPlaylist(playlist, track);
      changeCount(playlist, 1);
      if (playlist === current || (current && same(playlist, current))) {
        tracks = [...tracks.slice(0, index), track, ...tracks.slice(index)];
        detail.setTracks(tracks);
      }
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  async function load() {
    const mine = ++generation;
    paintStatus(status, t("loading"), { kind: "cards" });
    try {
      await ensurePlaylists();
      if (mine !== generation) return;
      loaded = true;
      renderGrid();
    } catch (error) {
      if (mine === generation) {
        paintStatus(status, errorText(error), { error: true });
      }
      toast(errorText(error), { error: true });
    }
  }

  return {
    element,
    show() {
      if (!loaded) load();
      if (!albumsLoaded) loadAlbums();
    },
    /** Щелчок на боковой панели, когда список воспроизведения открыт, позволяет вернуться к списку. */
    root() { if (current) showList(); },
    /** The phone opens a playlist as a page of its own (see mobile/phone-shell.js). */
    open(playlist, shared) { pushed = true; return open(playlist, shared); },
    reset() {
      generation++;
      loaded = false;
      albumsLoaded = false;
      albumsGrid.replaceChildren();
      forgetPlaylists();
      grid.replaceChildren();
      pushed = false;
      showList();
    },
  };
}
