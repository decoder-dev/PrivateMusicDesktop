import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';

const config={signedIn:true,catalogMixes:true};
async function player(page){await page.locator('.stage-cta').click();await page.locator('.player .now').click();await page.locator('.np:not([hidden])').waitFor();}

test('titles: desktop title follows the active section and resets on sign out',async()=>{
 const app=await openApp({config});
 try{const {page}=app;await page.waitForFunction(()=>document.title==='Моя музыка — Private Music');
  await page.locator('.nav [data-view="settings"]').click();await page.waitForFunction(()=>document.title==='Настройки — Private Music');
  assert.equal(await page.evaluate(()=>window.__mock.last('set_window_title').args.title),'Настройки — Private Music');
  await page.evaluate(()=>window.__mock.sessionExpired());await page.locator('.login').waitFor();
  await page.waitForFunction(()=>document.title==='Private Music');assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('titles: a pushed playlist and a late heading update reach the phone bar without scrolling',async()=>{
 const app=await openApp({config:{...config,platform:'android'},phone:'small'});
 try{const {page}=app;await page.locator('.dock-tab[data-tab="library"]').click();
  await page.locator('.lib-sections [data-section="playlists"]').click();await page.locator('.lib-grid .card').first().click();
  await page.waitForFunction(()=>document.title==='Дорога — Private Music');
  await page.waitForFunction(()=>document.querySelector('.navbar .nav-title').textContent==='Дорога');
  await page.evaluate(()=>{const h=[...document.querySelectorAll('.main > .view:not([hidden]) h1')].find(h=>h.offsetParent);h.textContent='Название после загрузки';});
  await page.waitForFunction(()=>document.querySelector('.navbar .nav-title').textContent==='Название после загрузки');
  assert.equal(await page.title(),'Название после загрузки — Private Music');
  assert.equal(await page.evaluate(()=>window.__mock.count('set_window_title')),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

for(const phone of [true,false])test(`settings shortcuts scroll the real container (${phone?'phone':'desktop'})`,async()=>{
 const app=await openApp({config,phone});
 try{const {page}=app;
  if(phone){await page.locator('.dock-tab[data-tab="profile"]').click();await page.locator('[data-row="settings"]').click();}
  else await page.locator('.nav [data-view="settings"]').click();
  await page.locator('.settings-nav button[aria-controls="settings-update-title"]').click();
  const result=await page.evaluate(()=>{const section=document.querySelector('#settings-update-title'),view=document.querySelector('.settings'),body=view.querySelector('.set-body');const box=section.getBoundingClientRect(),area=(document.documentElement.dataset.layout==='phone'?view:body).getBoundingClientRect();return {top:box.top,bottom:box.bottom,areaTop:area.top,areaBottom:area.bottom,scroll:Math.max(view.scrollTop,body.scrollTop)};});
  assert.ok(result.scroll>0,JSON.stringify(result));assert.ok(result.top>=result.areaTop-1 && result.top<result.areaBottom,JSON.stringify(result));assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('player shortcuts leave typing and modal Escape to the dialog',async()=>{
 const app=await openApp({config,phone:'small'});
 try{const {page}=app;await player(page);
  await page.evaluate(()=>{import('/js/dialog.js').then(({formDialog})=>formDialog({title:'Название',fields:[{name:'name',label:'Имя'}],confirm:'Сохранить'}));});
  await page.locator('dialog input').fill('');await page.locator('dialog input').pressSequentially('fly velvet');
  assert.equal(await page.locator('dialog input').inputValue(),'fly velvet');assert.equal(await page.locator('.np').getAttribute('hidden'),null);
  await page.keyboard.press('Escape');await page.locator('dialog').waitFor({state:'detached'});
  assert.equal(await page.locator('.np').getAttribute('hidden'),null);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('failed playback controls show an error and remain usable without an unhandled rejection',async()=>{
 const app=await openApp({config,phone:'small'});
 try{const {page}=app;await player(page);await page.evaluate(()=>window.__mock.config.failing=['toggle']);
  await page.locator('.np-play').click();await page.locator('.toast.error').waitFor();assert.deepEqual(app.problems,[]);
  await page.evaluate(()=>window.__mock.config.failing=[]);await page.locator('.np-play').click();
  await page.waitForFunction(()=>document.querySelector('.np-play').getAttribute('aria-label')==='Играть');assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('player focus stays within the player and returns to its opener; status distinguishes pause and buffering',async()=>{
 const app=await openApp({config,phone:'small'});
 try{const {page}=app;await page.locator('.stage-cta').click();await page.locator('.player:not(.is-empty)').waitFor();await page.locator('.player .cover-button').focus();await page.keyboard.press('Enter');await page.locator('.np:not([hidden])').waitFor();
  await page.locator('.np-quick button').last().focus();await page.keyboard.press('Tab');
  assert.equal(await page.evaluate(()=>document.querySelector('.np').contains(document.activeElement)),true);
  await page.locator('.np-play').click();await page.waitForFunction(()=>document.querySelector('.np .k1').textContent==='Пауза');
  await page.evaluate(async()=>{const {store}=await import('/js/store.js');window.__mock.emit('pm-event',{...store.player,type:'state',status:'buffering'});});
  await page.waitForFunction(()=>document.querySelector('.np').dataset.state==='buffering');
  assert.notEqual(await page.locator('.np .k1').textContent(),'Сейчас играет');
  await page.locator('.np-play').focus();await page.keyboard.press('Escape');await page.locator('.np').waitFor({state:'hidden'});
  assert.equal(await page.evaluate(()=>document.activeElement.matches('.player .cover-button')),true);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('queue failures show a message rather than becoming an unhandled page error',async()=>{
 const app=await openApp({config});
 try{const {page}=app;await page.locator('.main .row').first().dblclick();await page.locator('.nav [data-view="queue"]').click();
  await page.evaluate(()=>window.__mock.config.failing=['move_upcoming']);
  const row=page.locator('.main > .view:not([hidden]) .row').nth(2);
  await row.hover();await row.locator('[data-action="up"]').click();
  await page.locator('.toast.error').waitFor();assert.deepEqual(app.problems,[]);
  await page.evaluate(()=>window.__mock.config.failing=['jump']);
  await page.locator('.main > .view:not([hidden]) .row').nth(1).dblclick();
  await page.waitForFunction(()=>window.__mock.count('jump')>0);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('empty search offers no selection button; results do',async()=>{
 const app=await openApp({config:{...config,platform:'android'},phone:'small'});
 try{const {page}=app;await page.locator('.dock-tab[data-tab="search"]').click();await page.locator('.search-view input, .main > .view:not([hidden]) input[type="search"]').first().waitFor();
  assert.equal(await page.locator('.main > .view:not([hidden]) .select-toggle').isVisible(),false);
  await page.locator('.main > .view:not([hidden]) input').first().fill('тест');
  await page.locator('.main > .view:not([hidden]) .select-toggle').waitFor();assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('statistics play counts stay inside their cards on a phone',async()=>{
 const app=await openApp({config:{...config,platform:'android',history:[1,2,3]},phone:'small'});
 try{const {page}=app;await page.locator('.dock-tab[data-tab="profile"]').click();await page.locator('[data-row="stats"]').click();
  await page.locator('.stat-count').first().waitFor();
  const overflow=await page.locator('.stat-count').evaluateAll(nodes=>nodes.filter(n=>n.offsetParent).map(n=>{const card=n.closest('.stat-row').parentElement.getBoundingClientRect();return n.getBoundingClientRect().right-card.right;}).filter(d=>d>0));
  assert.deepEqual(overflow,[]);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
