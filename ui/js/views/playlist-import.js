import { h, formatTime } from "../dom.js";
import { api, trackKey } from "../api.js";
import { t, errorText } from "../i18n.js";
import { accountId } from "../personal.js";
import { store, subscribe, own } from "../store.js";
import { addPlaylist, changeCount } from "../my-playlists.js";
import { parseImport, bestImportMatch } from "../playlist-import.js";
import "../mobile/strings.js";

export function createPlaylistImportView({ push }) {
  let stage = "input", rows = [], run = 0, stopped = false, playlist = null, owner = accountId();
  let added = 0, failed = 0;
  const status = h("p", {class:"import-status", role:"status", "aria-live":"polite"});
  const input = h("textarea", {class:"field import-text", maxlength:1048576, "aria-label":t("import.text"), placeholder:t("import.hint"), oninput: updateInput});
  const name = h("input", {class:"field", maxlength:128, "aria-label":t("import.name"), value:t("import.default"), oninput:updateInput});
  const file = h("input", {type:"file", accept:".csv,.txt,text/plain,text/csv", hidden:true, onchange: readFile});
  const fileButton = h("button", {class:"btn", type:"button", text:t("import.file"), onclick:()=>file.click()});
  const count = h("p", {class:"import-note"});
  const match = h("button", {class:"btn primary", type:"button", text:t("import.match"), onclick:matchTracks});
  const form = h("div", {class:"import-input"}, h("label", {}, t("import.name"), name),
    h("label", {}, t("import.text"), input), file, fileButton, count, match);
  const reviewRows = h("div", {class:"import-rows"});
  const create = h("button", {class:"btn primary", type:"button", text:t("import.create"), onclick:createPlaylist});
  const retry = h("button", {class:"btn", type:"button", text:t("import.matchAgain"), onclick:matchTracks});
  const review = h("div", {class:"import-review", hidden:true}, h("p", {class:"import-note", text:t("import.review")}), reviewRows,
    h("div", {class:"import-actions"}, create, retry));
  const cancel = h("button", {class:"btn", type:"button", text:t("import.stop"), hidden:true, onclick:()=>{
    stopped = true;
    if (stage === "matching") { run++; stage = "input"; status.textContent=t("import.cancelled"); render(); }
    else { cancel.disabled=true; status.textContent=t("import.stopping"); }
  }});
  const open = h("button", {class:"btn primary", type:"button", text:t("import.open"), hidden:true, onclick:()=>playlist && push("playlists",playlist)});
  const again = h("button", {class:"btn", type:"button", text:t("import.again"), hidden:true, onclick:()=>{stage="input";status.textContent="";render();}});
  const introduction = h("p", {class:"import-note", text:t("import.explanation")});
  const element = h("section", {class:"view playlist-import-view", hidden:true},
    h("header", {class:"view-header"}, h("div", {class:"vh-text"}, h("h1", {text:t("import.title")}))),
    h("div", {class:"import-body"}, introduction, form, review, status, cancel, open, again));
  const alive = (token, account) => token === run && account === accountId() && store.session.signed_in;
  function updateInput() { const items=parseImport(input.value); count.textContent=t("import.count", {count:items.length}); match.disabled=!name.value.trim() || !items.length; }
  function render() {
    introduction.hidden=stage!=="input";
    form.hidden=stage!=="input"; review.hidden=stage!=="review";
    cancel.hidden=!["matching","creating"].includes(stage); cancel.disabled=false;
    open.hidden=stage!=="done" || !playlist; again.hidden=stage!=="done";
    updateInput();
  }
  async function readFile() {
    const selected=file.files?.[0], token=run, account=accountId(); file.value="";
    if (!selected) return;
    if (selected.size>1048576) { status.textContent=t("import.fileLimit"); return; }
    try { const text=await selected.text(); if(alive(token,account) && stage==="input") { input.value=text;updateInput(); } }
    catch(error) { if(alive(token,account)) status.textContent=errorText(error); }
  }
  async function matchTracks() {
    const items=parseImport(input.value), account=accountId();
    if(!items.length || !account || !name.value.trim()) return;
    const token=++run; stopped=false; rows=[]; stage="matching"; render();
    let message="";
    for(let i=0;i<items.length;i++) {
      if(!alive(token,account) || stopped) return;
      status.textContent=t("import.searching",{done:i+1,total:items.length});
      try {
        const page=await api.search(`${items[i].artist} ${items[i].title}`.trim(),0,10);
        if(!alive(token,account) || stopped) return;
        const candidates=[...new Map((page.items??[]).map(track=>[trackKey(track),track])).values()];
        const best=bestImportMatch(items[i],candidates);
        rows.push({item:items[i],candidates,track:best?.track??null,include:(best?.confidence??0)>=0.9});
      } catch(error) { message=errorText(error); break; }
    }
    if(!alive(token,account)) return;
    stage="review"; status.textContent=message; renderRows(); render();
  }
  function renderRows() {
    reviewRows.replaceChildren(...rows.map(row=>{
      const check=h("input",{type:"checkbox",checked:row.include,"aria-label":`${t("import.include")}: ${row.item.title}`,onchange:event=>{row.include=event.target.checked;updateCreate();}});
      check.disabled=!row.track;
      const select=h("select",{class:"field","aria-label":`${t("import.matchFor")}: ${row.item.title}`,onchange:event=>{
        row.track=event.target.value === "" ? null : row.candidates[Number(event.target.value)]??null; row.include=!!row.track;
        check.disabled=!row.track;check.checked=row.include;updateCreate();
      }},h("option",{value:"",text:t("import.noMatch")}),row.candidates.map((track,index)=>h("option",{value:index,text:`${track.artist} — ${track.title} · ${formatTime(track.duration)}`})));
      select.value=row.track?String(row.candidates.indexOf(row.track)):"";
      return h("div",{class:"import-row"},h("label",{class:"import-choice"},check,h("span",{text:`${row.item.artist?`${row.item.artist} — `:""}${row.item.title}`})),select);
    }));
    updateCreate();
  }
  function updateCreate() { create.disabled=!rows.some(row=>row.include&&row.track); }
  async function createPlaylist() {
    if(stage!=="review") return;
    const tracks=[...new Map(rows.filter(row=>row.include&&row.track).map(row=>[trackKey(row.track),row.track])).values()];
    const account=accountId(), token=++run;
    if(!tracks.length || !account) return;
    let failure = "";
    stage="creating";stopped=false;added=0;failed=0;playlist=null;render();status.textContent=t("import.creating",{done:0,total:tracks.length});
    try {
      const created=await api.createPlaylist(name.value.trim(),"");
      if(!alive(token,account)) return;
      playlist=created;addPlaylist(created);
      for(const track of tracks) {
        if(stopped || !alive(token,account)) break;
        try { await api.addToPlaylist(created,track); }
        catch(error) { if(alive(token,account)) { failed++;failure=errorText(error);status.textContent=failure; } stopped=true;break; }
        if(!alive(token,account)) return;
        added++;changeCount(created,1);
        status.textContent=t("import.creating",{done:added,total:tracks.length});
      }
      if(!alive(token,account)) return;
      playlist={...created,count:added};
      stage="done";status.textContent=t("import.done",{added,failed,remaining:tracks.length-added-failed})+(failure?` ${failure}`:"");render();
    } catch(error) { if(alive(token,account)) { stage="review";status.textContent=errorText(error);render(); } }
  }
  subscribe("view",id=>{
    if(id==="playlistImport") return;
    if(stage==="matching") { run++;stage="input";status.textContent="";render(); }
    else if(stage==="creating") stopped=true;
  });
  subscribe("session",()=>{
    if(owner===accountId()) return;
    owner=accountId();run++;stopped=true;stage="input";rows=[];playlist=null;input.value="";name.value=t("import.default");status.textContent="";reviewRows.replaceChildren();render();
  });
  own(()=>{run++;stopped=true;});
  render();
  return {element,show() {render();}};
}
