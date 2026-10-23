import test from 'node:test';
import assert from 'node:assert/strict';
import { openApp } from './harness.js';
async function setup(config={}) {
 const app=await openApp({phone:true,config:{signedIn:true,libraryTotal:5,...config}});
 app.page.setDefaultTimeout(10000);
 await app.page.waitForSelector('.row');
 await app.page.waitForFunction(()=>!!document.querySelector('.account .name')?.textContent);
 await app.page.evaluate(async()=>{
  const {libraryTracks}=await import('/js/likes.js');
  const {refreshDownloads}=await import('/js/downloads.js');
  window.__mock.state.downloads=[libraryTracks()[0]];await refreshDownloads();
 });
 return app;
}
async function open(page) {
 await page.click('.dock-tab[data-tab="library"]');
 await page.locator('.lib-summary').first().click();
 await page.locator('.main > .view:not([hidden]) .row').first().locator('[data-action="menu"]').click();
 await page.waitForTimeout(280); // The sheet ignores the finger release that opened it.
 await page.locator('[data-action="file-details"]').click();
 await page.waitForSelector('.file-status[data-available="true"]');
}
test('audio file details show native facts and cached-only sharing never downloads a missing file',async()=>{
 const app=await setup();try{
  const {page}=app;await open(page);
  assert.match(await page.locator('.file-facts').textContent(),/AAC/);
  assert.match(await page.locator('.file-facts').textContent(),/4 МБ/);
  assert.equal(await page.evaluate(()=>window.__mock.count('play_tracks')),0);
  await page.locator('.file-share').click();
  await page.waitForFunction(()=>window.__mock.count('share_audio')===1);
  assert.equal(await page.evaluate(()=>window.__mock.last('share_audio').args.allowDownload),false);
  assert.equal(await page.evaluate(()=>window.__mock.count('download')),0);
  await page.evaluate(()=>{window.__mock.state.downloads=[];});
  await page.locator('.file-share').click();
  await page.waitForSelector('.file-status[data-available="false"]');
  assert.equal(await page.locator('.file-play').isDisabled(),true);
  assert.equal(await page.locator('.file-share').isDisabled(),true);
  assert.equal(await page.evaluate(()=>window.__mock.count('download')),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('audio file deletion asks first and removes only the requested downloaded copy',async()=>{
 const app=await setup();try{
  const {page}=app;await open(page);
  await page.locator('.file-remove').click();
  assert.equal(await page.locator('.file-play').isVisible(),false);
  assert.equal(await page.locator('.file-confirm-remove').evaluate(el=>{const box=el.getBoundingClientRect(),parent=el.closest('dialog').getBoundingClientRect();return box.bottom<=parent.bottom && box.top>=parent.top;}),true);
  await page.locator('.file-cancel-remove').click();
  assert.equal(await page.evaluate(()=>window.__mock.count('remove_download')),0);
  await page.locator('.file-remove').click();
  await page.locator('.file-confirm-remove').click();
  await page.waitForFunction(()=>window.__mock.count('remove_download')===1);
  await page.waitForFunction(()=>!document.querySelector('.file-details-dialog'));
  assert.equal(await page.evaluate(()=>window.__mock.count('remove_from_library')),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('audio file details discard late metadata after account change',async()=>{
 const app=await setup({downloadDetailsDelay:500});try{
  const {page}=app;
  await page.evaluate(async()=>{
   const {createDownloadDetails}=await import('/js/download-details.js');
   const {libraryTracks}=await import('/js/likes.js');
   const {store,publish}=await import('/js/store.js');
   const details=createDownloadDetails({play:()=>{},toast:()=>{}});
   details.open(libraryTracks()[0]);store.session.profile={id:999};publish('session');
  });
  await page.waitForTimeout(600);
  assert.equal(await page.locator('.file-details-dialog').count(),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
