import assert from 'node:assert/strict';
import test from 'node:test';
import {randomPlaybackId} from '../ui/js/random.js';
import {markdownCell} from '../scripts/android-emulator-tests/report-format.mjs';

test('playback IDs reject both endpoints and retain the complete supported range', () => {
 const samples = [0, 1, 0xfffffffe, 0xffffffff, 2];
 let draws = 0;
 assert.equal(randomPlaybackId(values => {values[0] = samples[draws++];}), 1);
 assert.equal(draws, samples.length);
 assert.equal(randomPlaybackId(values => {values[0] = 0xfffffffd;}), 2147483646);
});

test('device report cells cannot inject table columns, links, images or HTML', () => {
 const result = markdownCell('\\|![click](https://example.invalid) <img src=x> &\r\nnext');
 assert.equal(result, '&#92;&#124;&#33;&#91;click&#93;(https://example.invalid) &#60;img src=x&#62; &#38;  next');
 assert.doesNotMatch(result, /[\\|\r\n<>\[\]!]/);
});

test('device report text is bounded without splitting Unicode characters or entities', () => {
 assert.equal(markdownCell('😀'.repeat(301)), '😀'.repeat(300));
 assert.equal(markdownCell('x'.repeat(299) + '|trailing'), 'x'.repeat(299) + '&#124;');
 assert.equal(markdownCell(null), '');
});
