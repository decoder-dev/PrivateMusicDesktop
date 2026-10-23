import test from "node:test";
import assert from "node:assert/strict";
import {radioSnapshot} from "../ui/js/radio-snapshot.js";

test("radio bookmarks accept bounded source options and strip unrelated source fields",()=>{
  assert.deepEqual(radioSnapshot({source:{kind:'personal',options:null,access_token:'secret'},next_offset:700}),{source:{kind:'personal',options:null},next_offset:700});
  assert.deepEqual(radioSnapshot({source:{kind:'catalog',mix_id:'saved',options:'{ "mood": ["calm"] }'},next_offset:800}),{source:{kind:'catalog',mix_id:'saved',options:'{"mood":["calm"]}'},next_offset:800});
});
test("radio bookmarks reject malformed, excessive or unsupported local data",()=>{
  for(const value of [null,{}, {source:{kind:'personal'},next_offset:-1},{source:{kind:'personal'},next_offset:1e9},
    {source:{kind:'unknown'},next_offset:0},{source:{kind:'catalog',mix_id:'\n'},next_offset:0},
    {source:{kind:'personal',options:'[1]'},next_offset:0},{source:{kind:'personal',options:'no json'},next_offset:0},
    {source:{kind:'personal',options:' '.repeat(8193)},next_offset:0},{source:{kind:'similar',seed:{owner_id:7,track_id:0}},next_offset:0}])assert.equal(radioSnapshot(value),null);
});
test("similar radio stores only track metadata with no playback URL or credential field",()=>{
  const result=radioSnapshot({source:{kind:'similar',seed:{owner_id:7,track_id:1,title:'A',artist:'B',duration:12,stream_url:'https://private.invalid',access_token:'secret'}},next_offset:0});
  assert.equal(result.source.seed.stream_url,null);
  assert.equal(result.source.seed.access_token,undefined);
});
