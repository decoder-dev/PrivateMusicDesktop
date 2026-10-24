import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';
const config={signedIn:true,platform:'android',catalogMixes:true};
async function settings(page){await page.locator('.dock-tab[data-tab="profile"]').click();await page.locator('[data-row="settings"]').click();await page.locator('.settings .eq').waitFor();}
test('iPhone audio options: native processing, full presets, haptics and recovery survive restart',async()=>{
 const app=await openApp({config,phone:'small'});
 try{
  const {page}=app;await settings(page);
  assert.equal(await page.locator('.eq select option').count(),25);
  const preamp=page.getByRole('slider',{name:'Предусиление',exact:true});
  await preamp.fill('-4.5');await preamp.dispatchEvent('change');
  await page.getByRole('switch',{name:'Сжатие динамического диапазона',exact:true}).check();
  await page.getByRole('switch',{name:'Расширение стерео',exact:true}).check();
  const width=page.getByRole('slider',{name:'Ширина стерео',exact:true});await width.fill('0.7');await width.dispatchEvent('change');
  await page.getByRole('switch',{name:'Тактильный отклик',exact:true}).uncheck();
  await page.getByRole('switch',{name:'Пропускать недоступные треки',exact:true}).uncheck();
  await page.waitForFunction(()=>window.__mock.state.audioProcessing?.spatialIntensity===0.7);
  assert.deepEqual(await page.evaluate(()=>window.__mock.state.audioProcessing),{preamp:-4.5,compression:true,spatial:true,spatialIntensity:0.7});
  await page.reload();await settings(page);
  assert.equal(await preamp.inputValue(),'-4.5');assert.equal(await width.inputValue(),'0.7');
  assert.equal(await page.getByRole('switch',{name:'Сжатие динамического диапазона',exact:true}).isChecked(),true);
  assert.equal(await page.evaluate(()=>document.documentElement.dataset.haptics),'off');
  assert.equal(await page.evaluate(()=>window.__mock.state.advanceOnError),false);
  const boxes=await page.locator('.audio-options .set-row').evaluateAll(rows=>rows.map(row=>{const label=row.querySelector('.set-label').getBoundingClientRect(),control=row.lastElementChild.getBoundingClientRect();return {overlap:Math.min(label.right,control.right)>Math.max(label.left,control.left) && Math.min(label.bottom,control.bottom)>Math.max(label.top,control.top),right:control.right};}));
  assert.ok(boxes.every(box=>!box.overlap && box.right<=360),JSON.stringify(boxes));
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('iPhone home stage can be hidden and restored without hiding recommendations',async()=>{
 const app=await openApp({config,phone:true});
 try{const {page}=app;await settings(page);await page.getByRole('switch',{name:'Карточка плеера на главной',exact:true}).uncheck();await page.locator('.dock-tab[data-tab="home"]').click();await page.locator('.home-view:not([hidden])').waitFor();assert.equal(await page.locator('.home-view .stage').isVisible(),false);assert.equal(await page.locator('.home-view .home-body').isVisible(),true);await page.reload();await page.locator('.dock').waitFor();assert.equal(await page.locator('.home-view .stage').isVisible(),false);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test('automatic liked downloads pause on metered data and resume on an unmetered connection',async()=>{
 const app=await openApp({config:{...config,metered:true,libraryTotal:3},phone:true});
 try{const {page}=app;await settings(page);await page.getByRole('switch',{name:'Скачивать избранное',exact:true}).check();await page.waitForTimeout(700);assert.equal(await page.evaluate(()=>window.__mock.count('download')),0);assert.equal(await page.locator('.dialog').count(),0);
 await page.evaluate(()=>{window.__mock.config.metered=false;window.dispatchEvent(new Event('online'));});await page.waitForFunction(()=>window.__mock.count('download')>=3);assert.ok(await page.evaluate(()=>JSON.parse(localStorage.getItem('pm.personal.7.autoLikeDownloads')).length>=3));assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('automatic cache reuses playback bytes, records ownership and never starts another download',async()=>{
 const app=await openApp({config:{...config,metered:true},phone:true});
 try{const {page}=app;await settings(page);await page.getByRole('switch',{name:'Сохранять прослушанные треки',exact:true}).check();await page.locator('.dock-tab[data-tab="home"]').click();await page.locator('.stage-cta').click();await page.waitForFunction(()=>window.__mock.state.player.status==='playing');await page.evaluate(()=>window.dispatchEvent(new Event('online')));await page.waitForFunction(()=>JSON.parse(localStorage.getItem('pm.personal.7.automaticCache')||'[]').length===1);assert.equal(await page.evaluate(()=>window.__mock.count('cache_playing')),1);await page.evaluate(()=>window.dispatchEvent(new Event('online')));await page.waitForTimeout(700);assert.equal(await page.evaluate(()=>window.__mock.count('cache_playing')),1);assert.equal(await page.evaluate(()=>window.__mock.count('download')),0);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test('VK integration requires separate account consent, clears the status and stops reports after opt-out',async()=>{
 const app=await openApp({config,phone:true});
 try{
  const {page}=app;await page.locator('.stage-cta').click();await page.waitForFunction(()=>window.__mock.state.player.status==='playing');
  assert.equal(await page.evaluate(()=>window.__mock.count('vk_broadcast')+window.__mock.count('vk_report_playback')),0);
  await settings(page);await page.getByRole('switch',{name:'Транслировать музыку в статус',exact:true}).check();await page.getByRole('switch',{name:'Отправлять VK историю прослушивания',exact:true}).check();
  await page.waitForFunction(()=>window.__mock.state.listeningEvents?.length===1 && window.__mock.state.broadcast);
  assert.equal(await page.evaluate(()=>window.__mock.state.listeningEvents[0].e),'music_start_playback');
  await page.locator('.player .play').click();await page.waitForFunction(()=>window.__mock.state.listeningEvents?.length===2 && window.__mock.state.broadcast===null);
  assert.equal(await page.evaluate(()=>window.__mock.state.listeningEvents[1].e),'music_stop_playback');
  await page.getByRole('switch',{name:'Отправлять VK историю прослушивания',exact:true}).uncheck();await page.getByRole('switch',{name:'Транслировать музыку в статус',exact:true}).uncheck();
  const count=await page.evaluate(()=>window.__mock.count('vk_report_playback'));await page.locator('.player .play').click();await page.waitForTimeout(150);assert.equal(await page.evaluate(()=>window.__mock.count('vk_report_playback')),count);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('VK failures stop retries until the user asks and never stop playback',async()=>{
 const app=await openApp({config:{...config,failing:['vk_broadcast']},phone:true});
 try{const {page}=app;await page.locator('.stage-cta').click();await settings(page);await page.getByRole('switch',{name:'Транслировать музыку в статус',exact:true}).check();await page.getByRole('button',{name:'Повторить подключение',exact:true}).waitFor();await page.waitForTimeout(250);assert.equal(await page.evaluate(()=>window.__mock.count('vk_broadcast')),1);assert.equal(await page.evaluate(()=>window.__mock.state.player.status),'playing');await page.evaluate(()=>window.__mock.config.failing=[]);await page.getByRole('button',{name:'Повторить подключение',exact:true}).click();await page.waitForFunction(()=>window.__mock.state.broadcast);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test('extra large text scales phone settings without horizontal overflow and saves playback policies',async()=>{
 const app=await openApp({config,phone:'small'});
 try{const {page}=app;await settings(page);const before=await page.locator('.settings h1').evaluate(el=>parseFloat(getComputedStyle(el).fontSize));await page.getByRole('combobox',{name:'Размер текста',exact:true}).selectOption('extraLarge');await page.waitForFunction(()=>parseFloat(getComputedStyle(document.querySelector('.settings h1')).fontSize)>35.9);const after=await page.locator('.settings h1').evaluate(el=>parseFloat(getComputedStyle(el).fontSize));assert.ok(Math.abs(after/before-1.2)<0.01);
 await page.getByRole('switch',{name:'Продолжать при подключении Bluetooth',exact:true}).uncheck();await page.getByRole('switch',{name:'Пауза при нулевой громкости телефона',exact:true}).check();await page.getByRole('switch',{name:'Высокое качество',exact:true}).uncheck();
 const overflow=await page.locator('.settings .set-body').evaluate(el=>({width:el.clientWidth,scroll:el.scrollWidth}));assert.ok(overflow.scroll<=overflow.width+1,JSON.stringify(overflow));await page.reload();await settings(page);assert.equal(await page.getByRole('combobox',{name:'Размер текста',exact:true}).inputValue(),'extraLarge');assert.deepEqual(await page.evaluate(()=>window.__mock.state.playbackPreferences),{resumeBluetooth:false,pauseAtZero:true});assert.equal(await page.evaluate(()=>window.__mock.state.highQuality),false);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('cache limit evicts oldest owned files and protects manual, pinned and unknown downloads across accounts',async()=>{
 const app=await openApp({config:{...config,libraryTotal:260},phone:true});
 try{const {page}=app;await settings(page);await page.evaluate(async()=>{
  const {api,trackKey}=await import('/js/api.js');const items=(await api.library(0,260)).items;window.__mock.state.downloads=items;
  localStorage.setItem('pm.personal.7.automaticCache',JSON.stringify(items.filter(t=>t.track_id!==4).map(t=>({id:trackKey(t),used:t.track_id}))));
  localStorage.setItem('pm.personal.7.manualDownloads',JSON.stringify(['7_1']));localStorage.setItem('pm.personal.9.manualDownloads',JSON.stringify(['7_2']));localStorage.setItem('pm.personal.9.playlistSync',JSON.stringify([{key:'9_1',owned:['7_3'],wanted:['7_3']}]));
  localStorage.setItem('pm.personal.7.offlinePreferences',JSON.stringify({cache:true,likes:false,limit:1}));(await import('/js/store.js')).publish('personal.offlinePreferences');
 });await page.waitForFunction(()=>window.__mock.count('remove_download')===5);assert.deepEqual(await page.evaluate(()=>window.__mock.calls.filter(c=>c.command==='remove_download').map(c=>c.args.trackId)),['7_5','7_6','7_7','7_8','7_9']);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('automatic download and VK consent switches restore after a delayed account profile',async()=>{
 const app=await openApp({config:{...config,profileDelay:150,metered:true},phone:true});
 try{const {page}=app;await settings(page);await page.waitForFunction(()=>document.querySelector('.settings .account-name')?.textContent || window.__mock.count('profile')>0);await page.waitForTimeout(180);await page.getByRole('switch',{name:'Скачивать избранное',exact:true}).check();await page.getByRole('switch',{name:'Отправлять VK историю прослушивания',exact:true}).check();await page.reload();await settings(page);await page.waitForFunction(()=>document.querySelector('input[aria-label="Скачивать избранное"]').checked && document.querySelector('input[aria-label="Отправлять VK историю прослушивания"]').checked);assert.equal(await page.evaluate(()=>window.__mock.count('download')),0);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
for(const phone of ['small','landscape'])test(`extra large player text keeps transport and title separate on ${phone}`,async()=>{
 const app=await openApp({config,phone});
 try{const {page}=app;await page.evaluate(()=>localStorage.setItem('pm.settings.v1',JSON.stringify({textScale:'extraLarge'})));await page.reload();await page.locator('.stage-cta').click();await page.locator('.player .now').click();await page.locator('.np:not([hidden])').waitFor();
 const bounds=await page.evaluate(()=>{const rect=selector=>{const r=document.querySelector(selector).getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom}};return {info:rect('.np-info'),seek:rect('.np-seek'),transport:rect('.np-controls'),quick:rect('.np-quick'),cover:rect('.np-cover'),scroll:document.querySelector('.np').scrollWidth,width:innerWidth};});
 assert.ok(bounds.info.bottom<=bounds.seek.top+1,JSON.stringify(bounds));assert.ok(bounds.transport.bottom<=bounds.quick.top+1,JSON.stringify(bounds));assert.ok(bounds.scroll<=bounds.width,JSON.stringify(bounds));assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
