import {h} from '../dom.js';
import {t,addStrings,errorText} from '../i18n.js';
import {api,trackKey} from '../api.js';
import {store,own} from '../store.js';
import {createTracksScreen} from '../tracks-screen.js';
import {confirmDialog} from '../dialog.js';
addStrings({ru:{'local.title':'На устройстве','local.import':'Импортировать музыку','local.description':'MP3, M4A, AAC, WAV и FLAC из ваших файлов. Копии хранятся на устройстве и не загружаются в VK.','local.empty':'Здесь появится импортированная музыка','local.working':'Импорт музыки…','local.failed':'Не удалось импортировать: {names}. Проверьте формат и размер файла (до 150 МБ).','local.skipped':'За один раз можно импортировать 100 файлов. Осталось выбрать повторно: {count}.','local.remove':'Удалить копию с устройства? Исходный файл сохранится.','local.removeAction':'Удалить с устройства'},en:{'local.title':'On this device','local.import':'Import music','local.description':'MP3, M4A, AAC, WAV and FLAC from your files. Copies stay on this device and are not uploaded to VK.','local.empty':'Imported music will appear here','local.working':'Importing music…','local.failed':'Could not import: {names}. Check the file format and size (up to 150 MB).','local.skipped':'Import up to 100 files at a time. Select the remaining {count} files again.','local.remove':'Remove the copy from this device? The original file will be kept.','local.removeAction':'Remove from device'}});
export function createLocalAudioView({play,onTrackAction,openTrackMenu,toast}) {
  let disposed=false,request=0,busy=false;
  own(()=>{disposed=true;request++;});
  const status=h('p',{class:'import-status',role:'status','aria-live':'polite'});
  const choose=h('button',{type:'button',class:'btn primary',text:t('local.import'),onclick:importFiles});
  const remove=async tracks=>{
    if(!tracks.length || !await confirmDialog({text:t('local.remove'),confirm:t('local.removeAction'),danger:true}))return;
    try {
      const keys=new Set(tracks.map(trackKey));
      const queue=store.queue.tracks.slice();
      for(let i=queue.length-1;i>=0;i--)if(keys.has(trackKey(queue[i])))await api.removeFromQueue(i);
      for(const track of tracks)await api.removeDownload(trackKey(track));
      await load();
    } catch(error){if(!disposed)toast(errorText(error),{error:true});}
  };
  const screen=createTracksScreen({title:t('local.title'),filterable:true,hooks:{
    emptyText:t('local.empty'),art:'note',onPlay:play,pinLiked:false,batchActions:['queue','remove'],
    rowActions:[],downloadable:false,
    onAction:(action,track,index)=>action==='like'?remove([track]):onTrackAction(action,track,index),
    onMenu:(track,at)=>openTrackMenu(track,at,[{label:t('local.removeAction'),action:'remove-local',icon:'trash',run:()=>remove([track])}]),
    onBatchRemove:remove,
  }});
  screen.element.classList.add('local-audio-view');
  screen.header.after(h('div',{class:'import-body local-audio-intro'},h('p',{class:'import-note',text:t('local.description')}),choose,status));
  async function load(){
    const mine=++request;
    try {const records=await api.localAudio();if(!disposed && mine===request)screen.setTracks(records.map(record=>record.track));}
    catch(error){if(!disposed && mine===request)status.textContent=errorText(error);}
  }
  async function importFiles(){
    if(busy)return;
    busy=true;choose.disabled=true;status.textContent=t('local.working');
    try {
      const result=await api.importLocalAudio();
      if(disposed)return;
      await load();
      status.textContent=[result.failed?.length?t('local.failed',{names:result.failed.slice(0,8).join(', ')}):'',result.skipped?t('local.skipped',{count:result.skipped}):''].filter(Boolean).join(' ');
    }catch(error){if(!disposed)status.textContent=errorText(error);}
    finally{busy=false;if(!disposed)choose.disabled=false;}
  }
  return {element:screen.element,show:load};
}
