// Home: what the iOS app's Home tab holds. A stage with what is playing (or one
// button to start your mix), the way back into the paused track, then shelves:
// recommendations, VK's mixes with the way into Explore, new releases and what
// you played lately. The full mixes screen ("For you") is pushed from here.

import { h } from "../dom.js";
import { api } from "../api.js";
import { icon } from "../icons.js";
import { errorText, t, tn } from "../i18n.js";
import { formatTime } from "../dom.js";
import { store, subscribe } from "../store.js";
import { isLiked, isBusy } from "../likes.js";
import { artBox, paintStatus, swapArt } from "../ui-parts.js";
import { createMixShelf, createReleasesShelf } from "../shelves.js";
import { createCompactTrackList } from "./compact-list.js";
import "./strings.js";

const RECS = 6;
const RECS_MAX_AGE = 5 * 60 * 1000;
const RECENT = 10;

export function createHomeView({ play, onTrackAction, toast, openTrackMenu, like, push, openPlayer }) {
  let recsLoaded = false;
  let recsLoadedAt = 0;
  let recsLoading = false;
  let recsGeneration = 0;
  let historyGeneration = 0;
  let coverUrl;
  let stageState;

  // -------------------------------------------------------------- the stage
  const chip = h("div", { class: "stage-chip", role: "status" });
  const headline = h("h2", { class: "stage-headline" });
  const cover = h("button", { class: "stage-art", type: "button", onclick: () => (store.player.track ? openPlayer() : startStation()) },
    h("span", { class: "stage-cover" }, artBox(null, { glyph: "sparkles", size: 40 })));
  const startButton = h("button", { class: "btn primary cta stage-cta", type: "button", onclick: () => startStation() }, icon("sparkles", 16), t("home.start"));
  const playButton = h("button", { class: "stage-btn", type: "button", onclick: () => api.toggle() }, icon("play", 22));
  const trackButton = h("button", { class: "stage-track", type: "button", onclick: () => openPlayer() });
  const likeButton = h("button", { class: "stage-btn like", type: "button", onclick: () => like(store.player.track) }, icon("heart", 22));
  const controls = h("div", { class: "stage-controls" }, playButton, trackButton, likeButton);
  const stage = h("section", { class: "stage", "aria-label": t("home.title") }, chip, headline, cover, startButton, controls);

  async function startStation() {
    startButton.disabled = true;
    startButton.setAttribute("aria-busy", "true");
    try {
      const tracks = await api.mix();
      if (tracks.length) await play(tracks, 0, { shuffle: false, radio: { kind: "personal", options: null } });
    } catch (error) {
      toast(errorText(error), { error: true });
    } finally {
      startButton.disabled = false;
      startButton.removeAttribute("aria-busy");
    }
  }

  function renderStage(player) {
    const track = player.track;
    const playing = player.status === "playing" || player.status === "buffering" || player.status === "loading";
    const nextState=JSON.stringify([track?.owner_id,track?.track_id,track?.title,track?.artist,track?.artwork_url,player.status,playing?null:player.position]);
    if(nextState===stageState) return;
    stageState=nextState;
    stage.dataset.state = track ? "playing" : "idle";
    chip.textContent = track ? t("home.now") : t("home.idle");
    chip.classList.toggle("quiet", !track);
    headline.textContent = track ? (track.artist || track.title) : t("home.station");
    startButton.hidden = !!track;
    controls.hidden = !track;
    const url = track?.artwork_url ?? null;
    if (url !== coverUrl) {
      coverUrl = url;
      swapArt(cover.firstElementChild, url, { glyph: track ? "note" : "sparkles", size: 40 }, () => coverUrl === url);
    }
    cover.setAttribute("aria-label", track ? t("np.open") : t("home.start"));
    playButton.replaceChildren(icon(playing && player.status !== "loading" ? "pause" : "play", 22));
    playButton.setAttribute("aria-label", playing && player.status !== "loading" ? t("pause") : t("play"));
    trackButton.textContent = track ? track.title : "";
    trackButton.setAttribute("aria-label", track ? `${track.title} — ${t("np.open")}` : "");
    renderLike();
    renderContinue(player);
  }

  function renderLike() {
    const track = store.player.track;
    likeButton.hidden = !track;
    if (!track) return;
    const liked = isLiked(track);
    likeButton.classList.toggle("on", liked);
    likeButton.disabled = isBusy(track);
    likeButton.setAttribute("aria-pressed", String(liked));
    likeButton.setAttribute("aria-label", liked ? t("like.remove") : t("like.add"));
    likeButton.replaceChildren(icon(liked ? "heart-on" : "heart", 22));
  }

  // ------------------------------------------------- continue listening card
  const resumeArt = h("span", { class: "resume-art" });
  const resumeTitle = h("div", { class: "resume-title" });
  const resumeArtist = h("div", { class: "resume-artist" });
  const resumeAt = h("div", { class: "resume-at tnum" });
  const resume = h("button", { class: "resume-card", type: "button", hidden: true, onclick: () => api.toggle() },
    resumeArt,
    h("div", { class: "resume-text" }, h("div", { class: "resume-kicker", text: t("home.continue") }), resumeTitle, resumeArtist, resumeAt),
    h("span", { class: "resume-go", "aria-hidden": "true" }, icon("play", 18)));
  let resumeUrl;
  function renderContinue(player) {
    // The iOS card appears for a track that is loaded but not playing.
    const track = player.track;
    const show = !!track && player.status !== "playing" && player.status !== "buffering" && player.status !== "loading";
    resume.hidden = !show;
    if (!show) return;
    resumeTitle.textContent = track.title;
    resumeArtist.textContent = track.artist;
    resumeAt.textContent = t("home.continueAt", { time: formatTime(player.position || 0), count: tn("library.count", store.queue.tracks.length || player.queue_len || 1) });
    const url = track.artwork_url ?? null;
    if (url !== resumeUrl || !resumeArt.firstChild) { resumeUrl = url; resumeArt.replaceChildren(artBox(url)); }
  }

  // ------------------------------------------------------------ the shelves
  const recs = createCompactTrackList({
    play: (tracks, index, options) => play(tracks, index, options),
    onAction: onTrackAction, onMenu: (track, at) => openTrackMenu(track, at),
  });
  const recsStatus = h("div", { class: "status", role: "status", hidden: true });
  const recsSection = h("section", { class: "home-recs", hidden: true },
    h("h2", { class: "shelf-title", text: t("home.recs") }), recsStatus, recs.element);

  const mixes = createMixShelf({ play, toast });
  const explore = h("button", { class: "explore-row", type: "button", onclick: () => push("forYou") },
    h("span", { text: t("home.explore") }), h("span", { class: "chev" }, icon("chevronRight", 16)));
  const releases = createReleasesShelf();

  const recentRow = h("div", { class: "release-row recent-row" });
  const recentSection = h("section", { class: "mix-catalog recent-shelf", hidden: true },
    h("h2", { class: "shelf-title", text: t("home.recent") }), recentRow);

  async function loadRecs(force = false) {
    if (!force && (recsLoading || (recsLoaded && Date.now()-recsLoadedAt < RECS_MAX_AGE))) return;
    recsLoading = true;
    const mine = ++recsGeneration;
    try {
      const tracks = await api.recommendations();
      if (mine !== recsGeneration) return;
      recsLoaded = true;
      recsLoadedAt = Date.now();
      paintStatus(recsStatus, "");
      recs.setTracks(tracks.slice(0, RECS));
      recsSection.hidden = tracks.length === 0;
    } catch (error) {
      if (mine !== recsGeneration) return;
      recsSection.hidden = false;
      paintStatus(recsStatus, errorText(error), { error: true, extra: "inline" });
    } finally {
      if (mine === recsGeneration) recsLoading = false;
    }
  }

  async function loadRecent() {
    const mine = ++historyGeneration;
    try {
      const entries = await api.history();
      if (mine !== historyGeneration) return;
      const tracks = entries.map((entry) => entry.track).slice(0, RECENT);
      recentSection.hidden = tracks.length === 0;
      recentRow.replaceChildren(...tracks.map((track, index) => {
        const card = h("button", { class: "card track-card", type: "button", onclick: () => play(tracks, index, { shuffle: false }) },
          h("span", { class: "card-art art" }, artBox(track.artwork_url, { size: 30, className: "fill" })),
          h("div", { class: "name", text: track.title }),
          h("div", { class: "count", text: track.artist }));
        card.addEventListener("contextmenu", (event) => { event.preventDefault(); openTrackMenu(track, { x: event.clientX, y: event.clientY }); });
        return card;
      }));
    } catch {
      recentSection.hidden = true;
    }
  }

  const body = h("div", { class: "home-body" }, stage, resume, recsSection, mixes.element, explore, releases.element, recentSection);
  const element = h("section", { class: "view home-view", hidden: true }, body);

  subscribe("player", renderStage);
  subscribe("likes", renderLike);
  subscribe("queue", () => renderContinue(store.player));
  renderStage(store.player);

  function reload() {
    mixes.reset();
    releases.reset();
    return Promise.allSettled([loadRecs(true), mixes.load(true), releases.load(true), loadRecent()]);
  }

  return {
    element,
    show() {
      loadRecs();
      mixes.load();
      releases.load();
      loadRecent();
    },
    reload,
    reset() {
      recsLoaded = false;
      recsLoadedAt = 0;
      recsLoading = false;
      recsGeneration++;
      historyGeneration++;
      recs.setTracks([]);
      recsSection.hidden = true;
      mixes.reset();
      releases.reset();
      recentSection.hidden = true;
      recentRow.replaceChildren();
    },
  };
}
