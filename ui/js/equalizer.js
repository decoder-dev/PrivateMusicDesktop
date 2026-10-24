import { icon } from "./icons.js";
import { own } from "./store.js";
import { h } from "./dom.js";
import { t, addStrings } from "./i18n.js";
import { api } from "./api.js";
import { bindRange, syncRange } from "./ui-parts.js";

addStrings({ru:{"eq.curve":"Уровни частот", "eq.hint":"Автоматический запас громкости защищает усиленные частоты от перегруза. Нажмите на значение, чтобы обнулить полосу.","eq.resetBand":"Сбросить {band} Гц", "eq.db":"дБ", "eq.applyError":"Не удалось применить эквалайзер. Попробуйте ещё раз.", "eq.retry":"Повторить", "eq.jazz":"Джаз", "eq.classical":"Классика"},en:{"eq.curve":"Frequency levels", "eq.hint":"Automatic headroom protects boosted frequencies from overload. Select a value to reset its band.","eq.resetBand":"Reset {band} Hz", "eq.db":"dB", "eq.applyError":"Could not apply equaliser settings. Try again.", "eq.retry":"Retry", "eq.jazz":"Jazz", "eq.classical":"Classical"}});
addStrings({"ru": {"eq.bassReduction": "Меньше баса", "eq.trebleReduction": "Меньше высоких", "eq.vocalReduction": "Меньше вокала", "eq.speech": "Речь", "eq.piano": "Фортепиано", "eq.lounge": "Лаунж", "eq.pop": "Поп", "eq.rnb": "R&B", "eq.hipHop": "Хип-хоп", "eq.dance": "Танцевальная", "eq.metal": "Метал", "eq.loudness": "Тонкомпенсация", "eq.smallSpeakers": "Маленькие динамики", "eq.headphones": "Наушники"}, "en": {"eq.bassReduction": "Bass reduction", "eq.trebleReduction": "Treble reduction", "eq.vocalReduction": "Vocal reduction", "eq.speech": "Speech", "eq.piano": "Piano", "eq.lounge": "Lounge", "eq.pop": "Pop", "eq.rnb": "R&B", "eq.hipHop": "Hip hop", "eq.dance": "Dance", "eq.metal": "Metal", "eq.loudness": "Loudness", "eq.smallSpeakers": "Small speakers", "eq.headphones": "Headphones"}});
addStrings({ru:{"eq.previousBands":"Предыдущие частоты","eq.nextBands":"Следующие частоты","eq.visibleBands":"Полосы {from}–{to} из 10"},en:{"eq.previousBands":"Previous frequencies","eq.nextBands":"Next frequencies","eq.visibleBands":"Bands {from}–{to} of 10"}});
export const LABELS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
export const PRESETS = {
  "flat": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  "bass": [6, 6, 5, 3, 1, 0, 0, 0, -1, -1],
  "bassReduction": [-6, -5, -4, -2, -1, 0, 0, 0, 0, 0],
  "treble": [0, 0, 0, 0, 0, 1, 2, 4, 5, 6],
  "trebleReduction": [0, 0, 0, 0, 0, -1, -2, -4, -5, -6],
  "vocal": [-2, -2, -1, 0, 2, 4, 4, 3, 1, 0],
  "vocalReduction": [2, 2, 1, -1, -3, -4, -3, -1, 0, 1],
  "speech": [-4, -3, -1, 1, 4, 5, 5, 3, 0, -3],
  "acoustic": [4, 4, 3, 1, 1, 1, 2, 3, 3, 2],
  "classical": [0, 0, 0, 0, 0, 0, -3, -3, -3, -4],
  "piano": [2, 1, 0, 2, 3, 1, 3, 4, 3, 3],
  "jazz": [3, 2, 1, 2, -2, -2, 0, 1, 2, 3],
  "lounge": [-3, -2, 0, 2, 3, 1, -1, 0, 1, 2],
  "pop": [-1, 1, 3, 4, 3, 1, 0, -1, -1, -2],
  "rnb": [3, 6, 5, 1, -2, -1, 2, 2, 3, 3],
  "hipHop": [5, 4, 1, 3, -1, -1, 1, -1, 2, 3],
  "dance": [6, 5, 2, 0, 0, -3, -4, -4, 0, 0],
  "electronic": [5, 4, 2, 0, -1, 0, 2, 3, 5, 5],
  "rock": [4, 3, 2, 0, -1, 1, 3, 4, 4, 3],
  "metal": [4, 3, 0, -2, -4, -3, 0, 3, 5, 4],
  "loudness": [6, 4, 0, -1, -1, -1, 0, 1, 4, 5],
  "smallSpeakers": [5, 4, 3, 2, 1, 0, -1, -1, 0, 1],
  "headphones": [3, 5, 3, -1, -1, 0, 2, 4, 5, 4],
  "night": [-4, -3, -1, 0, 1, 1, 0, -1, -2, -3],
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
      onchange:()=>{clearTimeout(timer);timer=0;send();},
    });
    bindRange(input);
    return { input, element: h("div", { class: "eq-band" }, value, input, h("span", { class: "eq-label", text: label })) };
  });

  const bands=h("div",{class:"eq-bands"},sliders.map(s=>s.element));
  const previousBands=h("button",{type:"button",class:"icon-btn","aria-label":t("eq.previousBands"),onclick:()=>scrollBands(-1)},icon("back",18));
  const nextBands=h("button",{type:"button",class:"icon-btn","aria-label":t("eq.nextBands"),onclick:()=>scrollBands(1)},icon("chevronRight",18));
  const visibleBands=h("span",{class:"eq-band-range",role:"status"});
  const bandNav=h("div",{class:"eq-band-nav",hidden:true},previousBands,visibleBands,nextBands);
  function scrollBands(direction){bands.scrollBy({left:direction*Math.max(44,bands.clientWidth-44),behavior:matchMedia("(prefers-reduced-motion: reduce)").matches?"instant":"smooth"});}
  function updateBandNav(){
    if(disposed)return;
    const overflow=bands.scrollWidth>bands.clientWidth+1;
    bandNav.hidden=!overflow || !bands.clientWidth;
    previousBands.disabled=bands.scrollLeft<=1;
    nextBands.disabled=bands.scrollLeft+bands.clientWidth>=bands.scrollWidth-1;
    if(!overflow)return;
    const bounds=bands.getBoundingClientRect();
    const shown=sliders.map((slider,index)=>({index,rect:slider.element.getBoundingClientRect()})).filter(({rect})=>rect.right>bounds.left+1 && rect.left<bounds.right-1);
    if(shown.length)visibleBands.textContent=t("eq.visibleBands",{from:shown[0].index+1,to:shown.at(-1).index+1});
  }
  bands.addEventListener("scroll",updateBandNav,{passive:true});
  const bandsObserver=new ResizeObserver(updateBandNav);bandsObserver.observe(bands);

  const element = h("div", { class: "eq" },
    h("div", { class: "set-row" },
      h("label", { class: "set-check", for: "eq-on" }, h("span", { text: t("eq.on") }), toggle),
      h("div", { class: "set-inline" }, preset, reset)),
    graph,
    bands,bandNav,
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
    sliders.forEach(({ input }, i) => { if (input.value !== String(eq.gains[i])) input.value = String(eq.gains[i]); input.setAttribute("aria-valuetext",`${eq.gains[i]} ${t("eq.db")}`); syncRange(input); });
    values.forEach((node, i) => { node.textContent = `${eq.gains[i] > 0 ? "+" : ""}${eq.gains[i]}`; });
  }

  async function send() {
    onSettingsChange({ eq: { ...eq, gains: eq.gains.slice(), custom: custom.slice() } });
    pending={gains:eq.on?eq.gains.slice():null};
    if(sending)return;
    sending=true;
    try {
      while(pending) {
        const current=pending; pending=null;
        try { await api.setEqualizer(current.gains); if(!disposed)error.hidden=true; }
        catch { if(!disposed)error.hidden=false; }
      }
    } finally {sending=false;}
  }

  function update(patch, immediate=false) {
    eq = { ...eq, ...patch };
    render();
    // Apply the latest curve at a bounded rate even during a continuous drag.
    if(immediate) { clearTimeout(timer); timer=0; send(); }
    else if(!timer) timer=setTimeout(()=>{timer=0;send();},80);
  }

  function destroy() {
    if(disposed) return;
    if(timer) {clearTimeout(timer);timer=0;send();}
    disposed=true;
    bandsObserver.disconnect();bands.removeEventListener("scroll",updateBandNav);
  }
  own(destroy);
  render();
  return { element, render, destroy };
}
