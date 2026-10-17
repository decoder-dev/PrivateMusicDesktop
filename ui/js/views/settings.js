import { PALETTES, PALETTE_ACCENT } from "../themes.js";
import { createSleepTimer } from "../sleep-timer.js";
import { h } from "../dom.js";
import { t, errorText } from "../i18n.js";
import { api } from "../api.js";
import { store, subscribe } from "../store.js";
import { MODES } from "../visualizer.js";
import { createEqualizerPanel } from "../equalizer.js";
import { icon } from "../icons.js";
import { ACCENTS } from "../persist.js";

const IS_MAC = /Mac/i.test(navigator.platform || navigator.userAgent);
const MOD = IS_MAC ? "⌘" : "Ctrl";

function section(title, ...rows) {
  return h("section", { class: "set-section" }, h("h2", { text: title }), h("div", { class: "set-card" }, ...rows));
}

/** A small accent tile with a glyph: the leading icon of a settings row. */
const tile = (glyph) => h("span", { class: "set-ico", "aria-hidden": "true" }, icon(glyph, 16));

function row(label, control, note, glyph) {
  return h("div", { class: "set-row" },
    h("div", { class: "set-label" }, glyph ? tile(glyph) : null, h("div", { class: "set-text" }, label, note ? h("div", { class: "set-note", text: note }) : null)),
    h("div", { class: "set-control" }, control));
}

