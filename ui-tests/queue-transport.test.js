import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';

for(const phone of [false,'small'])test(`next is unavailable at the end and returns when the queue grows (${phone||'desktop'})`,async()=>{
 const app=await openApp({config:{signedIn:true,platform:phone?'android':'linux'},phone});
 try{
  const p=app.page;p.setDefaultTimeout(5000);
  await p.evaluate(async()=>{const {api}=await import('/js/api.js');const {items}=await api.library(0,3);window.__queueFixture=items;await api.playTracks(items.slice(0,1),0);});
  const bar=p.locator('.player').getByRole('button',{name:'Следующий',exact:true});
  await p.waitForFunction(()=>document.querySelector('.player [aria-label="Следующий"]').disabled);
  assert.equal(await bar.isEnabled(),false);
  await p.locator('.player .cover-button').click();
  const next=p.locator('.np-controls').getByRole('button',{name:'Следующий',exact:true});
  assert.equal(await next.isEnabled(),false);
  await p.evaluate(async()=>{const {api}=await import('/js/api.js');await api.playLast(window.__queueFixture[1]);});
  await p.waitForFunction(()=>!document.querySelector('.np-controls [aria-label="Следующий"]').disabled);
  assert.equal(await bar.isEnabled(),true);
  await next.click();
  await p.waitForFunction(()=>document.querySelector('.np-title').textContent===window.__queueFixture[1].title);
  assert.equal(await p.evaluate(()=>window.__mock.count('next')),1);
  assert.equal(await next.isEnabled(),false);
  await p.evaluate(async()=>{const {api}=await import('/js/api.js');await api.setRepeat('all');});
  await p.waitForFunction(()=>!document.querySelector('.np-controls [aria-label="Следующий"]').disabled);
  await p.evaluate(async()=>{const {api}=await import('/js/api.js');await api.setRepeat('one');});
  await p.waitForFunction(()=>document.querySelector('.np-controls [aria-label="Следующий"]').disabled);
  if(!phone){
   await p.evaluate(async()=>{const {setWindowMode}=await import('/js/window-mode.js');await setWindowMode('mini');});
   assert.equal(await p.locator('.mini-player').getByRole('button',{name:'Следующий',exact:true}).isEnabled(),false);
  }
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('UI uses engine playback order for shuffled queues',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try{
  const p=app.page;
  await p.evaluate(async()=>{const {api}=await import('/js/api.js');const {items}=await api.library(0,3);await api.playTracks(items,2);});
  await p.waitForFunction(()=>document.querySelector('.player [aria-label="Следующий"]').disabled);
  await p.evaluate(()=>window.__mock.emit('pm-event',{type:'state',...window.__mock.state.player,shuffle:true,can_next:true}));
  await p.waitForFunction(()=>!document.querySelector('.player [aria-label="Следующий"]').disabled);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
