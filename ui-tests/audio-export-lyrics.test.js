import assert from 'node:assert/strict';
import test from 'node:test';
import {exportLyrics} from '../ui/js/audio-export-lyrics.js';
test('audio export keeps plain lyrics and converts timed lines to a bounded LRC sidecar',()=>{
 assert.equal(exportLyrics(null),null);
 assert.deepEqual(exportLyrics({text:'Words',lines:[{time:0,text:'First'},{time:63.456,text:'Next\nline'},{time:-1,text:'Bad'}]}),{text:'Words',lrc:'[00:00.00]First\n[01:03.46]Next line'});
 assert.deepEqual(exportLyrics({text:'Words'}),{text:'Words',lrc:''});
 const result=exportLyrics({text:'Words',lines:Array.from({length:10000},(_,i)=>({time:i,text:'x'.repeat(100)}))});assert.equal(result.lrc,'');
});
