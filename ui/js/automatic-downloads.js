import {api,trackKey,loadAllPages} from './api.js';
import {store,subscribe,own,publish} from './store.js';
import {accountId,readPersonal,writePersonal,cleanTrack} from './personal.js';
import {downloads,refreshDownloads,downloadAll,downloadJob,isDownloaded} from './downloads.js';
import {downloadPermission} from './download-permission.js';
import {sharedProtection} from './playlist-sync.js';
import {onLibraryChange} from './likes.js';
import {h} from './dom.js';
import {addStrings,t,errorText} from './i18n.js';
import {toast} from './toast.js';

addStrings({ru:{'offline.auto':'Автоматические загрузки','offline.likes':'Скачивать избранное','offline.cache':'Сохранять прослушанные треки','offline.limit':'Лимит офлайн-хранилища','offline.note':'Избранное скачивается в сети без тарификации. Прослушанные треки сохраняются без дополнительного трафика. При нехватке места удаляется только автоматический кэш; ручные загрузки и закреплённые плейлисты сохраняются.','offline.gb':'ГБ'},en:{'offline.auto':'Automatic downloads','offline.likes':'Download liked tracks','offline.cache':'Cache played tracks','offline.limit':'Offline storage limit','offline.note':'Liked tracks use unmetered connections. Played tracks are saved without additional data usage. Only automatic cache files are evicted; manual downloads and pinned playlists are preserved.','offline.gb':'GB'}});
const LIMITS=[1,2,4,8,16,32];
export function offlinePreferences(){const p=readPersonal('offlinePreferences',{});return {likes:p.likes===true,cache:p.cache===true,limit:LIMITS.includes(p.limit)?p.limit:1};}
export function createOfflineOptions(){
  const likes=h('input',{type:'checkbox',role:'switch','aria-label':t('offline.likes'),onchange:()=>save({likes:likes.checked})});
  const cache=h('input',{type:'checkbox',role:'switch','aria-label':t('offline.cache'),onchange:()=>save({cache:cache.checked})});
  const limit=h('select',{class:'field','aria-label':t('offline.limit'),onchange:()=>save({limit:Number(limit.value)})},LIMITS.map(value=>h('option',{value,text:`${value} ${t('offline.gb')}`})));
  function save(patch){try{writePersonal('offlinePreferences',{...offlinePreferences(),...patch});}catch(error){toast(errorText(error),{error:true});}render();}
  function render(){const p=offlinePreferences();likes.checked=p.likes;cache.checked=p.cache;limit.value=String(p.limit);}
  subscribe('personal.offlinePreferences',render);subscribe('session',render);render();
  const row=(label,control)=>h(control.matches?.('input[type=checkbox]')?'label':'div',{class:'set-row'},h('div',{class:'set-label'},h('div',{class:'set-text',text:label})),control);
  return h('section',{class:'set-section'},h('h2',{text:t('offline.auto')}),h('div',{class:'set-card'},row(t('offline.likes'),likes),row(t('offline.cache'),cache),row(t('offline.limit'),limit),h('p',{class:'set-note offline-note',text:t('offline.note')})));
}

