import { api } from './api.js';
import { accountId } from './personal.js';
import { isAndroid } from './mobile/layout.js';
import { toast } from './toast.js';
import { t, addStrings } from './i18n.js';
addStrings({ru:{'lyrics.copy':'Копировать текст','lyrics.copied':'Текст скопирован','lyrics.copyFailed':'Не удалось скопировать текст','lyrics.copyHint':'Удерживайте строку, чтобы скопировать. Касание перематывает песню.'},en:{'lyrics.copy':'Copy lyrics','lyrics.copied':'Lyrics copied','lyrics.copyFailed':'Could not copy lyrics','lyrics.copyHint':'Hold a line to copy it. Tap to seek in the song.'}});
export async function copyLyricsText(text, isCurrent=()=>true, successKey='lyrics.copied', failureKey='lyrics.copyFailed') {
  if(!isCurrent())return;
  const account=accountId();
  try{
    if(typeof text!=='string' || !text.trim() || text.length>150000 || new TextEncoder().encode(text).length>150000)throw new Error('Invalid clipboard text');
    if(isAndroid())await api.copyText(text);
    else {
      let copied=false;
      if(navigator.clipboard?.writeText){try{await navigator.clipboard.writeText(text);copied=true;}catch{/* use the explicit selection fallback */}}
      if(!copied){
        const previous=document.activeElement, field=document.createElement('textarea');
        field.className='clipboard-copy-proxy';field.value=text;field.readOnly=true;(document.querySelector('dialog[open]')??document.body).append(field);
        try{field.focus({preventScroll:true});field.select();if(!document.execCommand('copy'))throw new Error('Clipboard unavailable');}
        finally{field.remove();previous?.focus?.({preventScroll:true});}
      }
    }
    if(account===accountId() && isCurrent())toast(t(successKey));
  }catch{if(account===accountId() && isCurrent())toast(t(failureKey),{error:true});}
}
// Delegate to the lyric body so line rerenders need no additional listeners.
export function installLyricsCopy(body, identity) {
  let active=null, blocked=null;
  const target=event=>{const node=event.target.closest?.('.ly-line:not(.gap)');return node && body.contains(node)?node:null;};
  function cancel(){if(active)clearTimeout(active.timer);active=null;}
  function valid(op){return !!op.identity && op.node.isConnected && identity()===op.identity;}
  function copy(op){
    if(!valid(op))return;
    blocked=op.node;op.copied=true;
    copyLyricsText(op.node.textContent,()=>valid(op));
  }
  function down(event){
    cancel();blocked=null;
    const node=target(event);if(!node || event.button!==0 || event.isPrimary===false)return;
    const op={node,identity:identity(),x:event.clientX,y:event.clientY,id:event.pointerId,copied:false};active=op;
    op.timer=setTimeout(()=>{if(active===op)copy(op);},550);
  }
  function move(event){if(active && event.pointerId===active.id && Math.hypot(event.clientX-active.x,event.clientY-active.y)>8)cancel();}
  function up(){cancel();}
  function click(event){if(blocked && target(event)===blocked){event.preventDefault();event.stopImmediatePropagation();blocked=null;}}
  function context(event){
    const node=target(event);if(!node)return;event.preventDefault();
    if(node!==blocked)copy({node,identity:identity()});cancel();
  }
  const handlers=[['pointerdown',down],['pointermove',move],['pointerup',up],['pointercancel',up],['pointerleave',up],['contextmenu',context],['click',click,true]];
  for(const [name,handler,capture]of handlers)body.addEventListener(name,handler,capture);
  return {cancel(){cancel();blocked=null;},dispose(){cancel();for(const[name,handler,capture]of handlers)body.removeEventListener(name,handler,capture);}};
}
