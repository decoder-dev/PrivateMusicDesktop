import { api } from "./api.js";
import { h } from "./dom.js";
import { t, errorText } from "./i18n.js";
import { accountId } from "./personal.js";
import { own, subscribe, store } from "./store.js";
import "./mobile/strings.js";

export function createCoverEditor({ getPlaylist, isSame, toast, onSaved, onBusy=()=>{} }) {
  let active=null;
  const current=op=>active===op && op.account===accountId() && store.session.signed_in && isSame(getPlaylist(),op.playlist);
  const discard=id=>{if(id)api.discardCover(id).catch(()=>{});};
  function close(op=active, notify=false) {
    if(!op || active!==op)return;
    active=null;discard(op.id);op.dialog?.close();op.dialog?.remove();onBusy(false);
    if(!document.querySelector('dialog[open], .sheet-root:not(.closing)'))document.body.classList.remove('overlay-open');
    if(notify && op.saving)toast(t('cover.stop'));
  }
  async function choose(op) {
    op.picking=true;onBusy(true);op.render?.();
    try {
      const info=await api.chooseCover();
      if(!current(op)){discard(info?.id);return;}
      if(!info){if(!op.id)close(op);return;}
      discard(op.id);op.id=info.id;op.preview=info.preview;op.failed=false;op.message='';
      if(!op.dialog)present(op);
    } catch(error) {
      if(current(op)){toast(t('cover.pickFailed'),{error:true});if(!op.id)close(op);}
    } finally {op.picking=false;if(current(op)){onBusy(false);op.render?.();}}
  }
  function present(op) {
    const image=h('img',{class:'cover-preview',alt:t('cover.preview')});
    const status=h('p',{class:'cover-status',role:'status','aria-live':'polite'});
    const cancel=h('button',{class:'btn',type:'button',text:t('pl.cancel'),onclick:()=>close(op,true)});
    const change=h('button',{class:'btn cover-change',type:'button',text:t('cover.choose'),onclick:()=>choose(op)});
    const save=h('button',{class:'btn primary cover-save',type:'button',text:t('pl.save'),onclick:async()=>{
      if(!current(op) || op.saving || op.picking || !op.id || op.failed)return;
      op.saving=true;op.message=t('cover.saving');op.render();
      try {
        await api.setPlaylistCover(op.id,op.playlist);
        if(!current(op))return;
        const playlist=op.playlist;close(op);toast(t('cover.saved'));
        try {await onSaved?.(playlist);}catch {if(op.account===accountId())toast(t('cover.refreshFailed'),{error:true});}
      } catch(error) {
        if(!current(op))return;
        op.failed=['unconfirmed','unauthorized','cancelled'].includes(error.kind);
        op.message=error.kind==='unconfirmed' ? t('cover.unconfirmed') : errorText(error);
      } finally {op.saving=false;if(current(op))op.render();}
    }});
    const dialog=h('dialog',{class:'dialog cover-dialog','aria-label':t('cover.title')},
      h('div',{class:'dialog-form'},h('h2',{text:t('cover.title')}),image,
        h('p',{class:'cover-hint',text:t('cover.hint')}),change,status,h('div',{class:'dialog-actions'},cancel,save)));
    op.dialog=dialog;
    op.render=()=>{if(image.src!==op.preview)image.src=op.preview;save.disabled=op.picking || op.saving || op.failed;change.disabled=op.picking || op.saving;cancel.textContent=t(op.saving?'cover.cancel':'pl.cancel');status.textContent=op.message;status.hidden=!op.message;};
    dialog.addEventListener('cancel',event=>{event.preventDefault();close(op,true);});
    document.body.classList.add('overlay-open');document.body.append(dialog);op.render();dialog.showModal();cancel.focus();
  }
  own(()=>close());
  subscribe('session',()=>{if(active && !current(active))close();});
  subscribe('view',view=>{if(active && view!=='playlists')close();});
  return {
    open(playlist) {close();if(!playlist || !accountId())return;active={playlist,account:accountId(),id:null,picking:false,saving:false};choose(active);},
    cancel:()=>close(),
  };
}