/** Preferences, shortcuts, account and version. Every change applies at once. */
export function createSettingsView({ settings, onSettingsChange, onSignOut, toast }) {
  // Theme and accent apply at once, and the pickers follow.
  const pick = (patch) => { onSettingsChange(patch); render(); };
  const deviceSelect = h("select", { class: "field", "aria-label": t("set.output"),
    onchange: () => chooseDevice(deviceSelect.value || null) });
  const refreshButton = h("button", { class: "btn", type: "button", onclick: () => loadDevices(), text: t("set.output.refresh") });

  const trayBox = h("input", { type: "checkbox", role: "switch", id: "set-tray",
    onchange: () => { onSettingsChange({ closeToTray: trayBox.checked }); api.setCloseToTray(trayBox.checked).catch(() => {}); } });
  const trayNote = h("div", { class: "set-note" });

  const visualizerSelect = h("select", { class: "field", "aria-label": t("set.visualizer"),
    onchange: () => onSettingsChange({ visualizer: visualizerSelect.value }) },
    MODES.map((mode) => h("option", { value: mode, text: t(`np.mode.${mode}`) })));
  const lyricsBox = h("input", { type: "checkbox", role: "switch", id: "set-lyrics",
    onchange: () => onSettingsChange({ lyrics: lyricsBox.checked }) });
  const flatBox = h("input", { type: "checkbox", role: "switch", id: "set-flat",
    onchange: () => pick({ flatChrome: flatBox.checked }) });

  // Theme: three circles; accent: a grid of colour dots (radio groups).
  const miniUi = (mode) => h("span", { class: `tp-ui ${mode}`, "aria-hidden": "true" },
    h("i", { class: "tp-side" }), h("i", { class: "tp-line a" }), h("i", { class: "tp-line b" }), h("i", { class: "tp-line c" }), h("i", { class: "tp-bar" }));
  const themeButtons = ["system", "light", "dark"].map((value) =>
    h("button", { type: "button", role: "radio", class: "theme-opt", "data-v": value, "aria-checked": "false", tabindex: "-1", onclick: () => pick({ theme: value }) },
      h("span", { class: "theme-thumb" },
        h("span", { class: `theme-swatch ${{ system: "sys", light: "lt", dark: "dk" }[value]}` },
          value === "system" ? [miniUi("lt"), miniUi("dk")] : miniUi(value === "light" ? "lt" : "dk")),
        h("span", { class: "tp-check", "aria-hidden": "true" }, icon("check", 12))),
      h("span", { class: "theme-label", text: t(`set.theme.${value}`) })));
  const themePicker = h("div", { class: "theme-picker", role: "radiogroup", "aria-label": t("set.theme") }, themeButtons);
  const accentButtons = ACCENTS.map((value) =>
    h("button", { type: "button", role: "radio", class: "accent-opt", "data-v": value, title: t(`set.accent.${value}`), "aria-label": t(`set.accent.${value}`), "aria-checked": "false", tabindex: "-1",
      onclick: () => pick({ accent: value }) }, icon("check", 18)));
  const accentGrid = h("div", { class: "accent-grid", role: "radiogroup", "aria-label": t("set.accent") }, accentButtons);
  const paletteButtons = PALETTES.map((value) => h("button", { type: "button", role: "radio", class: "palette-opt", "data-palette-choice": value, "aria-checked": "false", onclick: () => pick({ palette: value, accent: PALETTE_ACCENT[value] }) }, h("span", { class: `palette-swatch palette-${value}`, "aria-hidden": "true" }), t(`palette.${value}`)));
  const paletteGrid = h("div", { class: "palette-grid", role: "radiogroup", "aria-label": t("palette.title") }, paletteButtons);
  /** Arrow keys move through a radio group like native radios do. */
  for (const group of [themePicker, accentGrid, paletteGrid]) {
    group.addEventListener("keydown", (event) => {
      if (!["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp"].includes(event.key)) return;
      const items = [...group.querySelectorAll("[role=radio]")];
      const index = items.indexOf(document.activeElement);
      if (index < 0) return;
      event.preventDefault();
      event.stopPropagation();
      const next = items[(index + (event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length];
      next.focus();
      next.click();
    });
  }

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
    h("header", { class: "view-header" }, h("div", { class: "vh-text" }, h("h1", { text: t("set.title") }))),
    h("div", { class: "set-body" },
      section(t("set.sound"),
        row(t("set.output"), h("div", { class: "set-inline" }, deviceSelect, refreshButton), t("set.output.note"), "volume")),
      section(t("set.appearance"),
        row(t("set.theme"), themePicker, null, "sparkles"),
        row(t("palette.title"), paletteGrid, null, "sparkles"),
        row(t("set.accent"), accentGrid, null, "album"),
        h("div", { class: "set-row" },
          h("label", { class: "set-check", for: "set-flat" }, h("span", { class: "set-label" }, tile("settings"), h("span", { class: "set-text" }, t("set.flat"), h("div", { class: "set-note", text: t("set.flat.note") }))), flatBox))),
      section(t("eq.title"), equalizer.element),
      section(t("sleep.title"), createSleepTimer()),
      section(t("set.window"),
        h("div", { class: "set-row" },
          h("label", { class: "set-check", for: "set-tray" }, h("span", { class: "set-label" }, tile("collapse"), h("span", { class: "set-text", text: t("set.tray") })), trayBox),
          trayNote)),
      section(t("set.np"),
        row(t("set.visualizer"), visualizerSelect, null, "visualizer"),
        h("div", { class: "set-row" }, h("label", { class: "set-check", for: "set-lyrics" }, h("span", { class: "set-label" }, tile("lyrics"), h("span", { class: "set-text", text: t("set.lyrics") })), lyricsBox))),
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
    refreshButton.setAttribute("aria-busy", "true");
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
      refreshButton.removeAttribute("aria-busy");
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
    flatBox.checked = settings.flatChrome === true;
    // Radio groups: the checked one is the single tab stop.
    for (const button of themeButtons) {
      const on = button.dataset.v === (settings.theme ?? "system");
      button.setAttribute("aria-checked", String(on));
      button.tabIndex = on ? 0 : -1;
    }
    for (const button of accentButtons) {
      const on = button.dataset.v === (settings.accent ?? "blue");
      button.setAttribute("aria-checked", String(on));
      button.tabIndex = on ? 0 : -1;
    }
    for (const button of paletteButtons) {
      const on = button.dataset.paletteChoice === (settings.palette ?? "classic");
      button.setAttribute("aria-checked", String(on));
      button.tabIndex = on ? 0 : -1;
    }
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
