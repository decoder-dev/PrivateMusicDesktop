import { api, trackKey } from './api.js';
import { h, formatTime } from './dom.js';
import { t, addStrings, errorText } from './i18n.js';
import { accountId } from './personal.js';
import { store, own, subscribe } from './store.js';
import { formatSize, refreshDownloads } from './downloads.js';
import { isAndroid } from './mobile/layout.js';
addStrings({ru:{'file.details':'Об аудиофайле','file.available':'Доступен без сети','file.missing':'Файл недоступен. Его можно скачать заново из списка треков.','file.format':'Формат','file.size':'Размер','file.saved':'Сохранён','file.duration':'Длительность','file.close':'Закрыть','file.remove':'Удалить с устройства','file.confirm':'Удалить этот аудиофайл с устройства? Музыка в VK сохранится.','file.removed':'Файл удалён с устройства','file.cancel':'Отмена'},en:{'file.details':'Audio file details','file.available':'Available offline','file.missing':'The file is unavailable. You can download it again from the track list.','file.format':'Format','file.size':'Size','file.saved':'Saved','file.duration':'Duration','file.close':'Close','file.remove':'Remove from device','file.confirm':'Remove this audio file from the device? Your VK library will be kept.','file.removed':'File removed from device','file.cancel':'Cancel'}});
export function createDownloadDetails({play,toast}) {
  let active=null;
  const current=op=>active===op && op.account===accountId() && store.session.signed_in;
  function close(){
    const op=active;active=null;op?.dialog?.close();op?.dialog?.remove();
    if(!document.querySelector('dialog[open], .sheet-root:not(.closing)'))document.body.classList.remove('overlay-open');
  }
  async function check(op){
    const info=await api.downloadDetails(trackKey(op.track));
    if(!current(op))return false;
    op.info=info;op.loading=false;op.render();return info?.available===true;
  }
  async function act(op,fn){
    if(!current(op) || op.busy)return;
    op.busy=true;op.render();
    try{await fn();}catch(error){if(current(op))toast(errorText(error),{error:true});}
    finally{op.busy=false;if(current(op))op.render();}
  }
  function open(track){
    close();if(!accountId())return;
    const op={track,account:accountId(),info:null,busy:false,loading:true};active=op;
    const status=h('p',{class:'file-status',role:'status'});
    const facts=h('dl',{class:'file-facts'});
    const playButton=h('button',{class:'btn primary file-play',type:'button',text:t('play'),onclick:()=>act(op,async()=>{if(await check(op)){play([track],0,{shuffle:false});close();}})});
    const share=h('button',{class:'btn file-share',type:'button',text:t('share.audio'),hidden:!isAndroid(),onclick:()=>act(op,async()=>{
      if(!await check(op))return;
      const opened=await api.shareAudio(track,false);
      if(current(op) && !opened){op.info.available=false;op.render();}
    })});
    const remove=h('button',{class:'btn danger-text file-remove',type:'button',text:t('file.remove'),onclick:()=>{op.confirming=true;op.render();}});
    const confirmText=h('p',{class:'file-confirm-text',text:t('file.confirm'),role:'alert',hidden:true});
    const cancelRemove=h('button',{class:'btn file-cancel-remove',type:'button',text:t('file.cancel'),hidden:true,onclick:()=>{op.confirming=false;op.render();}});
    const confirmRemove=h('button',{class:'btn danger file-confirm-remove',type:'button',text:t('file.remove'),hidden:true,onclick:()=>act(op,async()=>{
      if(!op.confirming || !current(op))return;
      await api.removeDownload(trackKey(track));
      if(!current(op))return;
      close();toast(t('file.removed'));await refreshDownloads();
    })});
    const done=h('button',{class:'btn',type:'button',text:t('file.close'),onclick:close});
    op.dialog=h('dialog',{class:'dialog file-details-dialog','aria-label':t('file.details')},h('div',{class:'dialog-form'},
      h('h2',{text:track.title}),h('p',{class:'file-artist',text:track.artist}),status,facts,
      confirmText,h('div',{class:'file-actions'},playButton,share,remove,confirmRemove,cancelRemove),h('div',{class:'dialog-actions'},done)));
    op.render=()=>{
      status.textContent=op.loading?t('loading'):t(op.info?.available?'file.available':'file.missing');
      status.dataset.available=String(op.info?.available===true);
      playButton.disabled=share.disabled=op.loading || op.busy || !op.info?.available;
      remove.disabled=confirmRemove.disabled=cancelRemove.disabled=op.loading || op.busy;
      playButton.hidden=facts.hidden=!!op.confirming;share.hidden=!isAndroid() || !!op.confirming;
      remove.hidden=!!op.confirming;confirmRemove.hidden=cancelRemove.hidden=confirmText.hidden=!op.confirming;
      const info=op.info;
      const saved=info?.added>0?new Date(info.added*1000):null;
      const date=saved && Number.isFinite(saved.getTime())?saved.toLocaleString():null;
      facts.replaceChildren(...(info?[[t('file.format'),info.format],[t('file.duration'),formatTime(track.duration)],[t('file.size'),formatSize(info.bytes)],...(date?[[t('file.saved'),date]]:[])]:[]).map(([label,value])=>h('div',{},h('dt',{text:label}),h('dd',{text:value}))));
    };
    op.dialog.addEventListener('cancel',event=>{event.preventDefault();close();});
    document.body.classList.add('overlay-open');document.body.append(op.dialog);op.render();op.dialog.showModal();done.focus();
    check(op).catch(error=>{if(current(op)){op.loading=false;op.render();toast(errorText(error),{error:true});}});
  }
  own(close);subscribe('session',()=>{if(active && !current(active))close();});subscribe('view',close);
  return {open,close};
}
