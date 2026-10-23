import test from 'node:test';
import assert from 'node:assert/strict';
import { savedLyricsEntry, boundedLyrics, searchSavedLyrics, SAVED_LYRICS_BUDGET } from '../ui/js/saved-lyrics-model.js';
const track={owner_id:7,track_id:1,title:'Звёзды',artist:'Beyoncé',duration:120};
const lyrics={source:'Local',text:'Над рекой плывут облака\nМой далёкий дом',lines:[]};
test('saved lyrics sanitize track and lyric metadata and reject oversized or invalid input',()=>{
 const entry=savedLyricsEntry({...track,stream_url:'secret',access_key:'secret',album:{access_key:'secret'}},{...lyrics,token:'secret'});
 assert.equal(JSON.stringify(entry).includes('secret'),false);
 assert.equal(savedLyricsEntry({...track,track_id:0},lyrics),null);
 assert.equal(savedLyricsEntry(track,{...lyrics,text:'я'.repeat(75001)}),null);
 assert.equal(savedLyricsEntry(track,{...lyrics,lines:[{time:NaN,text:'bad'}]}),null);
});
test('saved lyrics match all words across fields, fold accents and preserve safe excerpts',()=>{
 const entries=[savedLyricsEntry(track,lyrics)];
 assert.equal(searchSavedLyrics(entries,'beyonce облака').length,1);
 assert.equal(searchSavedLyrics(entries,'звезды далекий')[0].excerpt,'Мой далёкий дом');
 assert.equal(searchSavedLyrics(entries,'звезды missing').length,0);
 assert.equal(searchSavedLyrics(entries,'я').length,0);
});
test('saved lyrics bound storage and deduplicate tracks while keeping newest entries',()=>{
 const entries=Array.from({length:160},(_,i)=>savedLyricsEntry({...track,track_id:i+1},{...lyrics,text:'x'.repeat(140000)}));
 const result=boundedLyrics([entries[0],...entries]);
 assert.ok(JSON.stringify(result).length<=SAVED_LYRICS_BUDGET);
 assert.equal(result[0].track.track_id,1);
 assert.equal(new Set(result.map(e=>e.track.track_id)).size,result.length);
 assert.ok(result.length<150);
});

