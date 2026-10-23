import test from 'node:test';
import assert from 'node:assert/strict';
import {openApp} from './harness.js';
async function ready(page){await page.waitForSelector('.row');await page.waitForFunction(()=>!!document.querySelector('.account .name')?.textContent);}
async function fixture(page){
 await page.evaluate(async()=>{
  const{installLyricsCopy}=await import('/js/lyrics-copy.js');const{api}=await import('/js/api.js');const{accountId}=await import('/js/personal.js');
  const body=document.createElement('div');body.className='lyrics-copy-fixture';
  const line=document.createElement('button');line.className='ly-line';line.textContent='Строка для копирования';line.onclick=()=>api.seek(5);
  body.append(line);document.body.append(body);line.style.position='fixed';line.style.inset='100px 20px auto';line.style.zIndex='10000';line.style.height='70px';
  window.__lyricIdentity='track';window.__copier=installLyricsCopy(body,()=>`${accountId()}:${window.__lyricIdentity}`);
 });
}
test('lyrics: holding a line copies plain text once without seeking; the next tap still seeks',async()=>{
 const app=await openApp({query:'?platform=android',config:{signedIn:true,libraryTotal:5}});try{
  const{page}=app;await ready(page);await fixture(page);const line=page.locator('.lyrics-copy-fixture .ly-line');
  await line.hover();await page.mouse.down();await page.waitForTimeout(650);await page.mouse.up();
  assert.equal(await page.evaluate(()=>window.__mock.count('copy_text')),1);
  assert.equal(await page.evaluate(()=>window.__mock.last('copy_text').args.text),'Строка для копирования');
  assert.equal(await page.evaluate(()=>window.__mock.count('seek')),0);
  await line.click();assert.equal(await page.evaluate(()=>window.__mock.count('seek')),1);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('lyrics: moving or changing the source during a hold cancels the copy',async()=>{
 const app=await openApp({query:'?platform=android',config:{signedIn:true,libraryTotal:5}});try{
  const{page}=app;await ready(page);await fixture(page);const box=await page.locator('.lyrics-copy-fixture .ly-line').boundingBox();
  await page.mouse.move(box.x+20,box.y+20);await page.mouse.down();await page.mouse.move(box.x+50,box.y+20);await page.waitForTimeout(650);await page.mouse.up();
  assert.equal(await page.evaluate(()=>window.__mock.count('copy_text')),0);
  await page.mouse.down();await page.evaluate(()=>{window.__lyricIdentity='other';});await page.waitForTimeout(650);await page.mouse.up();
  assert.equal(await page.evaluate(()=>window.__mock.count('copy_text')),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('lyrics: saved reader copies only its displayed text and leaves playback alone',async()=>{
 const app=await openApp({phone:'small',config:{signedIn:true,libraryTotal:5}});try{
  const{page}=app;await ready(page);
  await page.evaluate(async()=>{const{rememberLyrics}=await import('/js/saved-lyrics.js');const{libraryTracks}=await import('/js/likes.js');rememberLyrics(libraryTracks()[0],{source:'Local',text:'Тихий вечер\nНовый день',lines:[]});});
  await page.click('.dock-tab[data-tab="search"]');await page.getByRole('tab',{name:'Тексты',exact:true}).click();
  const field=page.locator('.main > .view:not([hidden]) input[type="search"]');await field.fill('вечер');await field.press('Enter');await page.locator('.saved-lyrics-excerpt').click();
  await page.locator('.saved-lyrics-copy').click();
  assert.equal(await page.evaluate(()=>window.__mock.last('copy_text').args.text),'Тихий вечер\nНовый день');
  assert.equal(await page.evaluate(()=>window.__mock.count('seek')),0);
  assert.equal(await page.evaluate(()=>window.__mock.count('play_tracks')),0);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
test('lyrics: player updates do not cancel a hold, and the copy button copies the displayed version',async()=>{
 const app=await openApp({query:'?platform=android',config:{signedIn:true,libraryTotal:5}});try{
  const{page}=app;await ready(page);
  await page.evaluate(async()=>{
   const{createLyricsPanel}=await import('/js/lyrics-panel.js');const{libraryTracks}=await import('/js/likes.js');
   const track=libraryTracks()[0],panel=createLyricsPanel();document.body.append(panel.element);panel.setVisible(true);
   Object.assign(panel.element.style,{position:'fixed',inset:'20px auto auto 20px',width:'400px',height:'500px',zIndex:'10000',background:'white'});
   panel.setTrack(track);window.__lyricsRefresh=setInterval(()=>panel.setTrack(track),40);
  });
  await page.waitForSelector('.ly-line');
  const line=page.locator('.ly-line').first();await line.hover();await page.mouse.down();await page.waitForTimeout(650);await page.mouse.up();
  assert.equal(await page.evaluate(()=>window.__mock.count('copy_text')),1);
  assert.equal(await page.evaluate(()=>window.__mock.count('seek')),0);
  await page.locator('.ly-copy').last().click();
  assert.equal(await page.evaluate(()=>window.__mock.count('copy_text')),2);
  assert.match(await page.evaluate(()=>window.__mock.last('copy_text').args.text),/Строка 39/);
  await page.evaluate(()=>clearInterval(window.__lyricsRefresh));
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
