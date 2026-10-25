import { installLyricsCopy, copyLyricsText } from "./lyrics-copy.js";
import { icon } from "./icons.js";
import { own } from "./store.js";
import { ft } from "./feature-i18n.js";
import { h } from "./dom.js";
import { t } from "./i18n.js";
import { api, trackKey } from "./api.js";
import { motionOk } from "./motion.js";
import { parseLocalLrc,personalLyrics,savePersonalLyrics } from "./lyrics-personal.js";
import { savedLyrics, rememberLyrics, forgetSavedLyrics } from "./saved-lyrics.js";
import { accountId } from "./personal.js";
import { toast } from "./toast.js";
import { decodeEntities } from "./html-entities.js";
import { errorText } from "./i18n.js";
import { openMenu } from "./menu.js";

const CACHE_LIMIT = 60;
/** Строка считается текущей незадолго до ее отметки, поэтому она загорается во время пения. */
const LEAD_SECONDS = 0.25;
/**
 * После того, как пользователь прокручивает вручную, представление остается на месте в течение этого
 * времени.
 */
const HAND_SCROLL_MS = 4000;

/**
 * Столбец текста в полноэкранном режиме: временные строки, следующие за песней (щелкните одну из них,
 * чтобы перейти туда), или простой текст, если время не установлено.
 */
