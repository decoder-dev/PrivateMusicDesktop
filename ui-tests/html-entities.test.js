import assert from 'node:assert/strict';
import test from 'node:test';
import {decodeEntities} from '../ui/js/html-entities.js';
test('lyrics character references are decoded once and unknown ones are kept',()=>{
 assert.equal(decodeEntities("You&#x27;re &amp; I&#39;m &quot;here&quot; &#X41;&#66;"),"You're & I'm \"here\" AB");
 assert.equal(decodeEntities("&amp;lt; &amp;#x27;"),"&lt; &#x27;");
 assert.equal(decodeEntities("AT&T; R&B & a;b &unknown; &#xZZ; &#1114112; &#55357; &"),"AT&T; R&B & a;b &unknown; &#xZZ; &#1114112; &#55357; &");
 assert.equal(decodeEntities("<b>тег</b> &#1090;"),"<b>тег</b> т");assert.equal(decodeEntities(null),"");
});