// A cache entry grants eviction rights only for files this account created.
// Every other account's manual/pinned/liked downloads protect the shared file.
const cacheEntries=()=>readPersonal('automaticCache',[]).filter(e=>e && /^-?\d+_\d+$/.test(e.id) && Number.isFinite(e.used));
export function installAutomaticDownloads(){
  let disposed=false,running=false,likesDirty=true,scheduled=0,lastTrack=null;
  const queue=new Map();
  let current=accountId();
  const valid=()=>!disposed && !!current && current===accountId() && store.session.signed_in;
  function schedule(delay=500){clearTimeout(scheduled);if(valid())scheduled=setTimeout(()=>run().catch(()=>{}),delay);}
  async function prune(){
    await refreshDownloads();if(!valid())return;
    const limit=offlinePreferences().limit*1024**3;
    const protection=sharedProtection();if(!protection.complete)return;
    const entries=cacheEntries().sort((a,b)=>a.used-b.used);
    for(const entry of entries){
      if(!valid() || downloads().bytes<=limit)break;
      const now=sharedProtection();
      if(!now.complete || now.manual.has(entry.id) || now.protectedKeys.has(entry.id) || trackKey(store.player.track ?? {})===entry.id)continue;
      await api.removeDownload(entry.id);if(!valid())return;
      writePersonal('automaticCache',cacheEntries().filter(e=>e.id!==entry.id));
      await refreshDownloads();
    }
  }
  async function run(){
    if(!valid() || running)return;
    const preferences=offlinePreferences();if(!preferences.likes && !preferences.cache){queue.clear();return;}
    if(downloadJob()?.running){schedule(3000);return;}
    running=true;
    try{
      const allowed=downloadPermission({interactive:false,isCancelled:()=>!valid()});
      const unmetered=preferences.likes ? await allowed().catch(()=>false) : false;
      await prune();if(!valid())return;
      if(preferences.likes && unmetered && likesDirty){
        const items=await loadAllPages((offset,count)=>api.library(offset,count),{isCancelled:()=>!valid() || !offlinePreferences().likes});
        if(!items || !valid())return;
        writePersonal('autoLikeDownloads',items.map(trackKey));likesDirty=false;
        for(const track of items)queue.set(trackKey(track),{track:cleanTrack(track),kind:'likes'});
      }
      for(const [key,item] of queue){
        if(!valid() || downloadJob()?.running)break;
        const p=offlinePreferences();
        if(!p[item.kind==='likes'?'likes':'cache']){queue.delete(key);continue;}
        if(isDownloaded(item.track)){queue.delete(key);continue;}
        if(item.kind==='cache'){
          const created=await api.cachePlaying(item.track,current,p.limit*1024**3);
          if(!valid())return;
          if(created){writePersonal('automaticCache',[...cacheEntries().filter(e=>e.id!==key),{id:key,used:Date.now()}].slice(-20000));queue.delete(key);await refreshDownloads();}
          continue;
        }
        if(!unmetered)continue;
        // Reserve a conservative 320 kbps allowance before starting a file.
        const estimate=Math.max(1_000_000,Number(item.track.duration||300)*40_000);
        if(downloads().bytes+estimate>p.limit*1024**3)continue;
        const result=await downloadAll([item.track],{managed:true,isCancelled:()=>!valid() || !offlinePreferences()[item.kind==='likes'?'likes':'cache']});
        if(!valid())return;
        if(!result || result.cancelled)break;
        queue.delete(key);

      }
    }finally{running=false;publish('automaticDownloads');}
  }
  subscribe('session',()=>{if(current==null)current=accountId();schedule();});
  subscribe('player',player=>{
    const key=player.track?trackKey(player.track):null;
    if(player.status!=='playing' || !key || key===lastTrack)return;
    if(lastTrack && queue.get(lastTrack)?.kind==='cache')queue.delete(lastTrack);
    lastTrack=key;
    const entries=cacheEntries();
    if(entries.some(e=>e.id===key))writePersonal('automaticCache',entries.map(e=>e.id===key?{...e,used:Date.now()}:e));
    if(offlinePreferences().cache){queue.set(key,{track:cleanTrack(player.track),kind:'cache'});schedule(10000);}
  });
  onLibraryChange(()=>{likesDirty=true;schedule();});
  subscribe('personal.offlinePreferences',()=>{likesDirty=true;const track=store.player.track;if(offlinePreferences().cache && store.player.status==='playing' && track)queue.set(trackKey(track),{track:cleanTrack(track),kind:'cache'});schedule();});
  const online=()=>schedule();window.addEventListener('online',online);
  const timer=setInterval(()=>schedule(),60000);
  own(()=>{disposed=true;clearTimeout(scheduled);clearInterval(timer);window.removeEventListener('online',online);queue.clear();});
  schedule();
}
