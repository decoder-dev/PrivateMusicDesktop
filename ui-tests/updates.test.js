import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';
async function settings(app){const {page}=app;await page.locator('.dock-tab[data-tab="profile"]').click();await page.locator('[data-row="settings"]').click();await page.locator('.update-panel').scrollIntoViewIfNeeded();}
test('Android update button checks, downloads and opens the installer once',async()=>{
 const app=await openApp({config:{signedIn:true,platform:'android',updateDelay:250},phone:'small'});
 try{await settings(app);const {page}=app;await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();await page.waitForFunction(()=>window.__mock.count('update_open')===1);assert.equal(await page.evaluate(()=>window.__mock.count('update_check')),1);assert.equal(await page.evaluate(()=>window.__mock.count('update_download')),1);assert.equal(await page.locator('.update-access').isVisible(),false);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test('Android install permission denial keeps the verified APK and offers retry',async()=>{
 const app=await openApp({config:{signedIn:true,platform:'android'},phone:'small'});
 try{await settings(app);const {page}=app;await page.evaluate(async()=>{const {api}=await import('/js/api.js');const original=api.updateOpen;let denied=false;api.updateOpen=()=>{if(!denied){denied=true;throw 'install_permission';}return original();};});await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();await page.locator('.update-error:not([hidden])').waitFor();assert.match(await page.locator('.update-error').innerText(),/Разрешите установку/);await page.getByRole('button',{name:'Установить обновление',exact:true}).click();await page.waitForFunction(()=>window.__mock.count('update_open')===1);assert.equal(await page.evaluate(()=>window.__mock.count('update_download')),1);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test('cancelled or unverified updates never launch installation',async()=>{
 for(const result of ['cancelled','error']){
  const app=await openApp({config:{signedIn:true,platform:'android'},phone:'small'});
  try{await settings(app);const {page}=app;await page.evaluate(async result=>{const {api}=await import('/js/api.js');api.updateDownload=async()=>({phase:result,error:result==='error'?'checksum':'',current:'0.6.5',version:'0.6.6'});},result);await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.update-actions button').disabled);assert.equal(await page.evaluate(()=>window.__mock.count('update_open')),0);assert.equal(await page.getByRole('button',{name:'Установить обновление',exact:true}).isVisible(),false);assert.deepEqual(app.problems,[]);}finally{await app.close();}
 }
});
test('a command failure after a progress event leaves checking retryable',async()=>{
 const app=await openApp({config:{signedIn:true,platform:'android'},phone:'small'});
 try{await settings(app);const {page}=app;await page.evaluate(async()=>{const {api}=await import('/js/api.js');api.updateCheck=async()=>{window.__mock.emit('pm-update',{phase:'checking',current:'0.6.5'});throw 'network';};});await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();await page.locator('.update-error:not([hidden])').waitFor();assert.equal(await page.getByRole('button',{name:'Проверить обновления',exact:true}).isEnabled(),true);assert.equal(await page.evaluate(()=>window.__mock.count('update_open')),0);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
