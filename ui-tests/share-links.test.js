import test from "node:test";
import assert from "node:assert/strict";
import {trackShareLink,collectionShareLink} from "../ui/js/share-links.js";

test("sharing builds VK locators without copying playback URLs or session fields",()=>{
  const track={owner_id:-7,track_id:15,title:'A',stream_url:'https://secret.invalid/file',access_token:'secret'};
  assert.equal(trackShareLink(track),'https://vk.com/audio-7_15');
  assert.equal(collectionShareLink('album',{owner_id:-12,album_id:4,access_key:'aBc123'}),'https://vk.com/music/album/-12_4?access_key=aBc123');
  assert.equal(collectionShareLink('playlist',{owner_id:7,playlist_id:2}),'https://vk.com/music/playlist/7_2');
});
test("sharing rejects invalid IDs, arbitrary URL-like access keys and unknown collection kinds",()=>{
  for(const track of [{owner_id:0,track_id:1},{owner_id:2,track_id:-1},{owner_id:Infinity,track_id:1},{owner_id:7,track_id:'2'},null])assert.equal(trackShareLink(track),null);
  assert.equal(collectionShareLink('playlist',{owner_id:7,playlist_id:2,access_key:'https://secret.invalid'}),null);
  assert.equal(collectionShareLink('other',{owner_id:7,album_id:2}),null);
});

test("a copied playlist access key survives opening the link and conflicting keys are rejected",async()=>{
  const {playlistLink}=await import('../ui/js/playlist-link.js');
  const url=collectionShareLink('playlist',{owner_id:7,playlist_id:3,access_key:'aBc123'});
  assert.equal(playlistLink(url).access_key,'aBc123');
  assert.throws(()=>playlistLink('https://vk.com/music/playlist/7_3_one?access_key=two'));
  assert.throws(()=>playlistLink('https://vk.com/music/playlist/7_3?access_key=https://evil.invalid'));
});
