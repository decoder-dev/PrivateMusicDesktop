import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

const config = { signedIn: true, platform: "android", catalogMixes: true };

test('transport buttons acknowledge a slow command immediately and clear feedback on failure',async()=>{
 const app=await openApp({config,phone:'small'});
 try {
  const {page}=app;await page.locator('.stage-cta').click();
  await page.evaluate(()=>{
   const invoke=window.__TAURI__.core.invoke;
   window.__TAURI__.core.invoke=(command,args)=>command==='toggle'?new Promise((resolve,reject)=>{window.__finishToggle=()=>reject({kind:'network',message:'Slow command failed'});}):invoke(command,args);
  });
  await page.locator('.player .play').tap();
  assert.equal(await page.locator('.player .play').getAttribute('aria-busy'),'true');
  await page.evaluate(()=>window.__finishToggle());
  await page.waitForFunction(()=>!document.querySelector('.player .play').classList.contains('is-requesting'));
  assert.equal(await page.locator('.player .play').getAttribute('aria-busy'),null);
  assert.deepEqual(app.problems,[]);
 } finally {await app.close();}
});

test('a Play icon redrawn while the finger is down does not lose the button tap',async()=>{
 const app=await openApp({config,phone:'small'});
 try {
  const {page}=app;await page.locator('.stage-cta').click();
  await page.locator('.player .play').waitFor({state:'visible'});
  const box=await page.locator('.player .play').boundingBox();
  const cdp=await page.context().newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:box.x+box.width/2,y:box.y+box.height/2,id:1}]});
  await page.evaluate(()=>{window.__mock.state.player.status='paused';window.__mock.emitState();});
  await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await cdp.detach();
  await page.waitForTimeout(80);
  assert.equal(await page.evaluate(()=>window.__mock.count('toggle')),1);
 } finally {await app.close();}
});
async function openPlayer(app) {
  const { page } = app;
  await page.locator(".stage-cta").click();
  await page.locator(".player:not(.is-empty)").waitFor();
  await page.locator(".player .now").click();
  await page.locator(".np:not([hidden])").waitFor();
}

for (const phone of ["small", "pixel7", "large", "landscape"]) {
  test(`phone ${phone}: radial spectrum is visible and clears the cover, title and controls`, async () => {
    const app = await openApp({ config, phone, reducedMotion: "no-preference" });
    try {
      const { page } = app;
      await openPlayer(app);
      for (let i=0;i<3;i++) await page.locator(".np-mode").click();
      await page.waitForTimeout(350);
      const result = await page.evaluate(async () => {
        window.__mock.emit("pm-event", { type: "spectrum", bands: Array(64).fill(1) });
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const canvas = document.querySelector(".np-canvas");
        const pixels = canvas.getContext("2d").getImageData(0,0,canvas.width,canvas.height).data;
        let left=canvas.width,right=0,top=canvas.height,bottom=0,count=0;
        for(let y=0;y<canvas.height;y++) for(let x=0;x<canvas.width;x++) {
          if(pixels[(y*canvas.width+x)*4+3]>4) {
            left=Math.min(left,x);right=Math.max(right,x);top=Math.min(top,y);bottom=Math.max(bottom,y);count++;
          }
        }
        const rect = el => { const r=el.getBoundingClientRect(); return {left:r.left,right:r.right,top:r.top,bottom:r.bottom}; };
        const bounds=canvas.getBoundingClientRect(),ratio=canvas.width/canvas.clientWidth;
        return {count,ring:{left:bounds.left+left/ratio,right:bounds.left+right/ratio,top:bounds.top+top/ratio,bottom:bounds.top+bottom/ratio},
          stage:rect(document.querySelector(".np-stage")),info:rect(document.querySelector(".np-info")),
          cover:rect(document.querySelector(".np-cover")),bottom:rect(document.querySelector(".np-bottom"))};
      });
      const {ring,stage,info,cover,bottom}=result;
      assert.ok(result.count>100,JSON.stringify(result));
      assert.ok(ring.top>=stage.top && ring.bottom<=stage.bottom,JSON.stringify(result));
      assert.ok(ring.right<info.left || ring.bottom<info.top || ring.left>info.right,JSON.stringify(result));
      assert.ok(ring.left<cover.left && ring.right>cover.right,JSON.stringify(result));
      assert.ok(info.bottom<=bottom.top+1,JSON.stringify(result));
      assert.deepEqual(app.problems,[]);
    } finally { await app.close(); }
  });
}

