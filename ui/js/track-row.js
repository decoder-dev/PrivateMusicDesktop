import { h, formatTime } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";

export const ROW_HEIGHT = 56;

function cover(url) {
  if (!url) return h("div", { class: "cover empty" }, icon("note", 18));
  return h("img", { class: "cover", src: url, alt: "", loading: "lazy", decoding: "async", referrerpolicy: "no-referrer", draggable: "false" });
}

/**
 * One track line. `extra` is an optional list of [iconName, action, label]
 * overriding the default hover actions.
 */
export function trackRow(track, { selected = false, actions, liked = false, busy = false, pinLiked = true } = {}) {
  const likeLabel = liked ? t("like.remove") : t("like.add");
  const like = h("button", {
    class: `icon-btn like${liked ? " on" : ""}${liked && pinLiked ? " pinned" : ""}`, type: "button", "data-action": "like",
    title: likeLabel, "aria-label": likeLabel, "aria-pressed": String(liked), disabled: busy,
  }, icon(liked ? "heart-on" : "heart", 16));
  const buttons = (actions ?? [
    ["play-next", "play-next", t("playNext")],
    ["queue-add", "play-last", t("playLast")],
    ["radio", "radio", t("radio")],
  ]).map(([name, action, label]) =>
    h("button", { class: "icon-btn", type: "button", "data-action": action, title: label, "aria-label": label }, icon(name, 16)));
  return h("div", { class: "row", role: "option", "aria-selected": selected ? "true" : "false", tabindex: "0" },
    cover(track.artwork_url),
    h("div", { class: "titles" },
      h("div", { class: "title", text: track.title }),
      h("div", { class: "artist", text: track.artist })),
    h("div", { class: "time", text: formatTime(track.duration) }),
    h("div", { class: "actions" }, like, buttons));
}
