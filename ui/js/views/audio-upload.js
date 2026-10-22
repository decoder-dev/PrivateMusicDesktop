import { h } from "../dom.js";
import { api } from "../api.js";
import { t, errorText } from "../i18n.js";
import { store, subscribe, own } from "../store.js";
import { accountId } from "../personal.js";
import { formatSize } from "../downloads.js";
import "../mobile/strings.js";

export function createAudioUploadView({ reloadLibrary }) {
  let file=null, busy=false, token=0, owner=accountId();
  const artist=h("input",{class:"field",maxlength:200,"aria-label":t("upload.artist"),oninput:sync});
  const title=h("input",{class:"field",maxlength:200,"aria-label":t("upload.track"),oninput:sync});
  const status=h("p",{class:"import-status",role:"status","aria-live":"polite"});
  const filename=h("strong",{class:"upload-filename",text:t("upload.noFile")});
  const size=h("span",{class:"import-note"});
  const choose=h("button",{class:"btn",type:"button",text:t("upload.choose"),onclick:pick});
  const send=h("button",{class:"btn primary",type:"button",text:t("upload.submit"),onclick:upload});
  const cancel=h("button",{class:"btn",type:"button",text:t("upload.cancel"),hidden:true,onclick:()=>reset(t("upload.cancelled"))});
  const element=h("section",{class:"view audio-upload-view",hidden:true},
    h("header",{class:"view-header"},h("div",{class:"vh-text"},h("h1",{text:t("upload.title")}))),
    h("div",{class:"import-body upload-body"},h("p",{class:"import-note",text:t("upload.hint")}),
      h("div",{class:"upload-file"},filename,size,choose),
      h("label",{},t("upload.artist"),artist),h("label",{},t("upload.track"),title),status,send,cancel));
  const alive=(mine,account)=>mine===token && account===accountId() && store.session.signed_in;
  const discard=id=>id && api.discardAudio(id).catch(()=>{});
  function sync() {
    const ready=!!accountId() && store.session.signed_in;
    artist.disabled=title.disabled=choose.disabled=busy || !ready;
    send.disabled=busy || !ready || !file || !artist.value.trim() || !title.value.trim();
    cancel.hidden=!busy;
    filename.textContent=file?.name??t("upload.noFile");size.textContent=file?formatSize(file.bytes):"";
  }
  function reset(message="") {
    token++;discard(file?.id);file=null;busy=false;
    artist.value="";title.value="";status.textContent=message;sync();
  }
  async function pick() {
    if(busy) return;
    const mine=++token, account=accountId();busy=true;status.textContent=t("upload.choosing");sync();
    try {
      const selected=await api.chooseAudio();
      if(!alive(mine,account)) {discard(selected?.id);return;}
      if(selected) {file=selected;if(!title.value.trim())title.value=[...selected.name.replace(/\.mp3$/i,"")].slice(0,200).join("");}
      status.textContent="";
    } catch(error) {if(alive(mine,account))status.textContent=error?.kind==="invalid"?t("upload.fileError"):errorText(error);}
    finally {if(alive(mine,account)){busy=false;sync();}}
  }
  async function upload() {
    if(busy || !file || !artist.value.trim() || !title.value.trim()) return;
    const mine=++token, account=accountId(), selected=file;
    busy=true;status.textContent=t("upload.sending");sync();
    try {
      await api.uploadAudio(selected.id,artist.value.trim(),title.value.trim());
      if(account===accountId() && store.session.signed_in) reloadLibrary?.();
      if(!alive(mine,account))return;
      file=null;status.textContent=t("upload.done");
    } catch(error) {
      if(!alive(mine,account))return;
      status.textContent=error?.kind==="unconfirmed"?t("upload.uncertain"):errorText(error);
      if(["unconfirmed","unauthorized","invalid"].includes(error?.kind)) {discard(selected.id);file=null;}
    } finally {if(alive(mine,account)){busy=false;sync();}}
  }
  subscribe("view",id=>{if(id!=="audioUpload")reset();});
  subscribe("session",()=>{if(owner!==accountId()){owner=accountId();reset();}else sync();});
  own(()=>{token++;discard(file?.id);});
  sync();
  return {element,show(){sync();}};
}
