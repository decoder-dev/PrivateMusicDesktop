import assert from 'node:assert/strict';
import test from 'node:test';
import {openApp} from './harness.js';

test('damaged cross-account offline metadata blocks eviction without crashing or losing valid protection',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {sharedProtection}=await import('/js/playlist-sync.js');
   localStorage.setItem('pm.personal.99999.playlistSync',JSON.stringify([null,{wanted:'invalid',owned:['7_1']},{wanted:['7_2',{}]}]));
   localStorage.setItem('pm.personal.99999.manualDownloads',JSON.stringify(['7_3',null]));
   const p=sharedProtection();return {complete:p.complete,manual:[...p.manual],protected:[...p.protectedKeys].sort()};
  });
  assert.equal(result.complete,false);assert.ok(result.manual.includes('7_3'));
  assert.ok(result.protected.includes('7_1'));assert.ok(result.protected.includes('7_2'));
 } finally {await app.close();}
});

test('sleep timer rejects stale initial reads, rolls back a failed change and ignores replies after disposal',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {api}=await import('/js/api.js');const {createSleepTimer}=await import('/js/sleep-timer.js');
   const {withScope,publish,store}=await import('/js/store.js');let resolve;
   api.sleepTimer=()=>new Promise(done=>{resolve=done;});
   const first=withScope(()=>createSleepTimer());
   store.sleep={remaining:900,end_of_track:false};publish('sleep');
   resolve([null,false]);await Promise.resolve();await Promise.resolve();
   const stale=first.value.querySelector('select').value;
   api.setSleepTimer=async()=>{throw {kind:'network'};};
   const select=first.value.querySelector('select');select.value='1800';select.dispatchEvent(new Event('change'));
   await Promise.resolve();await Promise.resolve();const failed=select.value;
   first.dispose();
   const second=withScope(()=>createSleepTimer());second.dispose();
   resolve([900,false]);await Promise.resolve();await Promise.resolve();
   return {stale,failed,disposed:second.value.querySelector('.set-note').textContent};
  });
  assert.deepEqual(result,{stale:'900',failed:'900',disposed:''});
 } finally {await app.close();}
});

test('late playlist data preserves creation, rename, counts and deletion during the request',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {api}=await import('/js/api.js');const p=await import('/js/my-playlists.js');
   p.forgetPlaylists();let resolve;
   api.playlists=()=>new Promise(done=>{resolve=done;});
   const request=p.ensurePlaylists();
   const playlist=id=>({owner_id:7,playlist_id:id,title:`Playlist ${id}`,count:2});
   p.addPlaylist(playlist(99));p.replacePlaylist({...playlist(1),title:'Renamed'});
   p.changeCount(playlist(1),1);p.changeCount(playlist(2),1);p.changeCount(playlist(2),1);p.dropPlaylist(playlist(3));
   resolve({items:[playlist(1),playlist(2),playlist(3)],total:3,next_offset:null});
   await request;return p.allPlaylists();
  });
  assert.ok(result.some(p=>p.playlist_id===99));
  assert.equal(result.find(p=>p.playlist_id===1)?.title,'Renamed');
  assert.equal(result.find(p=>p.playlist_id===1)?.count,3);
  assert.equal(result.find(p=>p.playlist_id===2)?.count,4);
  assert.equal(result.some(p=>p.playlist_id===3),false);
 } finally {await app.close();}
});

test('late followed-album data agrees with confirmed follow and unfollow decisions',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {api}=await import('/js/api.js');const albums=await import('/js/my-albums.js');
   const followed={owner_id:7,album_id:1,title:'Keep',is_followed:false};
   const removed={owner_id:7,album_id:2,title:'Remove',is_followed:true};
   albums.forgetAlbums();let resolve;
   api.followedAlbums=()=>new Promise(done=>{resolve=done;});api.followAlbum=async()=>{};
   const request=albums.ensureAlbums();
   await albums.toggleFollow(followed);await albums.toggleFollow(removed);
   resolve({items:[removed],total:1,next_offset:null});await request;
   return {ids:albums.followedAlbums().map(a=>a.album_id),followed:albums.isFollowed(followed),removed:albums.isFollowed(removed)};
  });
  assert.deepEqual(result,{ids:[1],followed:true,removed:false});
 } finally {await app.close();}
});

test('an older downloads refresh cannot resurrect files after a newer snapshot',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {api}=await import('/js/api.js');const d=await import('/js/downloads.js');
   const deferred=[];api.downloads=()=>new Promise(resolve=>deferred.push(resolve));
   const older=d.refreshDownloads(),newer=d.refreshDownloads();
   deferred[1]({tracks:[],bytes:0});await newer;
   deferred[0]({tracks:[{owner_id:7,track_id:999}],bytes:123});await older;
   return {count:d.downloads().tracks.length,bytes:d.downloads().bytes};
  });
  assert.deepEqual(result,{count:0,bytes:0});
 } finally {await app.close();}
});

test('download batches stop on account changes even when the remaining files are already cached',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {api}=await import('/js/api.js');const d=await import('/js/downloads.js');
   const {store,set}=await import('/js/store.js');
   const one={owner_id:7,track_id:9901},two={owner_id:7,track_id:9902};
   api.downloads=async()=>({tracks:[two],bytes:100});await d.refreshDownloads();
   let resolve;api.download=()=>new Promise(done=>{resolve=done;});
   const request=d.downloadAll([one,two]);
   while(!resolve)await Promise.resolve();
   set('session',{...store.session,profile:{...store.session.profile,id:99999}});
   resolve();const result=await request;
   return {cancelled:result.cancelled,other:JSON.parse(localStorage.getItem('pm.personal.99999.manualDownloads')??'[]')};
  });
  assert.deepEqual(result,{cancelled:true,other:[]});
 } finally {await app.close();}
});

test('disposed segmented controls stop watching detached tabs and cancel pending animation frames',async()=>{
 const app=await openApp({config:{signedIn:true}});
 try {
  const result=await app.page.evaluate(async()=>{
   const {watchSegmented}=await import('/js/ui-parts.js');const {withScope}=await import('/js/store.js');
   const tabs=document.createElement('div');tabs.innerHTML='<button aria-selected="true">One</button><button aria-selected="false">Two</button>';
   document.body.append(tabs);
   const scoped=withScope(()=>watchSegmented(tabs));scoped.dispose();
   tabs.children[0].setAttribute('aria-selected','false');tabs.children[1].setAttribute('aria-selected','true');
   await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
   const result=tabs.classList.contains('ready');tabs.remove();return result;
  });
  assert.equal(result,false);
 } finally {await app.close();}
});
