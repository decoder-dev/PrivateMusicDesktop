import { PALETTES, PALETTE_ACCENT } from "../themes.js";
import { createSleepTimer } from "../sleep-timer.js";
import { h } from "../dom.js";
import { t, errorText } from "../i18n.js";
import { api } from "../api.js";
import { store, subscribe } from "../store.js";
import { MODES } from "../visualizer.js";
import { createEqualizerPanel } from "../equalizer.js";
import { icon } from "../icons.js";
import { setWindowMode } from "../window-mode.js";
import { createUpdater } from "../updater.js";
import { ACCENTS } from "../persist.js";
import { addStrings } from "../i18n.js";
import { createTastePanel } from "../tastes.js";
import { createAudioOptions } from "../audio-options.js";
addStrings({ru:{"window.mini":"Мини-плеер"},en:{"window.mini":"Mini player"}});
import { isAndroid } from "../mobile/layout.js";
import { STYLES, currentStyle, setStyle } from "../mobile/style.js";
import { NP_THEMES, currentNpTheme, setNpTheme } from "../mobile/np-theme.js";
addStrings({ ru: { "set.nptheme": "Фон плеера", "set.nptheme.cover": "Обложка", "set.nptheme.aurora": "Аврора", "set.nptheme.plain": "Однотонный" }, en: { "set.nptheme": "Player background", "set.nptheme.cover": "Cover", "set.nptheme.aurora": "Aurora", "set.nptheme.plain": "Plain" } });
addStrings({ ru: { "set.style": "Вид приложения", "set.style.ios": "Как в iOS", "set.style.material": "Material 3" }, en: { "set.style": "App style", "set.style.ios": "Like iOS", "set.style.material": "Material 3" } });

const IS_MAC = /Mac/i.test(navigator.platform || navigator.userAgent);
const MOD = IS_MAC ? "⌘" : "Ctrl";

function section(title, ...rows) {
  return h("section", { class: "set-section" }, h("h2", { text: title }), h("div", { class: "set-card" }, ...rows));
}

/** Отмечает то, что имеет смысл только в настольном окне (на Android скрыто: css/mobile.css). */
function desktopOnly(node) {
  node.dataset.desktopOnly = "";
  return node;
}

/** Небольшая акцентная плитка с глифом: ведущим значком строки настроек. */
const tile = (glyph) => h("span", { class: "set-ico", "aria-hidden": "true" }, icon(glyph, 16));

function row(label, control, note, glyph) {
  return h("div", { class: "set-row" },
    h("div", { class: "set-label" }, glyph ? tile(glyph) : null, h("div", { class: "set-text" }, label, note ? h("div", { class: "set-note", text: note }) : null)),
    h("div", { class: "set-control" }, control));
}

