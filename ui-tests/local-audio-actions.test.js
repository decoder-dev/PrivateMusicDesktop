import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';

test('partial imports explain failures and removal clears the copy from the queue',async()=>{
 const app=await openApp({config:{signedIn:true,localImportFailed:['broken.mp3'],localImportSkipped:2}});
 try{
  const p=app.page;p.setDefaultTimeout(5000);
  await p.locator('.nav button[data-view="localAudio"]').click();
  const view=p.locator('.main>.view:not([hidden])');
  await view.getByRole('button',{name:'Импортировать музыку',exact:true}).click();
  const row=view.locator('.row[data-track-key="0_9001"]');await row.waitFor();
  assert.match(await view.locator('.import-status').innerText(),/broken\.mp3/);
  assert.match(await view.locator('.import-status').innerText(),/повторно: 2/);
  assert.equal(await view.getByRole('button',{name:'Скачать все',exact:true}).count(),0);
  await row.locator('.title').click();
  await view.locator('.row[data-track-key="0_9001"][aria-selected="true"]').waitFor();
  await row.hover();
  await row.locator('[data-action="menu"]').click();
  await p.locator('[data-action="remove-local"]').click();
  const dialog=p.getByRole('dialog');
  assert.match(await dialog.innerText(),/Исходный файл сохранится/);
  await dialog.getByRole('button',{name:'Удалить с устройства',exact:true}).click();
  await row.waitFor({state:'detached'});
  assert.equal(await p.evaluate(()=>window.__mock.count('remove_from_queue')),1);
  assert.equal(await p.evaluate(()=>window.__mock.last('remove_download').args.trackId),'0_9001');
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test('real cover swipes skip tracks once and upward swipes open the queue',async()=>{
 const app=await openApp({config:{signedIn:true,platform:'android'},phone:'small',fancyArt:true});
 try{
  const p=app.page;p.setDefaultTimeout(5000);
  await p.evaluate(async()=>{
   const {api}=await import('/js/api.js');
   const tracks=await api.library(0,3);await api.playTracks(tracks.items.map(track=>({...track,artwork_url:'https://example.com/cover.jpg'})),1);await api.seek(30);
  });
  await p.locator('.player .cover-button').click();
  const cover=p.locator('.np-cover');await cover.waitFor({state:'visible'});
  const box=await cover.boundingBox();assert.ok(box && box.width>50);
  const touch=await app.context.newCDPSession(p);
  const swipe=async(dx,dy)=>{
   const current=await cover.boundingBox();
   const x=current.x+current.width/2,y=current.y+current.height/2;
   await touch.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y,id:1}]});
   for(let step=1;step<=6;step++)await touch.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:x+dx*step/6,y:y+dy*step/6,id:1}]});
   await touch.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  };
  await swipe(-70,0);
  assert.equal(await p.evaluate(()=>window.__mock.count('next')),1);
  await swipe(70,0);
  assert.equal(await p.evaluate(()=>window.__mock.count('previous_track')),1);
  assert.equal(await p.evaluate(()=>window.__mock.count('previous')),0);
  await swipe(0,-65);
  assert.equal(await p.locator('.np-queue').isVisible(),true);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
