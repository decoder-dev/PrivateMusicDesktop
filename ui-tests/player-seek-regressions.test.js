import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

const config = { signedIn: true, platform: "android", catalogMixes: true };

async function startTrack(page) {
  await page.locator(".stage-cta").click();
  await page.locator(".player:not(.is-empty)").waitFor();
}

test("the scrubber does not flash the pre-seek position while the command is pending", async () => {
  const app = await openApp({ config, phone: "small" });
  try {
    const { page } = app;
    await startTrack(page);
    await page.locator(".player .now").click();
    const lane = page.locator(".np .np-seek input");
    await lane.waitFor();
    await page.waitForTimeout(400);
    await page.evaluate(() => {
      const invoke = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = (command, args) => {
        if (command === "seek") return new Promise(resolve => {
          window.__finishSeek = () => {
            window.__mock.state.player.position = args.seconds;
            window.__mock.emitState();
            resolve();
          };
        });
        return invoke(command, args);
      };
    });
    const box = await lane.boundingBox();
    const max = Number(await lane.getAttribute("max"));
    await page.touchscreen.tap(box.x + box.width * 0.75, box.y + box.height / 2);
    await page.waitForFunction(() => !!window.__finishSeek);
    await page.evaluate(() => {
      window.__mock.state.player.position = 0;
      window.__mock.emitState();
    });
    assert.ok(Math.abs(Number(await lane.inputValue()) - max * 0.75) <= max * 0.03);
    await page.evaluate(() => window.__finishSeek());
    await page.waitForFunction(() => Math.abs(window.__mock.state.player.position - window.__mock.last("seek").args.seconds) < 0.01);
    if (process.env.PM_SCREENSHOTS) await page.screenshot({ path: `${process.env.PM_SCREENSHOTS}/phone-seek-confirmed.png` });
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("the compact player timeline keeps advancing while its slider has focus", async () => {
  const app = await openApp({ config });
  try {
    const { page } = app;
    await page.locator(".main .row").first().dblclick();
    const slider = page.locator(".mini-player input[type=range]");
    await slider.focus();
    await page.evaluate(() => {
      window.__mock.state.player.duration = 300;
      window.__mock.state.player.position = 42;
      window.__mock.emitState();
    });
    assert.equal(Number(await slider.inputValue()), 42);
    assert.equal(await page.locator(".mini-time").textContent(), "0:42");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("an upward swipe that starts on the mini player cover opens Now Playing", async () => {
  const app = await openApp({ config, phone: "small" });
  try {
    const { page } = app;
    await startTrack(page);
    const box = await page.locator(".player .cover-button").boundingBox();
    const x = box.x + box.width / 2;
    const y = box.y + box.height / 2;
    const cdp = await page.context().newCDPSession(page);
    try {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y, id: 1 }] });
      for (let i = 1; i <= 5; i += 1) {
        await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y - i * 16, id: 1 }] });
      }
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    } finally { await cdp.detach(); }
    await page.locator(".np:not([hidden])").waitFor();
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});
