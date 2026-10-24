import { playbackAction } from "./playback-action.js";
import { morph } from "./motion.js";
import { h, formatTime } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { api } from "./api.js";
import { store, subscribe } from "./store.js";
import { isLiked, isBusy } from "./likes.js";
import { hasArtist, openArtistOf } from "./navigation.js";
import { artBox, bindRange, syncRange, swapArt } from "./ui-parts.js";
import { coverPalette } from "./palette.js";
import { isPhone } from "./mobile/layout.js";
import { attachMiniGestures } from "./mobile/mini-gestures.js";

/** Нижняя панель: что играет, транспорт, позиция и громкость. */
export function createPlayerBar({ onSettingsChange, onExpand, onLike }) {
  let dragging = false;
  let lastVolume = 1;

  const coverSlot = h("span", { class: "cover-slot" }, artBox(null, { size: 22 }));
  const title = h("div", { class: "title", text: t("nothing") });
  // Исполнитель открывает свою страницу, когда трек называет свое.
  const artist = h("button", { class: "artist link-text", type: "button", title: t("artist.open"), disabled: true,
    onclick: () => openArtistOf(store.player.track) });

  const shuffleButton = h("button", { class: "icon-btn shuffle", type: "button", title: t("shuffle.toggle"), "aria-label": t("shuffle.toggle"), "aria-pressed": "false",
    onclick: () => { playbackAction(() => api.setShuffle(!store.player.shuffle)); } }, icon("shuffle", 18));
  const previousButton = h("button", { class: "icon-btn", type: "button", title: t("previous"), "data-kbd": "Ctrl+Left", "aria-label": t("previous"), onclick: () => playbackAction(() => api.previous(), previousButton) }, icon("prev", 20));
  const playButton = h("button", { class: "icon-btn play", type: "button", title: t("play"), "data-kbd": "Space", "aria-label": t("play"), onclick: () => playbackAction(() => api.toggle(), playButton) }, icon("play", 20));
  const nextButton = h("button", { class: "icon-btn", type: "button", title: t("next"), "data-kbd": "Ctrl+Right", "aria-label": t("next"), onclick: () => playbackAction(() => api.next(), nextButton) }, icon("next", 20));
  const repeatButton = h("button", { class: "icon-btn repeat", type: "button", title: t("repeat.off"), "aria-label": t("repeat.off"),
    onclick: () => { playbackAction(() => api.setRepeat({ off: "all", all: "one", one: "off" }[store.player.repeat] ?? "off")); } }, icon("repeat", 18));

  const elapsed = h("span", { text: "0:00" });
  /** Правая метка ведет обратный отсчет, как скруббер iOS. */
  const remaining = (seconds) => seconds > 0 ? `-${formatTime(seconds)}` : formatTime(0);
  const total = h("span", { text: remaining(0) });
  const seek = h("input", {
    class: "reveal", type: "range", min: "0", max: "0", step: "1", value: "0", "aria-label": t("seek"),
    oninput: () => { dragging = true; elapsed.textContent = formatTime(Number(seek.value)); total.textContent = remaining(Number(seek.max) - Number(seek.value)); },
    onchange: () => { playbackAction(() => api.seek(Number(seek.value))); dragging = false; },
  });
  bindRange(seek);
  const buffering = h("div", { class: "buffering", hidden: true, role: "status", text: t("buffering") });

  const muteButton = h("button", { class: "icon-btn", type: "button", title: t("mute"), "data-kbd": "M", "aria-label": t("mute"),
    onclick: () => toggleMute() }, icon("volume", 18));
  const volume = h("input", {
    class: "reveal", type: "range", min: "0", max: "100", step: "1", value: "100", "aria-label": t("volume"),
    oninput: () => setVolume(Number(volume.value) / 100),
  });
  bindRange(volume);

  function setVolume(value) {
    const clamped = Math.min(1, Math.max(0, value));
    playbackAction(async () => {
      await api.setVolume(clamped);
      if (clamped > 0) lastVolume = clamped;
      onSettingsChange?.({ volume: clamped });
    });
  }

  function toggleMute() {
    setVolume(store.player.volume > 0 ? 0 : lastVolume || 1);
  }

  const expandButton = h("button", { class: "icon-btn", type: "button", title: t("np.open"), "data-kbd": "F", "aria-label": t("np.open"),
    onclick: () => onExpand?.() }, icon("expand", 18));
  const likeButton = h("button", { class: "icon-btn like", type: "button", hidden: true, onclick: () => onLike?.(store.player.track) }, icon("heart", 18));
  function renderLike() {
    const track = store.player.track;
    likeButton.hidden = !track;
    if (!track) return;
    const liked = isLiked(track);
    const label = liked ? t("like.remove") : t("like.add");
    likeButton.classList.toggle("on", liked);
    likeButton.title = label;
    likeButton.setAttribute("aria-label", label);
    likeButton.setAttribute("aria-pressed", String(liked));
    likeButton.disabled = isBusy(track);
    const changed = likeButton.dataset.liked !== String(liked);
    likeButton.dataset.liked = String(liked);
    if(changed || !likeButton.dataset.ready) likeButton.replaceChildren(icon(liked ? "heart-on" : "heart", 18));
    if (changed && likeButton.dataset.ready) morph(likeButton.firstChild);
    likeButton.dataset.ready = "1";
  }
  const coverButton = h("button", { class: "cover-button", type: "button", title: t("np.open"), "aria-label": t("np.open"), onclick: () => onExpand?.() }, coverSlot);

  const element = h("footer", { class: "player glass no-tint is-empty" },
    h("div", { class: "player-inner" },
      h("div", { class: "now" }, coverButton, h("div", { class: "titles" }, title, artist)),
      h("div", { class: "transport" },
        h("div", { class: "controls" }, shuffleButton, previousButton, playButton, nextButton, repeatButton, likeButton),
        h("div", { class: "seek" }, elapsed, seek, total),
        buffering),
      h("div", { class: "volume" }, muteButton, volume, expandButton)));

  // The phone's mini player: a tap anywhere but a button opens the player, and the
  // swipes of the iOS MiniPlayerGesturePolicy work (left/right = next/previous, up = open).
  element.addEventListener("click", (event) => {
    // composedPath: a button that re-draws its glyph while it handles the click has already detached its target.
    if (isPhone() && !event.composedPath().some((node) => node.matches?.("button, input"))) onExpand?.();
  });
  attachMiniGestures(element, { onNext: () => playbackAction(() => api.next()), onPrevious: () => playbackAction(() => api.previous()), onOpen: () => onExpand?.() });

  let coverUrl;
  let tintToken = 0;

  /** Панель меняет оттенок обложки один раз на трек. */
  async function applyTint(url) {
    const mine = ++tintToken;
    if (!url) { element.classList.add("no-tint"); return; }
    try {
      const palette = await coverPalette(url);
      if (mine !== tintToken) return;
      element.style.setProperty("--tint", palette.primary);
      element.classList.remove("no-tint");
    } catch {
      if (mine === tintToken) element.classList.add("no-tint");
    }
  }

  function render(player) {
    const track = player.track;
    const titleText=track ? track.title : t("nothing");
    if(title.textContent!==titleText) title.textContent=titleText;
    const artistText=track ? track.artist : "";
    if(artist.textContent!==artistText) artist.textContent=artistText;
    artist.disabled = !hasArtist(track);
    const url = track?.artwork_url ?? null;
    if (url !== coverUrl) {
      coverUrl = url;
      swapArt(coverSlot, url, { size: 22 }, () => coverUrl === url);
      applyTint(url);
    }
    element.classList.toggle("is-empty", !track);

    const playing = player.status === "playing" || player.status === "buffering" || player.status === "loading";
    const spinning = player.status === "buffering" || player.status === "loading";
    const glyph = playing ? "pause" : "play";
    const changed = playButton.dataset.glyph !== glyph;
    playButton.dataset.glyph = glyph;
    if(changed || playButton.classList.contains("is-buffering") !== spinning) playButton.replaceChildren(icon(glyph, 20), ...(spinning ? [h("span", { class: "spin-over", "aria-hidden": "true" }, h("span", { class: "spinner" }))] : []));
    playButton.classList.toggle("is-buffering", spinning);
    if (changed && playButton.dataset.ready) morph(playButton.firstChild);
    playButton.dataset.ready = "1";
    const label = playing ? t("pause") : t("play");
    playButton.title = label;
    playButton.setAttribute("aria-label", label);

    shuffleButton.classList.toggle("on", player.shuffle);
    shuffleButton.setAttribute("aria-pressed", String(player.shuffle));
    const repeatLabel = t(`repeat.${player.repeat}`);
    repeatButton.classList.toggle("on", player.repeat !== "off");
    repeatButton.title = repeatLabel;
    repeatButton.setAttribute("aria-label", repeatLabel);
    if(repeatButton.dataset.repeat !== player.repeat) {
      repeatButton.dataset.repeat=player.repeat;
      repeatButton.replaceChildren(icon("repeat", 18), ...(player.repeat === "one" ? [h("span", { class: "badge", text: "1" })] : []));
    }

    const duration = Math.max(0, player.duration || 0);
    seek.max = String(Math.max(duration, 1));
    if (!dragging) {
      seek.value = String(Math.min(player.position || 0, duration || 0));
      elapsed.textContent = formatTime(player.position || 0);
      total.textContent = remaining(duration - (player.position || 0));
    }
    syncRange(seek);
    seek.disabled = !track;
    buffering.hidden = player.status !== "buffering";

    volume.value = String(Math.round((player.volume ?? 1) * 100));
    syncRange(volume);
    const volumeGlyph=player.volume>0?"volume":"mute";
    if(muteButton.dataset.glyph!==volumeGlyph) {
      muteButton.dataset.glyph=volumeGlyph;
      muteButton.replaceChildren(icon(volumeGlyph,18));
    }
    previousButton.disabled = nextButton.disabled = playButton.disabled = !track && player.queue_len === 0;
  }

  subscribe("player", (player) => { render(player); renderLike(); });
  subscribe("likes", renderLike);
  render(store.player);
  return { element, toggleMute, volumeUp: () => setVolume(store.player.volume + 0.05), volumeDown: () => setVolume(store.player.volume - 0.05) };
}
