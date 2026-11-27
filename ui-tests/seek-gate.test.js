import assert from "node:assert/strict";
import test from "node:test";
import { createSeekGate } from "../ui/js/seek-gate.js";

test("an old state packet cannot replace the requested position", async () => {
  const gate = createSeekGate(() => {});
  gate.preview();
  assert.equal(gate.shouldRender(10), false);
  await gate.commit(90, async () => {});
  assert.equal(gate.shouldRender(10), false);
  assert.equal(gate.shouldRender(90), true);
  assert.equal(gate.shouldRender(91), true);
});

test("an invoke acknowledgement alone does not release the scrubber", async () => {
  const gate = createSeekGate(() => {});
  await gate.commit(75, async () => {});
  assert.equal(gate.shouldRender(4), false);
});

test("a failed or unanswered seek returns to the real position after the timeout", async () => {
  let notified = 0;
  const gate = createSeekGate(() => { notified += 1; }, { timeout: 5 });
  await gate.commit(80, async () => { throw new Error("seek failed"); }).catch(() => {});
  assert.equal(gate.shouldRender(8), false);
  await new Promise(resolve => setTimeout(resolve, 15));
  assert.equal(gate.shouldRender(8), true);
  assert.ok(notified >= 1);
});

test("an older request cannot clear a newer gesture", async () => {
  const gate = createSeekGate(() => {});
  let finish;
  const first = gate.commit(30, () => new Promise(resolve => { finish = resolve; }));
  gate.preview();
  finish();
  await first;
  assert.equal(gate.shouldRender(12), false);
});

test("changing tracks cancels the pending position", async () => {
  const gate = createSeekGate(() => {});
  await gate.commit(80, async () => {});
  gate.track("7_2");
  assert.equal(gate.shouldRender(0), true);
});
