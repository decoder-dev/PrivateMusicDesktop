import { h, formatTime } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { hasArtist } from "./navigation.js";
import { artBox } from "./ui-parts.js";

export const ROW_HEIGHT = 56;

/** The current-track mark: four static capsules while playing, a pause glyph when paused. */
function indicator(paused) {
  if (paused) return h("span", { class: "pbars paused", "aria-hidden": "true" }, icon("pause", 12));
  return h("span", { class: "pbars", "aria-hidden": "true" }, h("i"), h("i"), h("i"), h("i"));
}

/**
 * One track line. `actions` is an optional list of [iconName, action, label]
 * overriding the default hover actions. Rows are recreated while the list
 * scrolls, so nothing here may animate or transition.
 */
export function trackRow(track, { selected = false, paused = false, actions, liked = false, busy = false, pinLiked = true, downloaded = false, downloading = false } = {}) {
  const likeLabel = liked ? t("like.remove") : t("like.add");
  const like = h("button", {
    class: `icon-btn like${liked ? " on" : ""}${liked && pinLiked ? " pinned" : ""}`, type: "button", "data-action": "like",
    title: likeLabel, "aria-label": likeLabel, "aria-pressed": String(liked), disabled: busy,
  }, icon(liked ? "heart-on" : "heart", 16));
  const downloadLabel = downloading ? t("dl.working") : downloaded ? t("dl.remove") : t("dl.add");
  const download = h("button", {
    class: `icon-btn download${downloaded ? " on pinned" : ""}${downloading ? " busy pinned" : ""}`, type: "button",
    "data-action": "download", title: downloadLabel, "aria-label": downloadLabel, "aria-pressed": String(downloaded), disabled: downloading,
  }, icon(downloaded ? "downloaded" : "download", 16));
  const buttons = [...(actions ?? []), ["more", "menu", t("menu.more")]].map(([name, action, label]) =>
    h("button", { class: "icon-btn", type: "button", "data-action": action, title: label, "aria-label": label }, icon(name, 16)));

  const badges = h("div", { class: "badges" },
    selected ? indicator(paused) : null,
    liked && pinLiked ? h("span", { class: "badge-liked", "aria-hidden": "true" }, icon("heart-on", 12)) : null,
    downloaded ? h("span", { "aria-hidden": "true" }, icon("downloaded", 12)) : null,
    downloading ? h("span", { class: "spinner", "aria-hidden": "true" }) : null);
  return h("div", { class: "row", role: "option", "aria-selected": selected ? "true" : "false", tabindex: "0" },
    artBox(track.artwork_url),
    h("div", { class: "titles" },
      h("div", { class: "title", text: track.title }),
      hasArtist(track)
        ? h("button", { class: "artist link-text", type: "button", "data-action": "artist", tabindex: "-1", title: t("artist.open"), text: track.artist })
        : h("div", { class: "artist", text: track.artist })),
    badges,
    h("div", { class: "time", text: formatTime(track.duration) }),
    h("div", { class: "actions" }, like, download, buttons));
}