test("phone: continuous equalizer drag applies intermediate values and saves the final curve on closing", async () => {
  const app=await openApp({config,phone:"small"});
  try {
    const {page}=app;
    await openPlayer(app);
    await page.locator(".np-quick button").nth(2).click();
    await page.locator(".sheet .eq-slider").first().waitFor();
    assert.ok(await page.locator(".sheet .eq-label").first().evaluate(el=>{const r=el.getBoundingClientRect(),body=el.closest(".sheet-body").getBoundingClientRect();return r.bottom<=body.bottom;}),"frequency labels should be visible without vertical scrolling");
    assert.equal(await page.locator(".sheet .eq-slider").first().evaluate(el=>getComputedStyle(el).touchAction),"none");
    await page.waitForTimeout(280);
    const result=await page.evaluate(async()=>{
      const input=document.querySelector(".sheet .eq-slider");
      const before=window.__mock.count("set_equalizer");
      for(let i=1;i<=12;i++) {input.value=String(i);input.dispatchEvent(new Event("input",{bubbles:true}));await new Promise(r=>setTimeout(r,20));}
      const during=window.__mock.count("set_equalizer")-before;
      input.value="7";input.dispatchEvent(new Event("input",{bubbles:true}));
      return during;
    });
    assert.ok(result>=2 && result<=5,`expected bounded live updates, got ${result}`);
    await page.locator(".sheet-scrim").click({position:{x:8,y:8}});
    await page.waitForFunction(()=>window.__mock.state.eq?.[0]===7);
    assert.equal(await page.evaluate(()=>JSON.parse(localStorage.getItem("pm.settings.v1")).eq.gains[0]),7);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});

test("phone: recommendations stay cached on a quick return and refresh after five minutes",async()=>{
  const app=await openApp({config,phone:true});
  try {
    const {page}=app;
    await page.waitForFunction(()=>window.__mock.count("recommendations")===1);
    await page.waitForTimeout(60);
    await page.locator('.dock-tab[data-tab="library"]').click();
    await page.locator('.dock-tab[data-tab="home"]').click();
    assert.equal(await page.evaluate(()=>window.__mock.count("recommendations")),1);
    await page.locator('.dock-tab[data-tab="library"]').click();
    await page.evaluate(()=>{const now=Date.now;Date.now=()=>now()+5*60*1000+1;});
    await page.locator('.dock-tab[data-tab="home"]').click();
    await page.waitForFunction(()=>window.__mock.count("recommendations")===2);
    assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
});

for(const phone of ["small","pixel7","landscape"]) {
  test(`phone ${phone}: bars, mirror and wave stay between the text and playback controls`,async()=>{
    const app=await openApp({config,phone,reducedMotion:"no-preference"});
    try {
      const {page}=app;
      await openPlayer(app);
      for(const mode of ["bars","mirror","wave"]) {
        await page.waitForTimeout(300);
        const result=await page.evaluate(async()=>{
          window.__mock.emit("pm-event",{type:"spectrum",bands:Array(64).fill(1)});
          await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
          const canvas=document.querySelector(".np-canvas"),context=canvas.getContext("2d");
          const data=context.getImageData(0,0,canvas.width,canvas.height).data;
          let top=canvas.height,bottom=0,count=0;
          for(let y=0;y<canvas.height;y++)for(let x=0;x<canvas.width;x++)if(data[(y*canvas.width+x)*4+3]>4){top=Math.min(top,y);bottom=Math.max(bottom,y);count++;}
          const frame=canvas.getBoundingClientRect(),ratio=canvas.height/canvas.clientHeight;
          return {count,top:frame.top+top/ratio,bottom:frame.top+bottom/ratio,infoBottom:document.querySelector(".np-info").getBoundingClientRect().bottom,controlsTop:document.querySelector(".np-bottom").getBoundingClientRect().top};
        });
        assert.ok(result.count>100,`${mode}: ${JSON.stringify(result)}`);
        assert.ok(result.top>result.infoBottom && result.bottom<result.controlsTop,`${mode}: ${JSON.stringify(result)}`);
        await page.locator(".np-mode").click();
      }
      assert.deepEqual(app.problems,[]);
    }finally{await app.close();}
  });
}

test("phone: a finger moves a vertical EQ fader without scrolling the sheet",async()=>{
  const app=await openApp({config,phone:"small"});
  try {
    const {page}=app;
    await openPlayer(app);
    await page.locator(".np-quick button").nth(2).click();
    await page.locator(".sheet .eq-slider").first().waitFor();
    await page.waitForTimeout(300);
    const slider=await page.locator(".sheet .eq-slider").first().boundingBox();
    const before=await page.locator(".sheet-body").evaluate(el=>el.scrollTop);
    const cdp=await page.context().newCDPSession(page);
    try {
      const x=slider.x+slider.width/2,y=slider.y+slider.height/2;
      await cdp.send("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[{x,y,id:1}]});
      for(let i=1;i<=8;i++) {
        await cdp.send("Input.dispatchTouchEvent",{type:"touchMove",touchPoints:[{x,y:y-i*(slider.height/2-10)/8,id:1}]});
        await page.waitForTimeout(20);
      }
      await cdp.send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});
    }finally{await cdp.detach();}
    await page.waitForFunction(()=>window.__mock.state.eq?.[0]>=8);
    assert.equal(await page.locator(".sheet-body").evaluate(el=>el.scrollTop),before);
    assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
});

for(const phone of ['small','pixel7','landscape']) {
  test(`phone ${phone}: long titles fit beside one favourite and playback is centred`,async()=>{
    const app=await openApp({config,phone});
    try {
      const {page}=app;await openPlayer(app);
      await page.evaluate(async()=>{
        const {store}=await import('/js/store.js');
        window.__mock.emit('pm-event',{...store.player,type:'state',track:{...store.player.track,title:'Очень длинное название любимой песни — концертная запись',artist:'Исполнитель с длинным именем'}});
      });
      const metrics=await page.evaluate(()=>{
        const rect=s=>document.querySelector(s).getBoundingClientRect();
        const title=rect('.np-title'),like=rect('.np-like'),play=rect('.np-play'),controls=rect('.np-controls'),cover=rect('.np-cover'),header=rect('.np-top'),info=rect('.np-info'),bottom=rect('.np-bottom');
        return {favourites:document.querySelectorAll('.np-like').length,separate:title.right<=like.left,centred:Math.abs(play.x+play.width/2-innerWidth/2)<1,coverBelowHeader:cover.top>=header.bottom-1,infoAboveControls:info.bottom<bottom.top,controlsInside:controls.right<=innerWidth && controls.left>=0};
      });
      assert.deepEqual(metrics,{favourites:1,separate:true,centred:true,coverBelowHeader:true,infoAboveControls:true,controlsInside:true});
      await page.locator('.np-like').click();
      await page.waitForFunction(()=>document.querySelector('.np-like').getAttribute('aria-pressed')==='true');
      await page.locator('.np-quick button').first().click();
      assert.equal(await page.locator('.np-main').isVisible(),false);
      assert.deepEqual(app.problems,[]);
    }finally{await app.close();}
  });
}

test("phone: the label of the player's equaliser switch toggles its own switch, not the hidden Settings one",async()=>{
  const app=await openApp({config,phone:"small"});
  try {
    const {page}=app;
    await openPlayer(app);
    await page.locator(".np-quick button").nth(2).click();
    const toggle=page.locator(".sheet .set-check input");
    await toggle.waitFor();
    await page.waitForTimeout(300);
    assert.equal(await toggle.isChecked(),false);
    await page.locator(".sheet .set-check span").first().click();
    await page.waitForFunction(()=>document.querySelector(".sheet .set-check input").checked);
    assert.equal(await page.evaluate(()=>[...document.querySelectorAll(".eq .set-check input")].filter(input=>!input.closest(".sheet")).some(input=>input.checked)),false);
    assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
});
