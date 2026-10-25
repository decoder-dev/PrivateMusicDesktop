// The Profile tab (iOS ProfileView): who is signed in, then grouped inset lists:
// Settings, Equaliser and Statistics, the app's version, and Sign out.

import { h } from "../dom.js";
import { api } from "../api.js";
import { icon } from "../icons.js";
import { t } from "../i18n.js";
import { store, subscribe } from "../store.js";
import { artBox } from "../ui-parts.js";
import { confirmDialog } from "../dialog.js";
import "./strings.js";

function groupRow({ glyph, title, note, trailing = "chevron", destructive = false, onclick, disabled = false, id }) {
  return h("button", { type: "button", class: `group-row${destructive ? " destructive" : ""}`, onclick, disabled, "data-row": id ?? null },
    h("span", { class: "group-ico", "aria-hidden": "true" }, icon(glyph, 18)),
    h("span", { class: "group-text" }, h("span", { class: "group-title", text: title }), note ? h("span", { class: "group-note", text: note }) : null),
    trailing === "chevron" ? h("span", { class: "chev", "aria-hidden": "true" }, icon("chevronRight", 14)) : null);
}

export function createProfileView({ push, onSignOut }) {
  const avatar = h("span", { class: "profile-avatar", "aria-hidden": "true" });
  const name = h("div", { class: "profile-name" });
  const identity = h("div", { class: "profile-identity" }, avatar, h("div", { class: "profile-who" }, name, h("div", { class: "profile-app", text: t("app.name") })));
  const version = h("span", { class: "group-note tnum" });

  const sectionLabel = (text) => h("h2", { class: "group-label", text });
  const account = h("section", { class: "group-section" }, sectionLabel(t("profile.account")),
    h("div", { class: "group" },
      groupRow({ glyph: "settings", title: t("nav.settings"), note: t("profile.settingsNote"), id: "settings", onclick: () => push("settings") }),
      groupRow({ glyph: "chart", title: t("nav.stats"), note: t("profile.statsNote"), id: "stats", onclick: () => push("stats") })));
  const about = h("section", { class: "group-section" }, sectionLabel(t("set.about")),
    h("div", { class: "group" },
      h("div", { class: "group-row static" },
        h("span", { class: "group-ico", "aria-hidden": "true" }, icon("info", 18)),
        h("span", { class: "group-text" }, h("span", { class: "group-title", text: t("app.name") }), version))),
    h("p", { class: "group-foot", text: t("set.disclaimer") }));
  const session = h("section", { class: "group-section" }, sectionLabel(t("profile.session")),
    h("div", { class: "group" },
      groupRow({ glyph: "logout", title: t("account.signOut"), destructive: true, trailing: null, id: "signout", onclick: async () => {
        const ok = await confirmDialog({ text: `${t("profile.signOutAsk")} ${t("profile.signOutNote")}`, confirm: t("account.signOut"), danger: true });
        if (ok) onSignOut();
      } })));

  const element = h("section", { class: "view profile-view", hidden: true },
    h("header", { class: "view-header" }, h("div", { class: "vh-text" }, h("h1", { text: t("profile.title") }))),
    h("div", { class: "profile-body" }, identity, account, about, session));

  function render() {
    const profile = store.session.profile;
    const full = profile ? `${profile.first_name ?? ""} ${profile.last_name ?? ""}`.trim() : "";
    name.textContent = full || t("profile.listener");
    avatar.textContent = [...full.split(/\s+/).filter(Boolean).slice(0, 2).map((word) => [...word][0].toUpperCase())].join("");
    avatar.classList.toggle("empty", !full);
    if (profile?.photo_url) avatar.replaceChildren(artBox(profile.photo_url, { glyph: "artist", size: 28, eager: true }));
    else if (!full) avatar.replaceChildren(icon("artist", 28));
  }
  subscribe("session", render);
  render();

  return {
    element,
    show() {
      render();
      api.appInfo().then((info) => { version.textContent = t("set.version", { version: info.version }); }).catch(() => {});
    },
    setProfile() { render(); },
  };
}
