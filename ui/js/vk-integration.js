import {api,trackKey} from './api.js';
import {store,subscribe,own,publish} from './store.js';
import {readPersonal,writePersonal,accountId,cleanTrack} from './personal.js';
import {isDownloaded} from './downloads.js';
import {h} from './dom.js';
import {addStrings,t,errorText} from './i18n.js';
import {toast} from './toast.js';
addStrings({ru:{'vk.integration':'Интеграция VK','vk.broadcast':'Транслировать музыку в статус','vk.reporting':'Отправлять VK историю прослушивания','vk.note':'Обе функции выключены по умолчанию и включаются отдельно для каждого аккаунта. Передача истории помогает рекомендациям VK.','vk.retry':'Повторить подключение'},en:{'vk.integration':'VK integration','vk.broadcast':'Broadcast music to profile status','vk.reporting':'Send listening history to VK','vk.note':'Both options are off by default, separately for each account. Listening reports help VK recommendations.','vk.retry':'Retry integration'}});
const preferences=()=>{const p=readPersonal('vkIntegration',{});return {broadcast:p.broadcast===true,reporting:p.reporting===true};};
let errors={};
export function createVkIntegrationOptions(){
 const broadcast=h('input',{type:'checkbox',role:'switch','aria-label':t('vk.broadcast'),onchange:()=>save({broadcast:broadcast.checked})});
 const reporting=h('input',{type:'checkbox',role:'switch','aria-label':t('vk.reporting'),onchange:()=>save({reporting:reporting.checked})});
 const error=h('p',{class:'set-note',role:'status'});
 const retry=h('button',{type:'button',class:'btn',text:t('vk.retry'),onclick:()=>publish('vkIntegrationRetry')});
 function save(patch){try{writePersonal('vkIntegration',{...preferences(),...patch});}catch(e){toast(errorText(e),{error:true});}render();}
 function render(){const p=preferences();broadcast.checked=p.broadcast;reporting.checked=p.reporting;error.textContent=Object.values(errors).filter(Boolean).join(' · ');error.hidden=!error.textContent;retry.hidden=!error.textContent;}
 subscribe('vkIntegration',render);subscribe('session',render);subscribe('personal.vkIntegration',render);render();
 const row=(name,control)=>h(control.matches?.('input[type=checkbox]')?'label':'div',{class:'set-row'},h('div',{class:'set-label'},h('div',{class:'set-text',text:t(name)})),control);
 return h('section',{class:'set-section'},h('h2',{text:t('vk.integration')}),h('div',{class:'set-card'},row('vk.broadcast',broadcast),row('vk.reporting',reporting),h('p',{class:'set-note offline-note',text:t('vk.note')}),error,retry));
}
export function installVkIntegration(){
 let disposed=false,worker=false,commands=[],active=null,lastBroadcast=null,wasEnabled=false;
 const account=()=>accountId();let owner=account();
 errors={};
 function event(listening,name){const player=store.player;return {e:name,audio_id:trackKey(listening.track),uuid:listening.uuid,start_time:listening.started,playback_started_at:listening.started,duration:name==='music_stop_playback'?Math.floor(Math.max(0,Math.min(listening.track.duration,(Date.now()-listening.clock)/1000))):0,track_code:listening.track.track_code||'',streaming_type:isDownloaded(listening.track)?'offline':'online',shuffle:player.shuffle,repeat_mode:player.repeat,reason:'user',state:'app'};}
 async function drain(){
  if(worker || disposed)return;worker=true;
  try{while(commands.length && !disposed){const command=commands.shift();if(command.owner!==account() || errors[command.kind])continue;if(command.kind==='reporting' && !preferences().reporting)continue;try{if(command.kind==='broadcast')await api.vkBroadcast(command.owner,command.track);else await api.vkReportPlayback(command.owner,command.events);}catch(e){if(!disposed && command.owner===account()){errors[command.kind]=errorText(e);if(command.kind==='reporting')active=null;publish('vkIntegration');}}}}
  finally{worker=false;}
 }
 function update(){
  if(disposed)return;
  if(owner!==account()){owner=account();commands=[];active=null;lastBroadcast=null;wasEnabled=false;errors={};publish('vkIntegration');}
  if(!owner)return;
  const p=preferences(),player=store.player;
  if(!p.reporting){active=null;commands=commands.filter(c=>c.kind!=='reporting');errors.reporting=null;}
  const playing=player.status==='playing',track=player.track;
  const desired=p.broadcast && playing && track?trackKey(track):null;
  if(!errors.broadcast && (desired!==lastBroadcast || wasEnabled && !p.broadcast)){
   commands=commands.filter(c=>c.kind!=='broadcast');commands.push({kind:'broadcast',owner,track:desired?cleanTrack(track):null});lastBroadcast=desired;
  }
  wasEnabled=p.broadcast;
  if(p.reporting && !errors.reporting){
   if(active && (!playing || !track || trackKey(active.track)!==trackKey(track))){commands.push({kind:'reporting',owner,events:[event(active,'music_stop_playback')]});active=null;}
   if(!active && playing && track){active={track:cleanTrack(track),uuid:crypto.getRandomValues(new Uint32Array(1))[0]%2147483646+1,started:Math.floor(Date.now()/1000),clock:Date.now()};commands.push({kind:'reporting',owner,events:[event(active,'music_start_playback')]});}
  }
  commands=commands.slice(-32);drain();
 }
 subscribe('player',update);subscribe('session',update);
 subscribe('personal.vkIntegration',()=>{errors={};update();publish('vkIntegration');});
 subscribe('vkIntegrationRetry',()=>{errors={};lastBroadcast=null;wasEnabled=true;update();publish('vkIntegration');});
 own(()=>{disposed=true;commands=[];active=null;errors={};});update();
}
