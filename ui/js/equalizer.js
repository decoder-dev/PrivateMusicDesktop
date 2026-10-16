import { h } from "./dom.js";
import { t } from "./i18n.js";
import { api } from "./api.js";
import { bindRange, syncRange } from "./ui-parts.js";

export const LABELS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
export const PRESETS = {
  flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  bass: [7, 6, 4, 2, 0, 0, 0, 0, 0, 0],
  treble: [0, 0, 0, 0, 0, 1, 2, 4, 6, 7],
  vocal: [-2, -2, -1, 1, 3, 4, 3, 1, 0, -1],
  rock: [5, 4, 2, -1, -2, -1, 1, 3, 4, 5],
  electronic: [6, 5, 2, 0, -2, 0, 1, 2, 4, 5],
  acoustic: [3, 3, 2, 1, 1, 1, 2, 2, 2, 1],
  night: [-4, -3, -1, 0, 1, 1, 0, -1, -2, -3],
};

const presetFor = (gains) =>
  Object.entries(PRESETS).find(([, values]) => values.every((v, i) => v === gains[i]))?.[0] ?? "custom";

/** On/off, presets and ten sliders; changes reach the engine as you drag. */
export function createEqualizerPanel({ settings, onSettingsChange }) {
  let eq = { ...settings.eq, gains: settings.eq.gains.slice() };
  let timer = 0;

  const toggle = h("input", { type: "checkbox", role: "switch", id: "eq-on", onchange: () => update({ on: toggle.checked }) });
  const preset = h("select", { class: "field", "aria-label": t("eq.preset"),
    onchange: () => preset.value !== "custom" && update({ gains: PRESETS[preset.value].slice(), preset: preset.value, on: true }) },
    [...Object.keys(PRESETS), "custom"].map((id) => h("option", { value: id, text: t(`eq.${id}`) })));
  const reset = h("button", { class: "btn", type: "button", onclick: () => update({ gains: PRESETS.flat.slice(), preset: "flat" }), text: t("eq.reset") });

  const values = [];
  const sliders = LABELS.map((label, band) => {
    const value = h("span", { class: "eq-value" });
    values.push(value);
    const input = h("input", {
      type: "range", min: "-12", max: "12", step: "1", "aria-label": `${label} Hz`, class: "eq-slider",
      oninput: () => {
        const gains = eq.gains.slice();
        gains[band] = Number(input.value);
        update({ gains, preset: presetFor(gains), on: true });
      },
    });
    bindRange(input);
    return { input, element: h("div", { class: "eq-band" }, value, input, h("span", { class: "eq-label", text: label })) };
  });

  const element = h("div", { class: "eq" },
    h("div", { class: "set-row" },
      h("label", { class: "set-check", for: "eq-on" }, h("span", { text: t("eq.on") }), toggle),
      h("div", { class: "set-inline" }, preset, reset)),
    h("div", { class: "eq-bands" }, sliders.map((s) => s.element)));

  function render() {
    toggle.checked = eq.on;
    preset.value = presetFor(eq.gains);
    element.classList.toggle("off", !eq.on);
    sliders.forEach(({ input }, i) => { input.value = String(eq.gains[i]); syncRange(input); });
    values.forEach((node, i) => { node.textContent = `${eq.gains[i] > 0 ? "+" : ""}${eq.gains[i]}`; });
  }

  function send() {
    api.setEqualizer(eq.on ? eq.gains : null).catch(() => {});
  }

  function update(patch) {
    eq = { ...eq, ...patch };
    render();
    onSettingsChange({ eq: { ...eq, gains: eq.gains.slice() } });
    // Dragging fires many events; the engine needs only the latest.
    clearTimeout(timer);
    timer = setTimeout(send, 60);
  }

  render();
  return { element, render };
}
