import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

test("a catalog mix rejected by VK starts untuned and keeps that continuation", async () => {
  const app = await openApp({ config: { signedIn: true, platform: "android", catalogMixes: true }, phone: "small" });
  try {
    const { page } = app;
    await page.locator(".mix-tile").nth(1).waitFor();
    await page.evaluate(() => {
      const invoke = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = (command, args) => {
        if (command === "stream_mix" && args.options) throw { kind: "vk", message: "VK error 8: bad request" };
        return invoke(command, args);
      };
    });
    await page.locator(".mix-tile").nth(1).click();
    await page.waitForFunction(() => window.__mock.calls.some(call => call.command === "play_radio"));
    const calls = await page.evaluate(() => window.__mock.calls.filter(call => call.command === "stream_mix").map(call => call.args.options));
    assert.deepEqual(calls, ['{"mood":["calm"]}', null]);
    assert.equal(await page.evaluate(() => window.__mock.last("play_radio").args.source.options), null);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});
