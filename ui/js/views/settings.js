import { h } from "../dom.js";
import { t, errorText } from "../i18n.js";
import { api } from "../api.js";
import { store, subscribe } from "../store.js";
import { MODES } from "../visualizer.js";
import { createEqualizerPanel } from "../equalizer.js";

const IS_MAC = /Mac/i.test(navigator.platform || navigator.userAgent);
const MOD = IS_MAC ? "⌘" : "Ctrl";

function section(title, ...rows) {
  return h("section", { class: "set-section" }, h("h2", { text: title }), ...rows);
}

function row(label, control, note) {
  return h("div", { class: "set-row" },
    h("div", { class: "set-label" }, label, note ? h("div", { class: "set-note", text: note }) : null),
    h("div", { class: "set-control" }, control));
}

/** Preferences, shortcuts, account and version. Every change applies at once. */
export function createSettingsView({ settings, onSettingsChange, onSignOut, toast }) {
  const deviceSelect = h("select", { class: "field", "aria-label": t("set.output"),
    onchange: () => chooseDevice(deviceSelect.value || null) });
  const refreshButton = h("button", { class: "btn", type: "button", onclick: () => loadDevices(), text: t("set.output.refresh") });

  const trayBox = h("input", { type: "checkbox", id: "set-tray",
    onchange: () => { onSettingsChange({ closeToTray: trayBox.checked }); api.setCloseToTray(trayBox.checked).catch(() => {}); } });
  const trayNote = h("div", { class: "set-note" });

  const visualizerSelect = h("select", { class: "field", "aria-label": t("set.visualizer"),
    onchange: () => onSettingsChange({ visualizer: visualizerSelect.value }) },
    MODES.map((mode) => h("option", { value: mode, text: t(`np.mode.${mode}`) })));
  const lyricsBox = h("input", { type: "checkbox", id: "set-lyrics",
    onchange: () => onSettingsChange({ lyrics: lyricsBox.checked }) });

  const equalizer = createEqualizerPanel({ settings, onSettingsChange });

  const accountName = h("div", { class: "set-account" });
  const persistNote = h("div", { class: "set-note warn", hidden: true, text: t("set.persist.off") });
  const version = h("div", { class: "set-note" });

  const keys = [
    ["Space", "key.toggle"], ["← / →", "key.seek"], [`${MOD} + ← / →`, "key.skip"], [`${MOD} + ↑ / ↓`, "key.volume"],
    ["M", "key.mute"], ["/", "key.search"], [`${MOD} + F`, "key.filter"], ["F", "key.full"],
    ["V", "key.vis"], ["L", "key.lyrics"], ["F11", "key.window"],
  ];

  const element = h("section", { class: "view settings", hidden: true },
    h("header", { class: "view-header" }, h("h1", { text: t("set.title") })),
    h("div", { class: "set-body" },
      section(t("set.sound"),
        row(t("set.output"), h("div", { class: "set-inline" }, deviceSelect, refreshButton), t("set.output.note"))),
      section(t("eq.title"), equalizer.element),
      section(t("set.window"),
        h("div", { class: "set-row" },
          h("label", { class: "set-check", for: "set-tray" }, trayBox, h("span", { text: t("set.tray") })),
          trayNote)),
      section(t("set.np"),
        row(t("set.visualizer"), visualizerSelect),
        h("div", { class: "set-row" }, h("label", { class: "set-check", for: "set-lyrics" }, lyricsBox, h("span", { text: t("set.lyrics") })))),
      section(t("set.keys"),
        h("table", { class: "set-keys" }, h("tbody", {},
          keys.map(([combo, label]) => h("tr", {}, h("td", {}, h("kbd", { text: combo })), h("td", { text: t(label) })))))),
      section(t("set.account"),
        h("div", { class: "set-row" }, accountName, h("button", { class: "btn", type: "button", onclick: () => onSignOut(), text: t("account.signOut") })),
        persistNote),
      section(t("set.about"),
        h("div", { class: "set-row" }, h("div", {}, h("strong", { text: "Private Music" }), version,
          h("div", { class: "set-note", text: t("set.disclaimer") }))))));

  async function loadDevices() {
    refreshButton.disabled = true;
    try {
      const names = await api.outputDevices();
      const chosen = settings.outputDevice;
      const options = [h("option", { value: "", text: t("set.output.default") }),
        ...names.map((name) => h("option", { value: name, text: name }))];
      // A saved device that is unplugged right now stays selectable.
      if (chosen && !names.includes(chosen)) options.push(h("option", { value: chosen, text: chosen }));
      deviceSelect.replaceChildren(...options);
      deviceSelect.value = chosen ?? "";
    } catch (error) {
      toast(errorText(error), { error: true });
    } finally {
      refreshButton.disabled = false;
    }
  }

  async function chooseDevice(name) {
    try {
      await api.setOutputDevice(name);
      onSettingsChange({ outputDevice: name });
    } catch (error) {
      toast(errorText(error), { error: true });
      deviceSelect.value = settings.outputDevice ?? "";
    }
  }

  function render() {
    trayBox.checked = settings.closeToTray;
    visualizerSelect.value = settings.visualizer ?? "bars";
    lyricsBox.checked = settings.lyrics !== false;
    const profile = store.session.profile;
    accountName.textContent = profile ? `${profile.first_name} ${profile.last_name}`.trim() : "";
    persistNote.hidden = store.session.persistent !== false;
  }

  subscribe("session", render);

  return {
    element,
    async show() {
      render();
      loadDevices();
      try {
        const info = await api.appInfo();
        version.textContent = t("set.version", { version: info.version });
        trayBox.disabled = !info.has_tray;
        if (!info.has_tray) trayBox.checked = false;
        trayNote.textContent = info.has_tray ? t("set.tray.note") : t("set.tray.none");
      } catch {
        // Version and tray state are cosmetic here.
      }
    },
    setProfile(profile) { accountName.textContent = profile ? `${profile.first_name} ${profile.last_name}`.trim() : ""; },
  };
}
