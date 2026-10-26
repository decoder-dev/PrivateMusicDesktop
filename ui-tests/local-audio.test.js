import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';

for(const phone of [false,'small'])test(`local music imports, plays and keeps VK actions hidden (${phone||'desktop'})`,async()=>{
 const app=await openApp({config:{signedIn:true,platform:phone?'android':'linux'},phone});
 try{
  const {page}=app;page.setDefaultTimeout(5000);
  if(phone){
   await page.locator('.dock-tab[data-tab="library"]').click();
   await page.getByRole('button',{name:'Действия',exact:true}).click();
   await page.waitForTimeout(260);
   await page.locator('[data-action="local-audio"]').click();
  }else await page.locator('.nav button[data-view="localAudio"]').click();
  const view=page.locator('.main>.view:not([hidden])');
  await view.getByRole('button',{name:'Импортировать музыку',exact:true}).click();
  await view.locator('.row[data-track-key="0_9001"]').waitFor();
  const inset=await view.evaluate(view=>view.querySelector('.import-note').getBoundingClientRect().left-view.getBoundingClientRect().left);
  assert.ok(inset>=14,'the import text aligns with the page gutter');
  assert.equal(await view.locator('.row .like,.row [data-action="download"]').count(),0);
  await view.locator('.row .title').click();
  assert.equal(await page.evaluate(()=>window.__mock.last('play_tracks').args.tracks[0].owner_id),0);
  await page.locator('.player .cover-button').click();
  assert.equal(await page.locator('.np-like').isVisible(),false);
  await page.locator('.np-quick').getByRole('button',{name:'Ещё',exact:true}).click();
  if(phone)await page.waitForTimeout(260);
  await page.locator('[data-action="copy-info"]').click();
  if(phone)assert.equal(await page.evaluate(()=>window.__mock.last('copy_text').args.text),'Local artist — Local song');
  assert.equal(await page.evaluate(()=>window.__mock.count('add_to_playlist')),0);
  assert.equal(await page.evaluate(()=>window.__mock.count('vk_report_playback')),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('artwork gesture policy rejects diagonals, reversals and taps',async()=>{
 const app=await openApp({config:{signedIn:true},phone:'small'});
 try{
  const result=await app.page.evaluate(async()=>{
   const {artworkGestureAction:action}=await import('/js/mobile/artwork-gestures.js');
   return [action({x:-30,y:4},{x:-130,y:10}),action({x:30,y:4},{x:130,y:10}),
    action({x:8,y:1},{x:130,y:10}),action({x:60,y:57},{x:120,y:110}),action({x:30,y:0},{x:-130,y:0}),action({x:4,y:-85},{x:8,y:-140})];
  });
  assert.deepEqual(result,['next','previous',null,null,null,'queue']);
 }finally{await app.close();}
});