export function createLyricsPanel() {
  const cache = new Map();
  let token = 0;
  let currentAccount = null;
  let current = null; // трек, который показывается
  let lines = []; // [{ время, узел }]
  let active = -1;
  let handScrollUntil = 0;
  let autoFollow = readAutoFollow();
  let offset=0;
  let displayed=null;

  const body = h("div", { class: "ly-body" });
  const copier=installLyricsCopy(body,()=>current && currentAccount===accountId()?`${currentAccount}:${trackKey(current)}:${token}`:null);
  own(copier.dispose);
  const source = h("div", { class: "ly-source" });
  const currentButton = h("button", { class: "ly-control", type: "button", text: t("ly.current"), hidden: true, onclick: () => snap(lastPosition) });
  const followButton = h("button", { class: "ly-control", type: "button", onclick: () => setAutoFollow(!autoFollow) });
  const copyButton=h("button",{class:"ly-control ly-copy",type:"button",title:t("lyrics.copy"),"aria-label":t("lyrics.copy"),disabled:true,onclick:()=>{
    const track=current,id=accountId(),text=displayed?.text;
    if(track && text)copyLyricsText(text,()=>current===track && accountId()===id);
  }},icon("copy",18));
  const controls = h("div", { class: "ly-controls" }, source, currentButton, followButton,copyButton);
  const offsetText=h("span",{class:"ly-offset"});
  function changeOffset(delta){if(!current)return;try{offset=Math.round(Math.max(-30,Math.min(30,offset+delta))*10)/10;savePersonalLyrics(current,{...personalLyrics(current),offset});offsetText.textContent=`${offset>0?"+":""}${offset.toFixed(1)} с`;snap(lastPosition);}catch(error){toast(errorText(error),{error:true});}}
  const file=h("input",{type:"file",accept:".lrc,text/plain",hidden:true,onchange:async()=>{
    const track=current,id=accountId(),chosen=file.files?.[0];file.value="";if(!track || !chosen)return;
    try{if(chosen.size>256*1024)throw new Error(ft("LRC должен быть не больше 256 КБ"));const lyrics=parseLocalLrc(await chosen.text());if(id!==accountId() || track!==current)return;savePersonalLyrics(track,{...personalLyrics(track),lyrics});show(lyrics);snap(lastPosition);}catch(error){toast(errorText(error),{error:true});}
  }});
  const editControls=h("div",{class:"ly-edit-controls"},
    h("button",{type:"button",class:"ly-control",text:"−0,5 с","aria-label":ft("Показать текст раньше на полсекунды"),onclick:()=>changeOffset(-.5)}),offsetText,
    h("button",{type:"button",class:"ly-control",text:"+0,5 с","aria-label":ft("Показать текст позже на полсекунды"),onclick:()=>changeOffset(.5)}),
    h("button",{type:"button",class:"ly-control",text:ft("Импорт LRC"),onclick:()=>file.click()}),
    h("button",{type:"button",class:"ly-control",text:ft("Версии"),onclick:async event=>{
      if(!current)return;const track=current,id=accountId(),button=event.currentTarget;button.disabled=true;
      try{const versions=await api.lyricsVersions(track);if(track!==current || id!==accountId())return;if(displayed && !versions.some(v=>v.text===displayed.text))versions.unshift(displayed);if(!versions.length)throw new Error(ft("Другие версии не найдены"));const box=button.getBoundingClientRect();openMenu({x:box.left,y:box.top-versions.length*40},versions.map((lyrics,index)=>({label:`${index+1}. ${lyrics.source} · ${lyrics.lines.length?`${lyrics.lines.length} строк с таймингом`:"без тайминга"}`,action:"lyrics-version",run:()=>{if(track!==current || id!==accountId())return;try{savePersonalLyrics(track,{...personalLyrics(track),lyrics});show(lyrics);snap(lastPosition);}catch(error){toast(errorText(error),{error:true});}}})),{label:ft("Версия текста")});}catch(error){toast(errorText(error),{error:true});}finally{button.disabled=false;}
    }}),h("button",{type:"button",class:"ly-control",text:ft("Сбросить"),onclick:()=>{if(!current)return;try{savePersonalLyrics(current,{});offset=0;cache.delete(`${accountId()}:${trackKey(current)}`);forgetSavedLyrics(current);load(current);}catch(error){toast(errorText(error),{error:true});}}}),file);
  const element = h("section", { class: "ly", "aria-label": t("ly.label"), hidden: true }, body, controls,editControls);
  let lastPosition = 0;

  function renderControls() {
    copyButton.disabled=!displayed?.text?.trim();
    followButton.textContent = autoFollow ? t("ly.followOn") : t("ly.followOff");
    followButton.setAttribute("aria-pressed", String(autoFollow));
    currentButton.hidden = autoFollow || lines.length === 0;
  }

  function setAutoFollow(on) {
    autoFollow = on;
    try { localStorage.setItem("lyrics.autoFollow", String(on)); } catch { /* хранилище можно отключить */ }
    renderControls();
    if (on) snap(lastPosition);
  }

  const markHandScroll = () => { handScrollUntil = Date.now() + HAND_SCROLL_MS; };
  body.addEventListener("wheel", markHandScroll, { passive: true });
  body.addEventListener("touchmove", markHandScroll, { passive: true });
  body.addEventListener("pointerdown", markHandScroll);

  function message(text, retry) {
    displayed=null;copier.cancel();
    lines = [];
    active = -1;
    source.textContent = "";
    body.replaceChildren(h("div", { class: "ly-message" }, text,
      retry ? h("button", { class: "np-chip", type: "button", onclick: retry, text: t("ly.retry") }) : null));
    renderControls();
  }

  function show(lyrics) {
    copier.cancel();displayed=lyrics;
    if(current)rememberLyrics(current,lyrics);
    active = -1;
    source.textContent = t("ly.source", { source: lyrics.source });
    if (lyrics.lines.length) {
      lines = lyrics.lines.map((line, index) => ({
        time: line.time,
        node: h("button", {
          type: "button", class: line.text ? "ly-line" : "ly-line gap", "data-i": String(index), title:t("lyrics.copyHint"),
          onclick: () => api.seek(Math.max(0,line.time+offset)),
        }, decodeEntities(line.text) || "♪"),
      }));
      body.replaceChildren(h("div", { class: "ly-pad" }), ...lines.map((l) => l.node), h("div", { class: "ly-pad" }));
      body.classList.remove("plain");
    } else {
      lines = [];
      body.classList.add("plain");
      body.replaceChildren(h("div", { class: "ly-text", text: decodeEntities(lyrics.text) }));
      body.scrollTop = 0;
    }
    renderControls();
  }

  async function load(track) {
    displayed=null;
    const key = `${accountId()}:${trackKey(track)}`;
    const mine = ++token;
    const id=accountId();
    const personal=personalLyrics(track);offset=Number.isFinite(personal.offset)?Math.max(-30,Math.min(30,personal.offset)):0;
    offsetText.textContent=`${offset>0?"+":""}${offset.toFixed(1)} с`;
    if(personal.lyrics?.lines && typeof personal.lyrics.text === "string"){show(personal.lyrics);return;}
    const saved=savedLyrics(track);
    if(saved){show(saved);return;}
    if (cache.has(key)) {
      const cached = cache.get(key);
      if (cached) show(cached); else message(t("ly.none"));
      return;
    }
    message(t("ly.loading"));
    try {
      const lyrics = await api.lyrics(track);
      if (mine !== token || id!==accountId()) return;
      if (cache.size >= CACHE_LIMIT) cache.clear();
      cache.set(key, lyrics ?? null);
      if (lyrics) show(lyrics); else message(t("ly.none"));
    } catch (error) {
      if (mine !== token || id!==accountId()) return;
      const unreachable = ["offline", "timeout", "network"].includes(error && error.kind);
      message(t(unreachable ? "ly.errorNetwork" : "ly.error"), () => load(track));
    }
  }

  /** Индекс исполняемой строки по адресу `position` (-1 перед первой). */
  function indexAt(position) {
    let low = 0;
    let high = lines.length - 1;
    let found = -1;
    while (low <= high) {
      const mid = (low + high) >> 1;
      if (lines[mid].time + offset <= position + LEAD_SECONDS) { found = mid; low = mid + 1; } else high = mid - 1;
    }
    return found;
  }

  function follow(position, { force = false } = {}) {
    lastPosition = position;
    if (!lines.length) return;
    const index = indexAt(position);
    if (index === active && !force) return;
    if (active >= 0) lines[active]?.node.classList.remove("active");
    active = index;
    if (index < 0) return;
    const node = lines[index].node;
    node.classList.add("active");
    if ((!autoFollow || Date.now() < handScrollUntil) && !force) return;
    const top = node.offsetTop - body.clientHeight / 2 + node.offsetHeight / 2;
    body.scrollTo({ top, behavior: !motionOk() || force ? "auto" : "smooth" });
  }

  function snap(position) {
    handScrollUntil = 0;
    follow(position, { force: true });
  }

  return {
    element,
    /** Показывает текст песни `track` (или удаляет его, если его нет). */
    setTrack(track) {
      if (!track) { copier.cancel();token += 1; current = null; message(""); return; }
      if (current && currentAccount === accountId() && trackKey(current) === trackKey(track)) return;
      copier.cancel();current = track;
      currentAccount = accountId();
      load(track);
    },
    setVisible(on) { if(!on)copier.cancel();element.hidden = !on; },
    update(position) { follow(position); },
    /** После поиска или появления панели: прыгайте, не планируя. */
    snap,
    forget() { copier.cancel();current = null; token += 1; },
  };
}

function readAutoFollow() {
  try { return localStorage.getItem("lyrics.autoFollow") !== "false"; }
  catch { return true; }
}