import { openApp } from './harness.js';
async function ready(page) {
 await page.waitForSelector('.row');
 await page.waitForFunction(()=>!!document.querySelector('.account .name')?.textContent);
}
test('saved lyrics preserve old cache on quota failure and isolate accounts',async()=>{
 const app=await openApp({config:{signedIn:true,libraryTotal:5}});
 try {
  await ready(app.page);
  const result=await app.page.evaluate(async()=>{
   const {rememberLyrics,savedLyrics}=await import('/js/saved-lyrics.js');
   const {store}=await import('/js/store.js');
   const track={owner_id:7,track_id:1,title:'Title',artist:'Artist',duration:10};
   const saved=rememberLyrics(track,{text:'old lyric',lines:[],source:'local'});
   const original=Storage.prototype.setItem;
   Storage.prototype.setItem=function(key,value){if(key.endsWith('savedLyrics.v1'))throw new DOMException('Quota','QuotaExceededError');return original.call(this,key,value);};
   const failed=rememberLyrics(track,{text:'new lyric',lines:[],source:'local'});
   Storage.prototype.setItem=original;
   const old=savedLyrics(track).text;
   const profile=store.session.profile;store.session.profile={id:999};
   const other=savedLyrics(track);store.session.profile=profile;
   return {saved,failed,old,other};
  });
  assert.deepEqual(result,{saved:true,failed:false,old:'old lyric',other:null});
 } finally {await app.close();}
});
test('lyrics panel restores text after recreation without fetching and ignores late account results',async()=>{
 const app=await openApp({config:{signedIn:true,libraryTotal:5}});
 try {
  await ready(app.page);
  const result=await app.page.evaluate(async()=>{
   const {createLyricsPanel}=await import('/js/lyrics-panel.js');
   const {api}=await import('/js/api.js');
   const {store}=await import('/js/store.js');
   const {libraryTracks}=await import('/js/likes.js');
   const {savedLyrics}=await import('/js/saved-lyrics.js');
   const track=libraryTracks()[0], original=api.lyrics;
   api.lyrics=async()=>({text:'offline remembered lyric',source:'test',lines:[]});
   const first=createLyricsPanel();first.setTrack(track);
   await new Promise(resolve=>setTimeout(resolve,20));
   api.lyrics=async()=>{throw new Error('must use saved text');};
   const second=createLyricsPanel();second.setTrack(track);
   const text=second.element.querySelector('.ly-text')?.textContent;
   const next={...track,track_id:999};let resolve;
   api.lyrics=()=>new Promise(done=>{resolve=done;});
   second.setTrack(next);
   const profile=store.session.profile;store.session.profile={id:999};
   resolve({text:'late private lyric',source:'test',lines:[]});
   await new Promise(done=>setTimeout(done,20));
   const leaked=savedLyrics(next);store.session.profile=profile;
   api.lyrics=original;
   return {text,leaked};
  });
  assert.deepEqual(result,{text:'offline remembered lyric',leaked:null});
  assert.deepEqual(app.problems,[]);
 } finally {await app.close();}
});
test('phone saved-lyrics search reads safely offline, plays separately and closes reader on account change',async()=>{
 const app=await openApp({phone:'small',config:{signedIn:true,libraryTotal:5}});
 try {
  const {page}=app;await ready(page);
  await page.evaluate(async()=>{
   const {rememberLyrics}=await import('/js/saved-lyrics.js');
   const {libraryTracks}=await import('/js/likes.js');
   rememberLyrics({...libraryTracks()[0],title:'Звёзды',artist:'Beyoncé'},{text:'Над рекой плывут облака\n<img src=x onerror=alert(1)>',source:'Local',lines:[]});
  });
  await page.click('.dock-tab[data-tab="search"]');
  await page.getByRole('tab',{name:'Тексты',exact:true}).click();
  const before=await page.evaluate(()=>window.__mock.count('search'));
  const field=page.locator('.main > .view:not([hidden]) input[type="search"]');
  await field.fill('beyonce облака');await field.press('Enter');
  await page.waitForSelector('.saved-lyrics-card');
  assert.equal(await page.evaluate(()=>window.__mock.count('search')),before);
  await page.locator('.saved-lyrics-excerpt').click();
  assert.equal(await page.locator('.saved-lyrics-reader img').count(),0);
  assert.equal(await page.locator('.saved-lyrics-reader').evaluate(el=>getComputedStyle(el).backgroundColor.includes('rgba')),false);
  assert.match(await page.locator('.saved-lyrics-text').textContent(),/<img/);
  assert.equal(await page.evaluate(()=>window.__mock.count('seek')),0);
  await page.screenshot({path:'/tmp/pm-saved-lyrics-reader.png'});
  await page.getByRole('button',{name:'Закрыть',exact:true}).click();
  await page.screenshot({path:'/tmp/pm-saved-lyrics-search.png'});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  assert.equal(await page.locator('.search-tabs').evaluate(el=>Array.from(el.querySelectorAll('button')).every(button=>{const box=button.getBoundingClientRect(),outer=el.getBoundingClientRect();return box.right<=outer.right+1 && box.left>=outer.left-1;})),true);
  await page.locator('.saved-lyrics-play').click();
  await page.waitForFunction(()=>window.__mock.count('play_tracks')>0);
  await page.locator('.saved-lyrics-excerpt').click();
  await page.evaluate(async()=>{const {store,publish}=await import('/js/store.js');store.session.profile={id:999};publish('session');});
  assert.equal(await page.locator('.saved-lyrics-reader').count(),0);
  assert.equal(await page.locator('.saved-lyrics-card').count(),0);
  assert.deepEqual(app.problems,[]);
 } finally {await app.close();}
});
