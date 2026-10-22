import assert from "node:assert/strict";
import test from "node:test";
import { parseImport, bestImportMatch } from "../ui/js/playlist-import.js";

test("import parser: BOM, quoted CSV fields, embedded newlines and duplicates",()=>{
  assert.deepEqual(parseImport('\uFEFFTrack Name,Artist Name(s)\r\n"A, B","First;Second"\r\n"Say ""Hi""","An\nArtist"\r\n"A, B","First;Second"'),[
    {title:"A, B",artist:"First, Second"}, {title:'Say "Hi"',artist:"An Artist"},
  ]);
});
test("import parser: numbered text, tabs, semicolon CSV, title-only lines and bounds",()=>{
  assert.deepEqual(parseImport('1. Singer — Song\n2) Singer - Other\nSolo\nSinger\tThird'),[
    {artist:"Singer",title:"Song"},{artist:"Singer",title:"Other"},{artist:"",title:"Solo"},{artist:"Singer",title:"Third"},
  ]);
  assert.deepEqual(parseImport('Исполнитель;Название\nПевец;Песня'),[{artist:"Певец",title:"Песня"}]);
  assert.equal(parseImport(Array.from({length:600},(_,i)=>`Title ${i}`).join('\n')).length,500);
  assert.deepEqual(parseImport('\n  \n'),[]);
});
test("import matcher: reject wrong artists, prefer exact songs, do not auto-select a recording variant",()=>{
  const item={artist:"Singer",title:"Song"};
  assert.equal(bestImportMatch(item,[{artist:"Other",title:"Song"}]),null);
  const exact={artist:"Singer",title:"Song"};
  assert.equal(bestImportMatch(item,[{artist:"Singer",title:"Song Live"},exact]).track,exact);
  assert.ok((bestImportMatch(item,[{artist:"Singer",title:"Song Live"}])?.confidence??0)<0.9);
  assert.ok(bestImportMatch({artist:"",title:"Song"},[exact]).confidence>=0.9);
});
