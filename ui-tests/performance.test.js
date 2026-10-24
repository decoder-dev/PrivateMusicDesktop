import test from 'node:test';
import assert from 'node:assert/strict';
import {openApp} from './harness.js';

async function ready(page) {
  await page.waitForSelector('.shell');
  await page.waitForFunction(()=>!!document.querySelector('.account .name')?.textContent);
}

test('scrolling retains visible rows instead of rebuilding them every pixel',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const {page}=app;await ready(page);
  const result=await page.evaluate(async()=>{
   const {createVirtualList}=await import('/js/virtual-list.js');
   let builds=0;
   const list=createVirtualList({rowHeight:56,renderRow:(item)=>{builds++;const row=document.createElement('div');row.className='row';row.textContent=String(item);return row;}});
   Object.assign(list.element.style,{position:'absolute',inset:'100px auto auto 100px',width:'300px',height:'300px',flex:'none',zIndex:'10000',overflow:'auto'});
   document.body.append(list.element);list.setItems(Array.from({length:1000},(_,i)=>i));
   const frame=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
   await frame();const first=list.element.querySelector('.row');builds=0;
   for(let i=1;i<=12;i++){list.element.scrollTop=i;await frame();}
   const smallScrollBuilds=builds,sameRow=first===list.element.querySelector('.row');
   builds=0;list.element.scrollTop=124;await frame();const rangeShiftBuilds=builds;
   list.refresh();const forcedRefreshBuilds=builds-rangeShiftBuilds;
   list.element.style.display='none';await frame();
   list.setItems(Array.from({length:1000},(_,i)=>2000+i));
   list.element.style.display='block';await frame();
   const refreshedAfterShow=Number(list.element.querySelector('.row').textContent)>=2000;
   return {smallScrollBuilds,sameRow,rangeShiftBuilds,forcedRefreshBuilds,refreshedAfterShow};
  });
  assert.equal(result.smallScrollBuilds,0,JSON.stringify(result));
  assert.equal(result.sameRow,true,JSON.stringify(result));
  assert.ok(result.rangeShiftBuilds>0 && result.rangeShiftBuilds<=3,JSON.stringify(result));
  assert.ok(result.forcedRefreshBuilds>0,'data updates must still repaint visible rows');
  assert.equal(result.refreshedAfterShow,true,'a hidden list must show updated data when reopened');
 }finally{await app.close();}
});

test('phone simplified appearance really disables backdrop blur on the dock and player',async()=>{
 const app=await openApp({config:{signedIn:true,platform:'android'},phone:true});
 try {
  const {page}=app;await ready(page);
  await page.evaluate(()=>{const settings=JSON.parse(localStorage.getItem('pm.settings.v1')??'{}');localStorage.setItem('pm.settings.v1',JSON.stringify({...settings,flatChrome:true}));});
  await page.reload();await ready(page);
  await page.locator('.stage-cta').click();await page.locator('.player:not(.is-empty)').waitFor();
  const filters=await page.evaluate(async()=>{
   await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
   return ['.dock-tabs','.dock-searchwrap','.player'].map(selector=>({selector,filter:getComputedStyle(document.querySelector(selector)).backdropFilter}));
  });
  assert.ok(filters.every(item=>item.filter==='none'),JSON.stringify(filters));
 }finally{await app.close();}
});

test('position-only playback events preserve transport icons and leave the Home header alone',async()=>{
 const app=await openApp({config:{signedIn:true,platform:'android'},phone:true});
 try {
  const {page}=app;await ready(page);await page.locator('.stage-cta').click();
  await page.locator('.player:not(.is-empty)').waitFor();await page.waitForTimeout(60);
  const result=await page.evaluate(async()=>{
   const selectors=['.player .play svg','.player .repeat svg','.player .like svg','.mini-player .mini-play svg'];
   const before=selectors.map(s=>document.querySelector(s));
   const stage=document.querySelector('.home-view .stage');let stageMutations=0;
   const observer=new MutationObserver(records=>stageMutations+=records.length);observer.observe(stage,{subtree:true,childList:true});
   const {store}=await import('/js/store.js');
   for(let i=1;i<=20;i++)window.__mock.emit('pm-event',{...store.player,type:'state',position:i});
   await Promise.resolve();observer.disconnect();
   return {stageMutations,preserved:selectors.map((s,i)=>before[i]===document.querySelector(s))};
  });
  assert.equal(result.stageMutations,0,JSON.stringify(result));
  assert.ok(result.preserved.every(Boolean),JSON.stringify(result));
 }finally{await app.close();}
});

test('disposing a virtual list cancels pending frames and stops rendering on scroll',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const {page}=app;await ready(page);
  const builds=await page.evaluate(async()=>{
   const {createVirtualList}=await import('/js/virtual-list.js');
   const {withScope}=await import('/js/store.js');let builds=0;
   const scoped=withScope(()=>createVirtualList({rowHeight:56,renderRow:item=>{builds++;const row=document.createElement('div');row.className='row';row.textContent=String(item);return row;}}));
   const list=scoped.value;
   Object.assign(list.element.style,{position:'absolute',inset:'100px auto auto 100px',height:'300px',width:'300px',overflow:'auto'});
   document.body.append(list.element);list.setItems(Array.from({length:1000},(_,i)=>i));
   await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
   list.element.scrollTop=200;list.element.dispatchEvent(new Event('scroll'));
   scoped.dispose();builds=0;
   await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
   list.element.scrollTop=400;list.element.dispatchEvent(new Event('scroll'));
   await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
   return builds;
  });
  assert.equal(builds,0);
 }finally{await app.close();}
});
