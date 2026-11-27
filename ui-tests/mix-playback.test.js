import assert from "node:assert/strict";
import test from "node:test";
import { loadMixTracks, mixOptionsRejected } from "../ui/js/mix-playback.js";

const mix = { id: "artist-mix", options: '{"curator":"anya"}' };

test("a rejected tuned mix retries untuned and continues with the working options", async () => {
  const calls = [];
  const tracks = [{ track_id: 1 }];
  const result = await loadMixTracks(mix, async (id, options) => {
    calls.push({ id, options });
    if (options) throw { kind: "vk", message: "VK error 8: bad request" };
    return tracks;
  });
  assert.deepEqual(result, { tracks, options: null });
  assert.deepEqual(calls, [
    { id: "artist-mix", options: mix.options },
    { id: "artist-mix", options: null },
  ]);
});

test("an empty tuned response also falls back to the plain mix", async () => {
  const tracks = [{ track_id: 2 }];
  const result = await loadMixTracks(mix, async (_id, options) => options ? [] : tracks);
  assert.deepEqual(result, { tracks, options: null });
});

test("network and service failures are not hidden by a second request", async () => {
  const failure = { kind: "offline", message: "No internet connection." };
  let calls = 0;
  await assert.rejects(loadMixTracks(mix, async () => { calls += 1; throw failure; }), error => error === failure);
  assert.equal(calls, 1);
  assert.equal(mixOptionsRejected(failure), false);
});

test("an accepted tuned mix keeps its options for radio continuation", async () => {
  const tracks = [{ track_id: 3 }];
  const result = await loadMixTracks(mix, async () => tracks);
  assert.deepEqual(result, { tracks, options: mix.options });
});