/** Настройки, ярлыки, учетная запись и версия. Каждое изменение применяется сразу. */
export function createSettingsView({ settings, onSettingsChange, onSignOut, toast }) {
  // Тема и акцент применяются одновременно, а сборщики следуют за ними.
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

  // Тема: три круга; акцент: сетка цветных точек (радиогруппы).
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
  /** Клавиши со стрелками перемещаются по группе радиостанций, как это делают родные радиоприемники. */
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
  const updater = createUpdater();
  const windowModes = h("div", {class:"window-modes set-inline"}, ["medium","maximized","fullscreen","mini"].map(mode => h("button",{class:"btn",type:"button","data-window-mode":mode,text:t(`window.${mode}`),onclick:async()=>{try{await setWindowMode(mode);}catch(error){toast(errorText(error),{error:true});}}})));
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
        desktopOnly(row(t("set.output"), h("div", { class: "set-inline" }, deviceSelect, refreshButton), t("set.output.note"), "volume")),createAudioOptions(settings,onSettingsChange)),
      section(t("set.appearance"),
        isAndroid() ? row(t("set.style"), h("select", { class: "field", "aria-label": t("set.style"), onchange: (event) => setStyle(event.target.value) },
          STYLES.map((value) => h("option", { value, selected: value === currentStyle(), text: t(`set.style.${value}`) }))), null, "sparkles") : null,
        row(t("set.nptheme"), h("select", { class: "field", "aria-label": t("set.nptheme"), onchange: (event) => setNpTheme(event.target.value) },
          NP_THEMES.map((value) => h("option", { value, selected: value === currentNpTheme(), text: t(`set.nptheme.${value}`) }))), null, "sparkles"),
        row(t("set.theme"), themePicker, null, "sparkles"),
        row(t("palette.title"), paletteGrid, null, "sparkles"),
        row(t("set.accent"), accentGrid, null, "album"),
        h("div", { class: "set-row" },
          h("label", { class: "set-check", for: "set-flat" }, h("span", { class: "set-label" }, tile("settings"), h("span", { class: "set-text" }, t("set.flat"), h("div", { class: "set-note", text: t("set.flat.note") }))), flatBox))),
      Object.assign(section(t("eq.title"), equalizer.element), { id: "set-eq" }),
      section("Selena",createTastePanel()),
      section(t("sleep.title"), row(t("sleep.action"),createSleepTimer(),t("sleep.note"),"moon")),
      desktopOnly(section(t("set.window"),
        row(t("window.mode"),windowModes,t("window.note"),"expand"),
        h("div", { class: "set-row" },
          h("label", { class: "set-check", for: "set-tray" }, h("span", { class: "set-label" }, tile("collapse"), h("span", { class: "set-text", text: t("set.tray") })), trayBox),
          trayNote))),
      section(t("set.np"),
        desktopOnly(row(t("set.visualizer"), visualizerSelect, null, "visualizer")),
        h("div", { class: "set-row" }, h("label", { class: "set-check", for: "set-lyrics" }, h("span", { class: "set-label" }, tile("lyrics"), h("span", { class: "set-text", text: t("set.lyrics") })), lyricsBox))),
      desktopOnly(section(t("set.keys"),
        h("table", { class: "set-keys" }, h("tbody", {},
          keys.map(([combo, label]) => h("tr", {}, h("td", {}, h("kbd", { text: combo })), h("td", { text: t(label) }))))))),
      section(t("set.account"),
        h("div", { class: "set-row" }, accountName, h("button", { class: "btn", type: "button", onclick: () => onSignOut(), text: t("account.signOut") })),
        persistNote),
      desktopOnly(section(t("update.title"), updater.element)),
      section(t("set.about"),
        h("div", { class: "set-row" }, h("div", {}, h("strong", { text: "Private Music" }), version,
          h("div", { class: "set-note", text: t("set.disclaimer") }))))));

  // Переходы ведут к настоящим разделам этой же прокручиваемой страницы.
  // Клавиатурный фокус переносится на заголовок, чтобы переход был понятен и без мыши.
  const settingsBody = element.querySelector(".set-body");
  const shortcuts = ["set.sound", "set.appearance", "eq.title", "set.window", "update.title"];
  const settingsNav = h("nav", { class:"settings-nav", "aria-label":t("set.title") });
  for (const key of shortcuts) {
    const heading = [...settingsBody.querySelectorAll(".set-section > h2")].find(node => node.textContent === t(key));
    if (!heading) continue;
    const section = heading.parentElement;
    section.id = `settings-${key.replaceAll(".", "-")}`;
    heading.tabIndex = -1;
    settingsNav.append(h("button", {
      type:"button", class:"btn", text:t(key), "aria-controls":section.id, ...(section.hasAttribute("data-desktop-only") ? { "data-desktop-only":"" } : {}),
      onclick:() => {
        const top = section.getBoundingClientRect().top - settingsBody.getBoundingClientRect().top + settingsBody.scrollTop;
        settingsBody.scrollTo({ top:Math.max(0, top - 12), behavior:"instant" });
        heading.focus({ preventScroll:true });
      },
    }));
  }
  element.querySelector(".view-header").append(settingsNav);

  async function loadDevices() {
    refreshButton.disabled = true;
    refreshButton.setAttribute("aria-busy", "true");
    try {
      const names = await api.outputDevices();
      const chosen = settings.outputDevice;
      const options = [h("option", { value: "", text: t("set.output.default") }),
        ...names.map((name) => h("option", { value: name, text: name }))];
      // Сохраненное устройство, которое в данный момент отключено от сети, остается доступным для
      // выбора.
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
    // Группы радио: отмечена одна позиция табуляции.
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
    /** The phone's Profile opens Settings at a section ("eq"). */
    open(payload) {
      if (payload?.section !== "eq") return;
      requestAnimationFrame(() => element.querySelector("#set-eq")?.scrollIntoView({ block: "start" }));
    },
    async show() {
      render();
      // На телефоне нет ни выбора устройства вывода, ни установщика обновлений (их команды-заглушки).
      if (!isAndroid()) { loadDevices(); updater.show(); }
      try {
        const info = await api.appInfo();
        version.textContent = t("set.version", { version: info.version });
        trayBox.disabled = !info.has_tray;
        if (!info.has_tray) trayBox.checked = false;
        trayNote.textContent = info.has_tray ? t("set.tray.note") : t("set.tray.none");
      } catch {
        // Версия и состояние лотка здесь носят косметический характер.
      }
    },
    setProfile(profile) { accountName.textContent = profile ? `${profile.first_name} ${profile.last_name}`.trim() : ""; },
  };
}
