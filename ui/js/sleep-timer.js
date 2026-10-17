import { h, formatTime } from "./dom.js";
import { api } from "./api.js";
import { t, addStrings, errorText } from "./i18n.js";
import { subscribe, own } from "./store.js";
import { toast } from "./toast.js";

addStrings({ ru: { "sleep.title": "Таймер сна", "sleep.off": "Выключен", "sleep.minutes": "{n} мин", "sleep.end": "В конце трека", "sleep.left": "До паузы: {time}" }, en: { "sleep.title": "Sleep timer", "sleep.off": "Off", "sleep.minutes": "{n} min", "sleep.end": "End of track", "sleep.left": "Pause in {time}" } });

export function createSleepTimer() {
  let deadline = null;
  let ticker = null;
  const select = h("select", { "aria-label": t("sleep.title"), onchange: async () => {
    select.disabled = true;
    try { await api.setSleepTimer(select.value === "end" || select.value === "0" ? null : Number(select.value), select.value === "end"); }
    catch (error) { toast(errorText(error), { error: true }); }
    finally { select.disabled = false; }
  } }, [h("option", { value: "0", text: t("sleep.off") }), ...[15,30,60,120].map((n) => h("option", { value: String(n*60), text: t("sleep.minutes", { n }) })), h("option", { value: "end", text: t("sleep.end") })]);
  const note = h("div", { class: "set-note" });
  function tick() { note.textContent = deadline === null ? "" : t("sleep.left", { time: formatTime(Math.max(0, Math.ceil((deadline-Date.now())/1000))) }); }
  function update({ remaining, end_of_track: end }) {
    clearInterval(ticker);
    deadline = remaining == null ? null : Date.now() + remaining*1000;
    for (const option of [...select.options]) if (option.dataset.custom) option.remove();
    if (remaining != null && ![900,1800,3600,7200].includes(remaining)) select.append(h("option", { value: String(remaining), "data-custom": "1", text: t("sleep.minutes", { n: Math.ceil(remaining/60) }) }));
    select.value = end ? "end" : String(remaining ?? 0);
    tick();
    if (deadline !== null) ticker = setInterval(tick, 1000);
  }
  subscribe("sleep", update);
  own(() => clearInterval(ticker));
  api.sleepTimer().then(([remaining,end_of_track]) => update({remaining,end_of_track})).catch(() => {});
  return h("div", {}, select, note);
}
