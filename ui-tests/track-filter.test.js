import assert from 'node:assert/strict';
import test from 'node:test';
import {filterItems,filterWords,matchesWords} from '../ui/js/track-filter.js';
test('library filter: words match in any order across title and artist, case and ё are ignored',()=>{
 const song={title:'Группа крови',artist:'Кино'};
 const find=(q,t=song)=>matchesWords(filterWords(q),t.title,t.artist);
 assert.equal(find(''),true);assert.equal(find('   '),true);
 assert.equal(find('кино'),true);assert.equal(find('КИНО группа'),true);assert.equal(find('крови  кино'),true);
 assert.equal(find('кино звезда'),false);assert.equal(find('рок'),false);
 assert.equal(find('елка',{title:'Ёлка',artist:'Тест'}),true);assert.equal(find('рок',{title:'Рок-н-ролл мёртв',artist:'Пилот'}),true);
 assert.equal(find('rock',{title:'Rock & Roll',artist:null}),true);

 const items=[{title:'Трек 1',artist:'Исполнитель 5'},{title:'Трек 15',artist:'Исполнитель 1'},{title:'Группа крови',artist:'Кино'}];
 const pick=q=>filterItems(items,filterWords(q),i=>[i.title,i.artist]).map(i=>i.title);
 assert.deepEqual(pick('исполнитель 5'),['Трек 1']);
 assert.deepEqual(pick('кино группа'),['Группа крови']);
 assert.deepEqual(pick(''),items.map(i=>i.title));assert.deepEqual(pick('нет такого'),[]);
});
