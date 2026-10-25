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

test('idle desktop: no "-0:00", empty queue has no dead buttons, sync checkbox sits beside its label',async()=>{
 const app=await openApp({config});
 try{const {page}=app;
  assert.equal(await page.locator('.player .seek span').last().textContent(),'0:00');
  await page.locator('.nav [data-view="queue"]').click();
  assert.equal(await page.locator('.main > .view:not([hidden]) .btn:visible').filter({hasText:/Предыдущие очереди|Очистить следующие/}).count(),0);
  await page.locator('.nav [data-view="downloads"]').click();
  const gap=await page.locator('.sync-panel label.set-inline').evaluate(l=>{const box=l.querySelector('input').getBoundingClientRect();const range=document.createRange();range.selectNodeContents(l.lastChild);return range.getBoundingClientRect().left-box.right;});
  assert.ok(gap>=0 && gap<24,String(gap));assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('renaming a coverless playlist refreshes the heading and title; new artwork metadata replaces the absent cover',async()=>{
 const app=await openApp({config:{...config,platform:'android'},phone:'small'});
 try{const {page}=app;
  await page.locator('.dock-tab[data-tab="library"]').click();await page.locator('.lib-sections [data-section="playlists"]').click();await page.locator('.lib-grid .card').first().click();
  await page.locator('.pl-owner .btn').filter({hasText:'Изменить'}).click();await page.locator('dialog input[name="title"]').fill('Новое название');await page.locator('dialog button[type="submit"]').click();
  await page.waitForFunction(()=>document.title==='Новое название — Private Music');
  const art=page.locator('.main > .view:not([hidden]) .hero-art').filter({visible:true});
  assert.equal(await art.count(),0);
  await page.evaluate(async()=>{const {allPlaylists,replacePlaylist}=await import('/js/my-playlists.js');const playlist=allPlaylists().find(p=>p.title==='Новое название');replacePlaylist({...playlist,artwork_url:'https://cdn.example/updated-cover.jpg'});});
  const cover=page.locator('.main > .view:not([hidden]) .hero-art img[src="https://cdn.example/updated-cover.jpg"]');await cover.waitFor();
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

for(const width of [320,412])test(`collection pin controls clear long titles at enlarged text (${width})`,async()=>{
 const app=await openApp({config:{...config,platform:'android'},phone:{viewport:{width,height:740}}});
 try{const {page}=app;await page.evaluate(()=>localStorage.setItem('pm.settings.v1',JSON.stringify({textScale:'extraLarge'})));await page.reload();
  await page.locator('.dock-tab[data-tab="library"]').click();await page.locator('.lib-sections [data-section="playlists"]').click();await page.locator('.lib-grid .collection-card').first().waitFor();
  await page.evaluate(async()=>{const {allPlaylists,replacePlaylist}=await import('/js/my-playlists.js');replacePlaylist({...allPlaylists()[0],title:'Очень длинное название любимой музыкальной коллекции'});});
  for(const layout of ['grid','list']){
   const issues=await page.locator('.lib-grid:not([hidden]) .lib-collection').evaluateAll(items=>items.flatMap(item=>{const title=item.querySelector('.name').getBoundingClientRect(),pin=item.querySelector('.lib-pin').getBoundingClientRect();return Math.min(title.right,pin.right)>Math.max(title.left,pin.left)+1 && Math.min(title.bottom,pin.bottom)>Math.max(title.top,pin.top)+1 ? ['title overlaps pin'] : [];}));
   assert.deepEqual(issues,[],layout);if(layout==='grid')await page.locator('.lib-layout[data-layout-kind="playlist"]').click();
  }assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('phone Home: the compact title keeps a left margin when there is no Back button',async()=>{
 const app=await openApp({config:{...config,platform:'android'},phone:'small'});
 try{const {page}=app;await page.locator('.navbar .nav-title').waitFor({state:'attached'});
  const left=await page.locator('.navbar .nav-title').evaluate(el=>{const range=document.createRange();range.selectNodeContents(el);return range.getBoundingClientRect().left;});
  assert.ok(left>=12,`title text starts at ${left}px`);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('wide full-screen player: volume is reachable and the spectrum stays below the controls',async()=>{
 const app=await openApp({config,viewport:{width:1280,height:800}});
 try{const {page}=app;
  await page.evaluate(()=>localStorage.setItem('pm.settings.v1',JSON.stringify({visualizer:'bars'})));await page.reload();
  await page.locator('.main .row').nth(1).dblclick();await page.locator('.player:not(.is-empty)').waitFor();
  await page.mouse.click(1225,746);await page.locator('.np:not([hidden])').waitFor();
  const slider=page.locator('.np .np-volume input');await slider.waitFor();
  await slider.fill('40');await page.waitForFunction(()=>Math.abs(window.__mock.state.player.volume-0.4)<0.01);
  await page.locator('.np .np-mute').click();await page.waitForFunction(()=>window.__mock.state.player.volume===0);
  await page.locator('.np .np-mute').click();await page.waitForFunction(()=>Math.abs(window.__mock.state.player.volume-0.4)<0.01);
  await page.evaluate(()=>window.__mock.emit('pm-event',{type:'spectrum',bands:Array(64).fill(1)}));
  const r=await page.evaluate(async()=>{
   await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
   const canvas=document.querySelector('.np-canvas'),ctx=canvas.getContext('2d'),data=ctx.getImageData(0,0,canvas.width,canvas.height).data;
   let top=canvas.height,count=0;for(let y=0;y<canvas.height;y++)for(let x=0;x<canvas.width;x++)if(data[(y*canvas.width+x)*4+3]>4){top=Math.min(top,y);count++;}
   const ratio=canvas.height/canvas.clientHeight,q=document.querySelector('.np-quick').getBoundingClientRect(),c=document.querySelector('.np-cover').getBoundingClientRect(),t=document.querySelector('.np-top').getBoundingClientRect();
   return {count,top:canvas.getBoundingClientRect().top+top/ratio,quickBottom:q.bottom,coverTop:c.top,headerBottom:t.bottom};});
  assert.ok(r.count>100,JSON.stringify(r));assert.ok(r.top>=r.quickBottom,JSON.stringify(r));assert.ok(r.coverTop>=r.headerBottom-1,'cover is not clipped by the header '+JSON.stringify(r));
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('phone Profile has no separate Equaliser row that only forwards to Settings',async()=>{
 const app=await openApp({config:{...config,platform:'android'},phone:'small'});
 try{const {page}=app;await page.locator('.dock-tab[data-tab="profile"]').click();await page.locator('[data-row="settings"]').waitFor();
  assert.equal(await page.locator('[data-row="eq"]').count(),0);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
