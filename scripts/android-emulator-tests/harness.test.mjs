import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const exec = promisify(execFile);
const moduleUrl = new URL('./lib.mjs', import.meta.url).href;
async function withoutDevice(source, check) {
 const dir=await mkdtemp(path.join(tmpdir(),'pm-no-device-'));
 try {
  await writeFile(path.join(dir,'adb'),'#!/bin/sh\necho "test: no device" >&2\nexit 1\n',{mode:0o700});
  await exec(process.execPath,['--input-type=module','-e',source],{env:{...process.env,PATH:`${dir}:${process.env.PATH}`,PM_OUT:dir,GITHUB_STEP_SUMMARY:''},timeout:10000});
  await check(dir);
 } finally {await rm(dir,{recursive:true,force:true});}
}
test('losing adb makes the failed step critical and skips dependent checks',()=>withoutDevice(`
 const {step,finish}=await import(${JSON.stringify(moduleUrl)});
 await step('lost device',()=>{throw new Error('page closed');});
 await step('dependent check',()=>{throw new Error('must be skipped');});
 if(finish())throw new Error('broken device must fail');
`,async dir=>{
 const results=JSON.parse(await readFile(path.join(dir,'results.json'),'utf8'));
 assert.equal(results[0].status,'fail');assert.equal(results[0].critical,true);
 assert.match(results[0].detail,/adb device unavailable/);assert.equal(results[1].status,'skip');
}));
test('failed log retrieval rejects instead of returning an empty clean log',()=>withoutDevice(`
 const {logcat}=await import(${JSON.stringify(moduleUrl)});
 let failed=false;try{await logcat();}catch{failed=true;}
 if(!failed)throw new Error('missing logs cannot mean clean');
`,async()=>{}));
