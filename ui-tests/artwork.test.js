import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';

test('artwork: missing covers keep row height, numbering and collection titles without empty tiles', async()=>{
  const app=await openApp({config:{signedIn:true,platform:'android'},phone:'small'});
  try {
    const result=await app.page.evaluate(async()=>{
      const {trackRow}=await import('/js/track-row.js');
      const {albumCard}=await import('/js/album-card.js');
      const row=trackRow({owner_id:1,track_id:1,title:'Без обложки',artist:'Исполнитель',duration:120},{index:6});
      const card=albumCard({title:'Альбом без обложки',artists:[],artwork_url:null},()=>{});
      const container=document.createElement('div');
      container.append(row,card);document.querySelector('.home-body').append(container);
      const result={number:row.querySelector('.art-ph').textContent,height:row.getBoundingClientRect().height,
        artwork:getComputedStyle(card.querySelector('.card-art')).display,title:card.querySelector('.name').textContent};
      container.remove();return result;
    });
    assert.deepEqual(result,{number:'7',height:56,artwork:'none',title:'Альбом без обложки'});
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});

test('artwork: a failed request removes the broken image and preserves a quiet fallback',async()=>{
  const app=await openApp({config:{signedIn:true},phone:'small'});
  try {
    await app.page.route('**/broken-cover.jpg',route=>route.fulfill({status:200,contentType:'image/jpeg',body:'invalid image'}));
    await app.page.evaluate(async()=>{
      const {artBox}=await import('/js/ui-parts.js');
      const box=artBox('/broken-cover.jpg',{eager:true,fallbackText:3});
      box.id='broken-art-test';document.querySelector('.main').append(box);
    });
    await app.page.waitForFunction(()=>document.querySelector('#broken-art-test').dataset.art==='error');
    const result=await app.page.locator('#broken-art-test').evaluate(box=>({images:box.querySelectorAll('img').length,text:box.textContent,background:getComputedStyle(box).backgroundImage}));
    assert.deepEqual(result,{images:0,text:'3',background:'none'});
    assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
});

test('player artwork: a broken next cover cannot restore a stale image after decoding fails',async()=>{
  const app=await openApp({config:{signedIn:true,platform:'android'},phone:'small'});
  try{
    const {page}=app;
    await page.locator('.stage-cta').click();await page.locator('.player .now').click();
    await page.locator('.np:not([hidden]) .np-cover img').waitFor();
    await page.locator('.np-backdrop .np-bd.show').first().waitFor({state:'attached'});
    await page.route('**/broken-next-cover.jpg',route=>route.fulfill({status:200,contentType:'image/jpeg',body:'invalid image'}));
    await page.evaluate(async()=>{
      const {store}=await import('/js/store.js');
      window.__mock.emit('pm-event',{type:'state',...store.player,track:{...store.player.track,title:'Следующий',artist:'Новый исполнитель',artwork_url:location.origin+'/broken-next-cover.jpg'}});
    });
    await page.locator('.np-cover .np-placeholder').waitFor();
    await page.waitForTimeout(550);
    assert.equal(await page.locator('.np-cover img').count(),0);
    assert.equal(await page.locator('.np-placeholder').textContent(),'Н');
    assert.equal(await page.locator('.np-backdrop .show').count(),0);
    assert.equal(await page.locator('.np-title').textContent(),'Следующий');
    assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
});
