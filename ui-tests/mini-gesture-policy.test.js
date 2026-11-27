import assert from "node:assert/strict";
import test from "node:test";
import { gestureAction, isMiniGestureTarget } from "../ui/js/mobile/mini-gesture-policy.js";

test("an upward cover gesture opens the full player", () => {
  assert.equal(gestureAction({ x: 3, y: -70 }, { x: 3, y: -70 }), "open");
  const cover = { classList: { contains: name => name === "cover-button" } };
  const target = { closest: () => cover };
  assert.equal(isMiniGestureTarget(target), true);
});

test("horizontal swipes keep their transport meaning", () => {
  assert.equal(gestureAction({ x: -90, y: 2 }, { x: -90, y: 2 }), "next");
  assert.equal(gestureAction({ x: 90, y: 2 }, { x: 90, y: 2 }), "previous");
});

test("play, next and links do not become gesture starters", () => {
  const button = { classList: { contains: () => false } };
  const target = { closest: () => button };
  assert.equal(isMiniGestureTarget(target), false);
});

test("short and downward movements have no action and need no click suppression", () => {
  assert.equal(gestureAction({ x: 5, y: 10 }, { x: 5, y: 10 }), null);
  assert.equal(gestureAction({ x: 2, y: 70 }, { x: 2, y: 70 }), null);
});
