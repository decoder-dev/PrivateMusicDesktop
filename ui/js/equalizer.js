import { own } from "./store.js";
import { h } from "./dom.js";
import { t, addStrings } from "./i18n.js";
import { api } from "./api.js";
import { bindRange, syncRange } from "./ui-parts.js";

addStrings({ru:{"eq.curve":"Уровни частот", "eq.hint":"Автоматический запас громкости защищает усиленные частоты от перегруза. Нажмите на значение, чтобы обнулить полосу.","eq.resetBand":"Сбросить {band} Гц", "eq.db":"дБ", "eq.applyError":"Не удалось применить эквалайзер. Попробуйте ещё раз.", "eq.retry":"Повторить", "eq.jazz":"Джаз", "eq.classical":"Классика"},en:{"eq.curve":"Frequency levels", "eq.hint":"Automatic headroom protects boosted frequencies from overload. Select a value to reset its band.","eq.resetBand":"Reset {band} Hz", "eq.db":"dB", "eq.applyError":"Could not apply equaliser settings. Try again.", "eq.retry":"Retry", "eq.jazz":"Jazz", "eq.classical":"Classical"}});
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
  jazz: [3, 2, 1, 1, -1, -1, 0, 1, 2, 3],
  classical: [2, 1, 0, 0, -1, 0, 1, 2, 3, 2],
};

const presetFor = (gains) =>
  Object.entries(PRESETS).find(([, values]) => values.every((v, i) => v === gains[i]))?.[0] ?? "custom";

/** Включение и выключение, пресеты и десять ползунков; изменения достигают движка при перетаскивании. */
export function createEqualizerPanel({ settings, onSettingsChange }) {
  let eq = { ...settings.eq, gains: settings.eq.gains.slice() };
  let timer = 0;
  let disposed=false, sending=false, pending=null;
  let custom=(settings.eq.custom ?? settings.eq.gains).slice();
  const svg=(tag,attrs={})=>{const node=document.createElementNS("http://www.w3.org/2000/svg",tag);for(const [key,value] of Object.entries(attrs))node.setAttribute(key,String(value));return node;};
  const graph=svg("svg",{viewBox:"0 0 600 112",preserveAspectRatio:"none",role:"img","aria-label":t("eq.curve"),class:"eq-curve"});
  for(const y of [16,56,96])graph.append(svg("line",{x1:12,x2:588,y1:y,y2:y,class:y===56?"eq-zero":"eq-grid"}));
  const area=svg("path",{class:"eq-area"}),line=svg("polyline",{class:"eq-line"});graph.append(area,line);
  const dots=LABELS.map(()=>svg("circle",{r:3}));graph.append(...dots);
  const error=h("div",{class:"eq-error",hidden:true,role:"status"},h("span",{text:t("eq.applyError")}),h("button",{class:"btn",type:"button",text:t("eq.retry"),onclick:()=>send()}));

  const toggle = h("input", { type: "checkbox", role: "switch", id: "eq-on", onchange: () => update({ on: toggle.checked },true) });
  const preset = h("select", { class: "field", "aria-label": t("eq.preset"),
    onchange: () => update({ gains: (preset.value === "custom" ? custom : PRESETS[preset.value]).slice(), preset: preset.value, on: true },true) },
    [...Object.keys(PRESETS), "custom"].map((id) => h("option", { value: id, text: t(`eq.${id}`) })));
  const reset = h("button", { class: "btn", type: "button", onclick: () => update({ gains: PRESETS.flat.slice(), preset: "flat" },true), text: t("eq.reset") });

  const values = [];
  const sliders = LABELS.map((label, band) => {
    const value = h("button", { class: "eq-value", type:"button", title:t("eq.resetBand",{band:label}),
      "aria-label":t("eq.resetBand",{band:label}), onclick:()=>changeBand(band,0) });
    values.push(value);
    const input = h("input", {
      type: "range", min: "-12", max: "12", step: "1", "aria-label": `${label} Hz`, class: "eq-slider",
      oninput: () => {
        changeBand(band,Number(input.value));
      },
      onchange:()=>{clearTimeout(timer);send();},
    });
    bindRange(input);
    return { input, element: h("div", { class: "eq-band" }, value, input, h("span", { class: "eq-label", text: label })) };
  });

  const element = h("div", { class: "eq" },
    h("div", { class: "set-row" },
      h("label", { class: "set-check", for: "eq-on" }, h("span", { text: t("eq.on") }), toggle),
      h("div", { class: "set-inline" }, preset, reset)),
    graph,
    h("div", { class: "eq-bands" }, sliders.map((s) => s.element)),
    h("p",{class:"eq-hint",text:t("eq.hint")}),error);

  function changeBand(band,value) {
    const gains=eq.gains.slice(); gains[band]=value; custom=gains.slice();
    update({gains,preset:presetFor(gains),custom:custom.slice(),on:true});
  }

  function render() {
    toggle.checked = eq.on;
    preset.value = presetFor(eq.gains);
    element.classList.toggle("off", !eq.on);
    graph.setAttribute("aria-label",`${t("eq.curve")}: ${eq.gains.map((g,i)=>`${LABELS[i]} Hz ${g} ${t("eq.db")}`).join(", ")}`);
    const points=eq.gains.map((gain,i)=>[12+i*64,56-gain*40/12]);
    line.setAttribute("points",points.map(p=>p.join(",")).join(" "));
    area.setAttribute("d",`M12,56 L${points.map(p=>p.join(",")).join(" L")} L588,56 Z`);
    dots.forEach((dot,i)=>{dot.setAttribute("cx",points[i][0]);dot.setAttribute("cy",points[i][1]);});
    sliders.forEach(({ input }, i) => { input.value = String(eq.gains[i]); input.setAttribute("aria-valuetext",`${eq.gains[i]} ${t("eq.db")}`); syncRange(input); });
    values.forEach((node, i) => { node.textContent = `${eq.gains[i] > 0 ? "+" : ""}${eq.gains[i]}`; });
  }

  async function send() {
    pending={gains:eq.on?eq.gains.slice():null};
    if(sending || disposed)return;
    sending=true;
    try {
      while(pending && !disposed) {
        const current=pending; pending=null;
        try { await api.setEqualizer(current.gains); if(!disposed)error.hidden=true; }
        catch { if(!disposed)error.hidden=false; }
      }
    } finally {sending=false;}
  }

  function update(patch, immediate=false) {
    eq = { ...eq, ...patch };
    render();
    onSettingsChange({ eq: { ...eq, gains: eq.gains.slice() } });
    // Перетаскивание пожаров многих событий; двигателю нужно только самое последнее.
    clearTimeout(timer);
    if(immediate)send();else timer = setTimeout(send, 60);
  }

  own(() => {disposed=true;pending=null;clearTimeout(timer);});
  render();
  return { element, render };
}
