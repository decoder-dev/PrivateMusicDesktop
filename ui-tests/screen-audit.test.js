import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';
import {everyPhoneScreen} from './phone-screen-walk.js';
const config={signedIn:true,catalogMixes:true,history:[1,2,3],platform:'android'};
for(const phone of [{name:'compact',viewport:{width:320,height:568}},{name:'landscape',viewport:{width:915,height:412}}]){
 test(`screen audit: all screens contain enlarged text and controls on ${phone.name}`,async()=>{
  const app=await openApp({config,phone});
  try{const {page}=app;await page.evaluate(()=>localStorage.setItem('pm.settings.v1',JSON.stringify({textScale:'extraLarge'})));await page.reload();await page.locator('.home-view .stage').waitFor();
   const issues=[];const screens=[];
   await everyPhoneScreen(page,async screen=>{
    screens.push(screen);
    const found=await page.evaluate(()=>{
     const issues=[];
     const intersects=(a,b)=>Math.min(a.right,b.right)-Math.max(a.left,b.left)>1 && Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)>1;
     for(const root of document.querySelectorAll('.main > .view:not([hidden]),.np:not([hidden]),.sheet'))if(root.scrollWidth>root.clientWidth+1)issues.push(`${root.className}: horizontal overflow ${root.scrollWidth}/${root.clientWidth}`);
     for(const toolbar of document.querySelectorAll('.smart-tools,.vh-actions,.sync-panel > .set-inline')){
      if(!toolbar.offsetParent)continue;
      const controls=[...toolbar.children].filter(e=>e.offsetParent && !e.hidden);
      for(let i=0;i<controls.length;i++)for(let j=i+1;j<controls.length;j++)if(intersects(controls[i].getBoundingClientRect(),controls[j].getBoundingClientRect()))issues.push(`overlap in ${toolbar.className}`);
     }
     const title=document.querySelector('.navbar.has-title .nav-title');
     if(title)for(const button of document.querySelectorAll('.navbar button:not([hidden])'))if(intersects(title.getBoundingClientRect(),button.getBoundingClientRect()))issues.push('navigation title overlaps an action');
     return issues;
    });
    issues.push(...found.map(issue=>`${screen}: ${issue}`));
   });
   assert.equal(screens.length,28);assert.deepEqual(issues,[]);assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
 });
}
test('equalizer makes every frequency reachable without shrinking touch targets',async()=>{
 const app=await openApp({config,phone:'small'});
 try{const {page}=app;await page.locator('.stage-cta').click();await page.locator('.player .now').click();await page.locator('.np-quick button:nth-child(3)').click();await page.locator('.eq-band-nav:not([hidden])').waitFor();await page.waitForTimeout(300);
 const next=page.getByRole('button',{name:'Следующие частоты',exact:true});await next.click();await page.waitForFunction(()=>{const c=document.querySelector('.eq-sheet .eq-bands'),last=c.lastElementChild;return last.getBoundingClientRect().right<=c.getBoundingClientRect().right+1;});
 const last=page.locator('.eq-sheet .eq-slider').last();await last.focus();await page.keyboard.press('ArrowUp');await page.waitForFunction(()=>window.__mock.state.eq?.[9]===1);
 await page.getByRole('button',{name:'Предыдущие частоты',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.eq-sheet .eq-bands').scrollLeft===0);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

for (const colorScheme of ['dark', 'light']) {
 test(`phone dialog actions remain legible in ${colorScheme} mode`, async () => {
  const app = await openApp({config, phone:'small', colorScheme});
  try {
   const {page} = app;
   await page.locator('.dock-tab[data-tab="library"]').click();
   await page.locator('.lib-summary.smart').click();
   await page.locator('.smart-tools .btn').first().click();
   const save = page.locator('dialog[open] .dialog-actions .btn.primary');
   await save.waitFor();
   assert.equal(await save.isEnabled(), true);
   const style = await save.evaluate(button => {
    const css = getComputedStyle(button);
    return {background:css.backgroundColor, image:css.backgroundImage};
   });
   assert.deepEqual(style, {background:'rgba(0, 0, 0, 0)', image:'none'});
   assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
 });
}
