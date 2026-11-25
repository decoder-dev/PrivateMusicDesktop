import assert from "node:assert/strict";
import test from "node:test";
import { createCatalogueGate } from "../ui/js/catalogue-gate.js";

test("catalogue reads stay within two at a time and all finish", async () => {
  const gate = createCatalogueGate(2);
  let active = 0;
  let peak = 0;
  const seen = [];
  await Promise.all([0, 1, 2, 3, 4].map((n) => gate(async () => {
    active += 1;
    peak = Math.max(peak, active);
    await new Promise((resolve) => setTimeout(resolve, 20));
    active -= 1;
    seen.push(n);
  })));
  assert.equal(peak, 2);
  assert.deepEqual(seen.sort(), [0, 1, 2, 3, 4]);
});

test("a rejected catalogue read releases the next waiter", async () => {
  const gate = createCatalogueGate(1);
  const first = gate(async () => { throw new Error("vk"); });
  let second = false;
  const next = gate(async () => { second = true; });
  await assert.rejects(first, /vk/);
  await next;
  assert.equal(second, true);
});
