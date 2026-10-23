import { h } from './dom.js';
import { t, addStrings } from './i18n.js';
import { findSavedLyrics } from './saved-lyrics.js';
import { accountId } from './personal.js';
import { store, own, subscribe } from './store.js';
import { libraryTracks } from './likes.js';
import { trackKey } from './api.js';
import { icon } from './icons.js';
addStrings({ru:{'search.tab.lyrics':'Тексты','savedLyrics.hint':'Поиск в текстах, которые вы уже открывали. Доступен без сети.','savedLyrics.empty':'Сохранённых текстов с такими словами нет.','savedLyrics.prompt':'Введите хотя бы две буквы из песни, названия или имени исполнителя.','savedLyrics.read':'Открыть текст','savedLyrics.close':'Закрыть','savedLyrics.play':'Слушать «{title}»'},en:{'search.tab.lyrics':'Lyrics','savedLyrics.hint':'Search lyrics you have opened before. Available offline.','savedLyrics.empty':'No saved lyrics match these words.','savedLyrics.prompt':'Enter at least two letters from the lyrics, title or artist.','savedLyrics.read':'Read lyrics','savedLyrics.close':'Close','savedLyrics.play':'Play “{title}”'}});
export function createSavedLyricsView(play) {
  let dialog=null, query='';
  const status=h('p',{class:'saved-lyrics-status',role:'status'});
  const list=h('div',{class:'saved-lyrics-list'});
  const element=h('section',{class:'tab-pane saved-lyrics-pane',hidden:true,'aria-label':t('search.tab.lyrics')},h('p',{class:'saved-lyrics-hint',text:t('savedLyrics.hint')}),status,list);
  function close() {
    if(!dialog)return;
    dialog.close();dialog.remove();dialog=null;
    if(!document.querySelector('dialog[open], .sheet-root:not(.closing)'))document.body.classList.remove('overlay-open');
  }
  function read(entry) {
    close();
    const button=h('button',{class:'btn',type:'button',text:t('savedLyrics.close'),onclick:close});
    dialog=h('dialog',{class:'dialog saved-lyrics-reader','aria-label':entry.track.title},h('div',{class:'dialog-form'},
      h('h2',{text:entry.track.title}),h('p',{class:'saved-lyrics-artist',text:entry.track.artist}),
      h('div',{class:'saved-lyrics-text',text:entry.lyrics.text}),h('p',{class:'saved-lyrics-hint',text:entry.lyrics.source}),
      h('div',{class:'dialog-actions'},button)));
    dialog.addEventListener('cancel',event=>{event.preventDefault();close();});
    document.body.classList.add('overlay-open');document.body.append(dialog);dialog.showModal();button.focus();
  }
  function render(value=query) {
    query=value;
    const id=accountId(), entries=findSavedLyrics(query);
    status.textContent=query.trim().length<2?t('savedLyrics.prompt'):entries.length?'':t('savedLyrics.empty');
    status.hidden=!!entries.length;
    list.replaceChildren(...entries.map(entry=>h('article',{class:'saved-lyrics-card'},
      h('button',{class:'saved-lyrics-play',type:'button','aria-label':t('savedLyrics.play',{title:entry.track.title}),onclick:()=>{
        if(accountId()!==id)return;
        const available=[store.player.track,...store.queue.tracks,...libraryTracks()].filter(Boolean);
        const track=available.find(item=>trackKey(item)===trackKey(entry.track))??entry.track;
        play([track],0,{shuffle:false});
      }},icon('play',18)),h('div',{class:'saved-lyrics-result'},h('strong',{text:entry.track.title}),h('span',{class:'saved-lyrics-artist',text:entry.track.artist}),
        h('button',{type:'button',class:'saved-lyrics-excerpt','aria-label':`${t('savedLyrics.read')}: ${entry.track.title}`,onclick:()=>{if(accountId()===id)read(entry);}},entry.excerpt)))));
  }
  own(close);
  subscribe('session',()=>{close();render();});
  subscribe('view',()=>{if(store.view!=='search')close();});
  subscribe('personal.savedLyrics.v1',()=>render());
  render();
  return {element,render,close};
}
