// The phone layout: the Android app runs this same UI in a WebView. Everything is
// driven the way a finger does it (touch taps, CDP touch drags) in a Pixel-sized
// Chromium with an Android user agent, against the same mock backend.

import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

// `platform: "android"` is what the backend of the Android build answers in app_info.
const CONFIG = { signedIn: true, catalogMixes: true, history: [1, 2, 3], platform: "android" };
const phoneApp = async (options = {}) => {
  const app = await openApp({ config: CONFIG, phone: true, ...options });
  await app.page.waitForSelector(".dock");
  await app.page.waitForFunction(() => document.querySelector(".home-view .stage"));
  return app;
};
const visible = (page, selector) => page.locator(selector).locator("visible=true");
const tap = async (page, selector) => {
  // A sheet ignores taps for a moment after it opens (the finger that opened it may still be down).
  if (selector.includes(".sheet")) await page.waitForTimeout(300);
  await visible(page, selector).first().tap();
  await page.waitForTimeout(120);
};
const see = (page, selector) => visible(page, selector).first().waitFor();
/** The Android Back button: true when the page handled it (preventDefault was called). */
const pressBack = (page) => page.evaluate(() => !window.dispatchEvent(new CustomEvent("pm-back", { cancelable: true })));
const tab = (page, id) => tap(page, `.dock-tab[data-tab="${id}"]`);
const view = (page) => page.evaluate(() => document.querySelector(".main").dataset.view);
const title = (page) => page.locator(".mini-title, .player .title").first().innerText();

/** A touch drag through CDP: real touch events, so pointer events, scrolling and gestures behave as on a device. */
async function drag(page, from, to, { steps = 8, hold = 0, end = true } = {}) {
  const cdp = await page.context().newCDPSession(page);
  const send = (type, x, y) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y, id: 1 }] });
  await send("touchStart", from.x, from.y);
  if (hold) await page.waitForTimeout(hold);
  for (let i = 1; i <= steps; i++) {
    await send("touchMove", from.x + ((to.x - from.x) * i) / steps, from.y + ((to.y - from.y) * i) / steps);
    await page.waitForTimeout(12);
  }
  if (end) { await send("touchEnd"); await cdp.detach(); return null; }
  // Still down: the caller lifts the finger with release().
  return { release: async () => { await send("touchEnd"); await cdp.detach(); } };
}
/** Every CSS rule of the page, @imports and media blocks included. */
const allRules = () => {
  const out = [];
  const walk = (rules) => { for (const r of rules) { if (r.styleSheet) walk(r.styleSheet.cssRules); else if (r.cssRules && !r.selectorText) walk(r.cssRules); else out.push(r); } };
  for (const sheet of document.styleSheets) walk(sheet.cssRules);
  return out;
};
const center = async (page, selector) => {
  const box = await visible(page, selector).first().boundingBox();
  return { x: box.x + box.width / 2, y: box.y + box.height / 2, box };
};
async function playFromHome(page) {
  await tap(page, ".stage-cta");
  await page.waitForSelector(".player:not(.is-empty)");
}
const openLibraryTracks = async (page) => {
  await tab(page, "library");
  await tap(page, '.lib-sections [data-section="tracks"]');
  await page.waitForFunction(() => document.querySelectorAll(".library-hub .row").length > 3);
};

// ------------------------------------------------------------------ layout

test("phone: platform and layout are set on <html>, the sidebar gives way to the dock", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    const attrs = await page.evaluate(() => ({ ...document.documentElement.dataset }));
    assert.equal(attrs.platform, "android");
    assert.equal(attrs.layout, "phone");
    assert.equal(attrs.orientation, "portrait");
    assert.equal(await page.locator(".sidebar").isVisible(), false);
    assert.equal(await page.locator(".dock").isVisible(), true);
    assert.deepEqual(await page.locator(".dock-tab").evaluateAll((nodes) => nodes.map((n) => n.dataset.tab)), ["home", "library", "profile", "search"]);
    assert.equal(await page.locator('meta[name="theme-color"]').getAttribute("content"), "#ffffff");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("wide layout is untouched: no dock, no navigation bar, the sidebar stays", async () => {
  // A desktop backend sends no `platform` in app_info.
  const app = await openApp({ config: { ...CONFIG, platform: undefined } });
  try {
    const { page } = app;
    await page.waitForSelector(".sidebar");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.layout), "wide");
    assert.equal(await page.locator(".dock").isVisible(), false);
    assert.equal(await page.locator(".navbar").isVisible(), false);
    assert.equal(await page.locator(".sidebar").isVisible(), true);
    // A narrow window with a mouse is still a desktop window.
    await page.setViewportSize({ width: 420, height: 800 });
    assert.equal(await page.evaluate(() => document.documentElement.dataset.layout), "wide");
    // The phone UI can be tried in a desktop browser with ?layout=phone.
    await page.goto(`${app.url}?layout=phone`);
    await page.waitForSelector(".dock");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.layout), "phone");
  } finally { await app.close(); }
});

test("the layout follows the window: a touch device goes wide and back, rotation keeps the phone layout", async () => {
  const app = await phoneApp({ query: "?platform=desktop" });
  try {
    const { page } = app;
    assert.equal(await page.evaluate(() => document.documentElement.dataset.layout), "phone");
    await page.setViewportSize({ width: 1000, height: 700 });
    await page.waitForFunction(() => document.documentElement.dataset.layout === "wide");
    await page.waitForSelector(".sidebar", { state: "visible" });
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length > 3);
    await page.setViewportSize({ width: 412, height: 915 });
    await page.waitForFunction(() => document.documentElement.dataset.layout === "phone");
    await page.waitForSelector(".dock", { state: "visible" });
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("rotation: an Android phone in landscape keeps the phone layout and stays usable", async () => {
  const app = await phoneApp({ phone: "landscape" });
  try {
    const { page } = app;
    const attrs = await page.evaluate(() => ({ ...document.documentElement.dataset }));
    assert.equal(attrs.layout, "phone");
    assert.equal(attrs.orientation, "landscape");
    assert.equal(await page.locator(".dock").isVisible(), true);
    await tab(page, "library");
    await tap(page, '.lib-sections [data-section="tracks"]');
    await page.waitForFunction(() => document.querySelectorAll(".library-hub .row").length > 2);
    await playFromHome(page).catch(async () => { await tab(page, "home"); await playFromHome(page); });
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    const inside = await page.evaluate(() => [".np-top .np-btn", ".np-play", ".np-seek input", ".np-quick button"].every((s) => {
      const r = document.querySelector(s).getBoundingClientRect();
      return r.top >= 0 && r.bottom <= window.innerHeight + 1 && r.right <= window.innerWidth + 1;
    }));
    assert.ok(inside, "the player's controls fit a 412 px high screen");
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    assert.equal(overflow, false);
    await page.setViewportSize({ width: 412, height: 915 });
    await page.waitForFunction(() => document.documentElement.dataset.orientation === "portrait");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.layout), "phone");
  } finally { await app.close(); }
});

// ------------------------------------------------------------- navigation

test("tab dock: Home, Library, Search and Profile open their screens and keep their own stacks", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    assert.equal(await view(page), "home");
    await tab(page, "library");
    assert.equal(await view(page), "libraryHub");
    assert.equal(await page.locator('.dock-tab[data-tab="library"]').getAttribute("aria-current"), "page");
    await tab(page, "search");
    assert.equal(await view(page), "search");
    await tab(page, "profile");
    assert.equal(await view(page), "profile");
    // Profile → Settings is a pushed page; another tab and back to Profile keeps it.
    await tap(page, '[data-row="settings"]');
    assert.equal(await view(page), "settings");
    await tab(page, "home");
    assert.equal(await view(page), "home");
    await tab(page, "profile");
    assert.equal(await view(page), "settings");
    // Tapping the active tab again returns to its first page.
    await tab(page, "profile");
    assert.equal(await view(page), "profile");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("push and pop: the back chevron and the Android Back button go one page back, then let the app leave", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await tab(page, "library");
    await tap(page, '.nav-btn[aria-label="История"]');
    assert.equal(await view(page), "history");
    assert.equal(await page.locator(".nav-back").isVisible(), true);
    await tap(page, ".nav-back");
    assert.equal(await view(page), "libraryHub");
    assert.equal(await page.locator(".nav-back").isVisible(), false);
    await tap(page, '.nav-btn[aria-label="Загрузки"]');
    assert.equal(await view(page), "downloads");
    assert.equal(await pressBack(page), true, "Back pops the page");
    assert.equal(await view(page), "libraryHub");
    assert.equal(await pressBack(page), false, "at a root the press goes on to Android");
    // Pages on pages: an album from search, its artist, then back twice.
    await tab(page, "search");
    await page.locator('input[placeholder^="Исполнитель"]').fill("тест");
    await page.waitForFunction(() => document.querySelectorAll(".row").length > 3);
    await page.locator('input[placeholder^="Исполнитель"]').blur();
    await tap(page, '.tabs [data-tab="albums"]');
    await tap(page, ".card");
    await page.waitForFunction(() => document.querySelector(".main").dataset.view === "album");
    assert.equal(await pressBack(page), true);
    assert.equal(await view(page), "search");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("Back closes what is on top first: an action sheet, a dialog, the player, then the page", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await playFromHome(page);
    await tab(page, "profile");
    await tap(page, '[data-row="stats"]');
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await pressBack(page), true);
    await page.waitForFunction(() => document.querySelector(".np").hidden);
    assert.equal(await view(page), "stats", "the player closed, the page stayed");
    assert.equal(await pressBack(page), true);
    assert.equal(await view(page), "profile");
    // A dialog (sign out confirmation).
    await tap(page, '[data-row="signout"]');
    await page.waitForSelector("dialog[open]");
    assert.equal(await pressBack(page), true);
    await page.waitForFunction(() => !document.querySelector("dialog"));
    assert.equal(await view(page), "profile");
    // An action sheet.
    await tab(page, "home");
    await tap(page, '.home-recs .row .icon-btn[data-action="menu"]');
    await page.waitForSelector(".sheet");
    assert.equal(await pressBack(page), true);
    await page.waitForFunction(() => !document.querySelector(".sheet-root"));
    assert.equal(await view(page), "home");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

// ----------------------------------------------------------------- player

test("mini player: shown above the dock for a track, play/pause and next work, a tap opens the player", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    assert.equal(await page.locator(".player").isVisible(), false);
    await playFromHome(page);
    const [mini, dock] = await page.evaluate(() => ["#app .player", ".dock"].map((s) => { const r = document.querySelector(s).getBoundingClientRect(); return { top: r.top, bottom: r.bottom, height: r.height }; }));
    assert.ok(mini.bottom <= dock.top, "the mini player sits above the dock");
    assert.ok(mini.height >= 56);
    assert.equal(await page.locator(".player .volume").isVisible(), false, "no volume slider on a phone");
    assert.equal(await page.locator(".player .title").innerText(), "Микс 1");
    await tap(page, ".player .controls .icon-btn:nth-child(4)");
    await page.waitForFunction(() => document.querySelector(".player .title").textContent === "Микс 2");
    await tap(page, ".player .controls .play");
    assert.equal(await page.evaluate(() => window.__mock.state.player.status), "paused");
    await tap(page, ".player .titles");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await page.locator(".np input[type=range]").count(), 1, "only the scrubber: no volume slider");
  } finally { await app.close(); }
});

test("mini player: swipe left is next, swipe right is previous, swipe up opens the player (MiniPlayerGesturePolicy)", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await playFromHome(page);
    await tap(page, ".player .controls .icon-btn:nth-child(4)");
    await page.waitForFunction(() => document.querySelector(".player .title").textContent === "Микс 2");
    const { x, y } = await center(page, ".player .titles");
    await drag(page, { x: x + 60, y }, { x: x - 60, y });
    await page.waitForFunction(() => document.querySelector(".player .title").textContent === "Микс 3");
    await drag(page, { x: x - 60, y }, { x: x + 60, y });
    await page.waitForFunction(() => document.querySelector(".player .title").textContent === "Микс 2");
    // A short drag does nothing.
    await drag(page, { x, y }, { x: x - 20, y });
    await page.waitForTimeout(200);
    assert.equal(await page.locator(".player .title").innerText(), "Микс 2");
    await drag(page, { x, y }, { x, y: y - 90 });
    await page.waitForSelector(".np:not([hidden])");
  } finally { await app.close(); }
});

test("full-screen player: a sheet with a grabber; the chevron closes it, a drag down follows the finger and dismisses it", async () => {
  const app = await phoneApp({ reducedMotion: "reduce" });
  try {
    const { page } = app;
    await playFromHome(page);
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await page.locator(".np-grab").isVisible(), true);
    const top = await page.evaluate(() => document.querySelector(".np").getBoundingClientRect().top);
    assert.ok(top >= 0 && top < 60, `the sheet leaves a sliver of the app above it (${top})`);
    // Closing with the button.
    await tap(page, ".np-top .np-btn");
    await page.waitForFunction(() => document.querySelector(".np").hidden);
    // A short drag springs back.
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    const cover = await center(page, ".np-cover");
    await drag(page, { x: cover.x, y: cover.y }, { x: cover.x, y: cover.y + 60 });
    await page.waitForTimeout(450);
    assert.equal(await page.evaluate(() => Math.round(document.querySelector(".np").getBoundingClientRect().top)), Math.round(top));
    // A long one follows the finger, and dismisses on release.
    const finger = await drag(page, { x: cover.x, y: cover.y }, { x: cover.x, y: cover.y + 200 }, { end: false });
    await page.waitForTimeout(100);
    const dragged = await page.evaluate(() => document.querySelector(".np").getBoundingClientRect().top);
    assert.ok(dragged > top + 150, `the sheet followed the finger (${dragged} vs ${top})`);
    await finger.release();
    await page.waitForFunction(() => document.querySelector(".np").hidden);
    // Lyrics and queue fill the sheet; a tap on a lyric line seeks.
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    await tap(page, ".np-controls .np-btn:nth-child(4)"); // the second mix track has timed lyrics
    await tap(page, ".np-quick button:nth-child(1)");
    await page.waitForSelector(".ly-line");
    await tap(page, ".ly-line:nth-child(5)");
    assert.equal(await page.evaluate(() => window.__mock.calls.some((c) => c.command === "seek")), true);
    await tap(page, ".np-quick button:nth-child(2)");
    await page.waitForSelector(".nq-row");
    assert.ok(await page.locator(".nq-row").count() > 3);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("the sheet animates with motion on and is instant with reduced motion", async () => {
  for (const [reducedMotion, animated] of [["no-preference", true], ["reduce", false]]) {
    const app = await phoneApp({ reducedMotion });
    try {
      const { page } = app;
      await playFromHome(page);
      await tap(page, ".player .now");
      await page.waitForSelector(".np:not([hidden])");
      const ms = await page.evaluate(() => parseFloat(getComputedStyle(document.querySelector(".np")).animationDuration) * 1000);
      assert.equal(ms > 100, animated, `${reducedMotion}: animation ${ms} ms`);
      assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".dock")).transitionDuration.split(",")[0]), animated ? "0.22s" : "1e-05s");
    } finally { await app.close(); }
  }
});

// ------------------------------------------------------------------- rows

test("long press on a row opens the track's action sheet; its items run", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await openLibraryTracks(page);
    const row = await center(page, ".library-hub .row");
    await drag(page, { x: row.x, y: row.y }, { x: row.x + 1, y: row.y }, { steps: 1, hold: 650 });
    await page.waitForSelector(".sheet");
    assert.equal(await page.locator(".sheet-title").innerText(), "Трек 1");
    assert.equal(await page.evaluate(() => window.__mock.calls.some((c) => c.command === "play_tracks")), false, "the press did not also play the track");
    assert.ok(await page.locator(".sheet-item").count() >= 4);
    await tap(page, '.sheet-item[data-action="play-next"]');
    await page.waitForFunction(() => !document.querySelector(".sheet-root"));
    await page.waitForTimeout(800); // the tap that follows a long press is swallowed for a moment
    assert.equal(await page.evaluate(() => window.__mock.count("play_next")), 1);
    // The ⋯ button opens the same sheet, and a sub-menu is a page of it.
    await tap(page, '.library-hub .row .icon-btn[data-action="menu"]');
    await page.waitForTimeout(350); // a sheet ignores taps for a moment after it opens (the finger that opened it is still down)
    await tap(page, '.sheet-item[data-action="add-to-playlist"]');
    assert.ok(await page.locator(".sheet-back").isVisible());
    await tap(page, ".sheet-back");
    assert.ok(await page.locator('.sheet-item[data-action="add-to-playlist"]').isVisible());
    // The sheet follows a drag on its grabber and goes away.
    const grab = await center(page, ".sheet-grab");
    await drag(page, grab, { x: grab.x, y: grab.y + 160 });
    await page.waitForFunction(() => !document.querySelector(".sheet-root"));
    // The scrim closes it too.
    await tap(page, '.library-hub .row .icon-btn[data-action="menu"]');
    await page.waitForTimeout(350);
    await page.mouse.click(200, 40);
    await page.waitForFunction(() => !document.querySelector(".sheet-root"));
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("a tap on a row plays it; swipe right is Play next, further right Play last, swipe left likes", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await openLibraryTracks(page);
    const row = await center(page, ".library-hub .row");
    await page.locator(".library-hub .row .titles").first().tap();
    await page.waitForFunction(() => window.__mock.count("play_tracks") === 1);
    const left = row.box.x + 20;
    await drag(page, { x: left, y: row.y }, { x: left + 100, y: row.y });
    await page.waitForFunction(() => window.__mock.count("play_next") === 1);
    await drag(page, { x: left, y: row.y }, { x: left + 230, y: row.y });
    await page.waitForFunction(() => window.__mock.count("play_last") === 1);
    assert.equal(await page.evaluate(() => window.__mock.count("play_tracks")), 1, "a swipe is not a tap");
    // Left: the library track is liked, so this removes it.
    await drag(page, { x: left + 250, y: row.y }, { x: left + 120, y: row.y });
    await page.waitForFunction(() => window.__mock.count("remove_from_library") === 1);
    // A short swipe springs back and does nothing.
    await drag(page, { x: left, y: row.y + 56 }, { x: left + 30, y: row.y + 56 });
    await page.waitForTimeout(150);
    assert.equal(await page.evaluate(() => window.__mock.count("play_next")), 1);
    assert.equal(await page.locator(".row[data-swipe]").count(), 0);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("vertical scrolling of a long list still works with touch momentum and keeps rendering rows", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await openLibraryTracks(page);
    const start = await page.evaluate(() => document.querySelector(".library-hub").scrollTop);
    await drag(page, { x: 200, y: 700 }, { x: 200, y: 250 }, { steps: 12 });
    await page.waitForTimeout(300);
    const after = await page.evaluate(() => document.querySelector(".library-hub").scrollTop);
    assert.ok(after > start + 200, `scrolled (${start} → ${after})`);
    const count = await page.locator(".library-hub .row").count();
    assert.ok(count > 5 && count < 60, `a window of rows (${count})`);
    await page.evaluate(() => { const v = document.querySelector(".library-hub"); v.scrollTop = v.scrollHeight; });
    await page.waitForFunction(() => [...document.querySelectorAll(".row .title")].some((e) => e.textContent === "Трек 1234"));
  } finally { await app.close(); }
});

test("pull to refresh: a drag down at the top of Home reloads it, and the browser's own refresh is off", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await page.waitForFunction(() => window.__mock.count("recommendations") === 1);
    assert.match(await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorY), /none|contain/);
    // A short pull does not refresh.
    await drag(page, { x: 200, y: 200 }, { x: 200, y: 240 });
    await page.waitForTimeout(500);
    assert.equal(await page.evaluate(() => window.__mock.count("recommendations")), 1);
    const finger = await drag(page, { x: 200, y: 200 }, { x: 200, y: 520 }, { steps: 16, end: false });
    assert.equal(await page.locator(".home-view .ptr").evaluate((n) => n.dataset.state), "ready");
    await finger.release();
    await page.waitForFunction(() => window.__mock.count("recommendations") === 2);
    await page.waitForFunction(() => document.querySelector(".home-view .ptr").dataset.state === "idle");
    // Library: the same on the Library tab.
    await tab(page, "library");
    await page.waitForFunction(() => window.__mock.count("playlists") >= 1);
    const before = await page.evaluate(() => window.__mock.count("playlists"));
    await drag(page, { x: 200, y: 200 }, { x: 200, y: 560 }, { steps: 16 });
    await page.waitForFunction((n) => window.__mock.count("playlists") > n, before);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

// ------------------------------------------------------- system integration

test("safe areas: --sat/--sab/--sal/--sar from the shell move the chrome, env() is the fallback", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    const metrics = () => page.evaluate(() => {
      const navbar = document.querySelector(".navbar").getBoundingClientRect();
      const dock = document.querySelector(".dock-tab").getBoundingClientRect();
      return { navbar: Math.round(navbar.height), dockBottom: Math.round(innerHeight - dock.bottom), dockLeft: Math.round(dock.left) };
    });
    const plain = await metrics();
    const settle = () => page.waitForTimeout(250);
    assert.equal(plain.navbar, 44);
    await page.evaluate(() => {
      const root = document.documentElement.style;
      root.setProperty("--sat", "32px"); root.setProperty("--sab", "24px"); root.setProperty("--sal", "10px"); root.setProperty("--sar", "10px");
    });
    await settle();
    const inset = await metrics();
    assert.equal(inset.navbar, 44 + 32);
    assert.equal(inset.dockBottom, plain.dockBottom + 24);
    assert.equal(inset.dockLeft, plain.dockLeft + 10);
    // Content still clears the dock and the mini player.
    await playFromHome(page);
    const clear = await page.evaluate(() => parseFloat(getComputedStyle(document.querySelector(".main > .view:not([hidden])")).paddingBottom));
    assert.ok(clear >= 58 + 24 + 58, `bottom padding ${clear}`);
  } finally { await app.close(); }
});

test("the keyboard: a focused text field hides the dock and the mini player, blur brings them back", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await playFromHome(page);
    await tab(page, "search");
    assert.equal(await page.locator(".dock").evaluate((n) => getComputedStyle(n).opacity), "1");
    const input = page.locator('input[placeholder^="Исполнитель"]');
    assert.equal(await input.getAttribute("enterkeyhint"), "search");
    assert.equal(await input.getAttribute("inputmode"), "search");
    await input.tap();
    await page.waitForFunction(() => document.documentElement.dataset.keyboard === "open");
    await page.waitForFunction(() => getComputedStyle(document.querySelector(".dock")).opacity === "0");
    assert.equal(await page.locator(".player").isVisible(), false);
    assert.equal(await page.locator(".dock-tab").first().evaluate((n) => n.getBoundingClientRect().top > innerHeight - 20 ? true : getComputedStyle(n.closest(".dock")).pointerEvents === "none"), true);
    await input.blur();
    await page.waitForFunction(() => !document.documentElement.dataset.keyboard);
    assert.equal(await page.locator(".player").isVisible(), true);
    await page.waitForFunction(() => getComputedStyle(document.querySelector(".dock")).opacity === "1");
  } finally { await app.close(); }
});

test("Android: desktop-only settings are not offered, and no desktop commands are sent; the player has the visualizer", async () => {
  const app = await openApp({ config: { ...CONFIG, platform: "android" }, phone: true, query: "?platform=android" });
  try {
    const { page } = app;
    await page.waitForSelector(".dock");
    await tab(page, "profile");
    await tap(page, '[data-row="settings"]');
    await page.waitForSelector(".settings .set-body");
    const hidden = await page.locator("[data-desktop-only]").evaluateAll((nodes) => nodes.map((n) => getComputedStyle(n).display));
    assert.ok(hidden.length >= 4, `sections marked desktop-only: ${hidden.length}`);
    assert.ok(hidden.every((d) => d === "none"), "all of them hidden");
    const texts = (await page.locator(".settings").innerText()).toLowerCase();
    for (const word of ["горячие клавиши", "устройство вывода", "визуализация"]) assert.ok(!texts.includes(word), `${word} is not shown`);
    assert.ok(texts.includes("эквалайзер"), "the equaliser stays");
    const calls = await page.evaluate(() => window.__mock.calls.map((c) => c.command));
    for (const command of ["output_devices", "set_output_device", "set_close_to_tray", "set_fullscreen"]) assert.ok(!calls.includes(command), `${command} was not called`);
    await playFromHome(page).catch(async () => { await tab(page, "home"); await playFromHome(page); });
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await page.locator(".np-top .desktop-only").evaluateAll((nodes) => nodes.every((n) => getComputedStyle(n).display === "none")), true);
    // The spectrum comes from the engine on Android too: the mode button is on the phone player, the analyser is asked for.
    assert.equal(await page.locator(".np-top .np-mode").isVisible(), true);
    assert.ok(await page.evaluate(() => window.__mock.count("set_visualizer")) >= 1, "the analyser is switched on while the player is open");
    // No tooltips on a phone, no shortcut hints.
    assert.equal(await page.locator(".tooltip").count(), 0);
  } finally { await app.close(); }
});

test("the backend's app_info decides the platform when the user agent does not say", async () => {
  const app = await openApp({ config: { ...CONFIG, platform: "android" }, viewport: { width: 1100, height: 760 } });
  try {
    await app.page.waitForFunction(() => document.documentElement.dataset.platform === "android");
    assert.equal(await app.page.evaluate(() => document.documentElement.dataset.layout), "phone");
    await app.page.waitForSelector(".dock", { state: "visible" });
  } finally { await app.close(); }
});

// ------------------------------------------------------ audits, every screen

/** Opens every screen of the phone UI in turn and calls `check(name)`. */
async function everyScreen(page, check) {
  const back = () => pressBack(page);
  await check("home");
  await tap(page, ".stage-cta");
  await see(page, ".player:not(.is-empty)");
  await check("home-playing");
  await tap(page, ".player .controls .icon-btn:nth-child(4)"); // the second mix track has timed lyrics
  await tap(page, ".explore-row");
  await page.waitForFunction(() => document.querySelector(".main").dataset.view === "forYou");
  await see(page, ".main > .view:not([hidden]) .row");
  await check("explore");
  await back();
  await tab(page, "library");
  await page.evaluate(async()=>{(await import('/js/listen-later.js')).saveForLater();});
  await check("library-all");
  await tap(page, '.nav-btn[aria-label="Действия"]');
  await tap(page, '.sheet-item[data-action="import-playlist"]');
  await check("playlist-import-input");
  await page.locator('.import-text').fill('Singer — One');
  await page.locator('.import-text').blur();
  await tap(page, '.import-input .btn.primary');
  await see(page, '.import-review');
  await check("playlist-import-review");
  await back();
  await tap(page, '.nav-btn[aria-label="Действия"]');
  await tap(page, '.sheet-item[data-action="upload-audio"]');
  await check("audio-upload");
  await back();
  for (const section of ["tracks", "playlists", "albums"]) {
    await tap(page, `.lib-sections [data-section="${section}"]`);
    await page.waitForTimeout(200);
    await check(`library-${section}`);
  }
  for (const [label, name] of [["История", "history"], ["Загрузки", "downloads"]]) {
    await tap(page, `.nav-btn[aria-label="${label}"]`);
    await page.waitForTimeout(250);
    await check(name);
    await back();
  }
  // Умные плейлисты (0.6.0): страница поверх вкладки «Медиатека».
  await tap(page, '.lib-sections [data-section="all"]');
  await tap(page, ".lib-summary.smart");
  await see(page, ".smart-view .row");
  await check("smart");
  await back();
  // Массовые действия: режим выбора в списке «Моя музыка».
  await tap(page, '.lib-sections [data-section="tracks"]');
  await see(page, ".library-hub .row");
  await tap(page, ".library-hub .select-toggle");
  await visible(page, ".library-hub .row").nth(1).tap();
  await check("selection");
  await tap(page, ".library-hub .select-toggle");
  await tap(page, '.lib-sections [data-section="all"]');
  await tap(page, '.nav-btn[aria-label="Действия"]');
  await tap(page, '.sheet-item[data-action="friends"]');
  await see(page, ".friend");
  await check("friends");
  await tap(page, ".friend");
  await see(page, ".main > .view:not([hidden]) .row");
  await check("friend");
  await back(); await back();
  await tap(page, '.lib-sections [data-section="playlists"]');
  await tap(page, ".lib-grid .card");
  await see(page, ".main > .view:not([hidden]) .hero-art");
  await page.waitForTimeout(300);
  await check("playlist");
  await back();
  await tab(page, "search");
  await check("search");
  await page.locator('input[placeholder^="Исполнитель"]').fill("тест");
  await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length > 3, null, { timeout: 8000 }).catch(async (e) => {
    throw new Error(`${e.message} ${JSON.stringify(await page.evaluate(() => ({ search: window.__mock.count("search"), view: document.querySelector(".main").dataset.view, rows: document.querySelectorAll(".row").length, value: document.querySelector(".main > .view:not([hidden]) input[type=search]")?.value, kb: document.documentElement.dataset.keyboard })))}`);
  });
  await page.locator('input[placeholder^="Исполнитель"]').blur();
  await check("search-results");
  await tap(page, '.tabs [data-tab="albums"]');
  await see(page, ".grid .card");
  await tap(page, ".grid .card");
  await see(page, ".main > .view:not([hidden]) .hero-art");
  await page.waitForTimeout(300);
  await check("album");
  await tap(page, ".page-artists .link-text");
  await page.waitForFunction(() => document.querySelector(".main").dataset.view === "artist");
  await page.waitForTimeout(400);
  await check("artist");
  await tab(page, "profile");
  await check("profile");
  await tap(page, '[data-row="settings"]');
  await page.waitForTimeout(250);
  await check("settings");
  await back();
  await tap(page, '[data-row="stats"]');
  await see(page, ".stat-list");
  await check("stats");
  await back();
  await tap(page, ".player .now");
  await see(page, ".np:not([hidden])");
  await check("player");
  await tap(page, ".np-quick button:nth-child(1)");
  await see(page, ".ly-line");
  await check("lyrics");
  await tap(page, ".np-quick button:nth-child(2)");
  await see(page, ".nq-row");
  await check("queue");
  await back(); await back();
  await tap(page, ".player .controls .icon-btn:nth-child(4)");
  await tap(page, ".player .now");
  await tap(page, ".np-quick button:nth-child(3)");
  await see(page, ".sheet");
  await check("action-sheet");
}

for (const [name, size] of [["small", "small"], ["pixel", "pixel7"], ["large", "large"], ["landscape", "landscape"]]) {
  test(`no horizontal overflow on any screen (${name} phone)`, async () => {
    const app = await phoneApp({ phone: size });
    try {
      const { page } = app;
      const width = page.viewportSize().width;
      const offenders = [];
      await everyScreen(page, async (screen) => {
        const bad = await page.evaluate((w) => {
          const out = [];
          if (document.documentElement.scrollWidth > w) out.push(`page scrollWidth ${document.documentElement.scrollWidth}`);
          for (const el of document.querySelectorAll(".main > .view:not([hidden]), .np:not([hidden]), .sheet")) {
            if (el.scrollWidth > el.clientWidth + 1) out.push(`${el.className} scrollWidth ${el.scrollWidth} > ${el.clientWidth}`);
          }
          // Anything visible that sticks out of the window (horizontal shelves scroll inside their own box).
          for (const el of document.querySelectorAll(".main > .view:not([hidden]) *, .np:not([hidden]) *, .sheet *, .dock *, .player *, .navbar *")) {
            const r = el.getBoundingClientRect();
            if (!r.width || !r.height) continue;
            if (el.closest(".release-row, .mix-grid, .selena-row, .settings-nav, .selection-bar, .ly-edit-controls, .eq-bands, .lib-row, .ptr")) continue;
            if (getComputedStyle(el).position === "absolute" && el.closest(".np-backdrop, .swipe-bg")) continue;
            if (r.right > w + 1 || r.left < -1) out.push(`${el.tagName}.${String(el.className).slice(0, 30)} ${Math.round(r.left)}..${Math.round(r.right)}`);
          }
          return out.slice(0, 5);
        }, width);
        if (bad.length) offenders.push(`${screen}: ${bad.join("; ")}`);
      });
      assert.deepEqual(offenders, []);
      assert.deepEqual(app.problems, []);
    } finally { await app.close(); }
  });
}

test("hit targets: everything a finger can press is at least 44 px, on every screen", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    const offenders = new Set();
    await everyScreen(page, async (screen) => {
      const bad = await page.evaluate(() => {
        const selector = "button, a[href], input:not([type=hidden]), select, textarea, summary, [role=button], [role=slider], [role=tab], .row, label.set-check";
        const out = [];
        for (const el of document.querySelectorAll(selector)) {
          const style = getComputedStyle(el);
          if (style.pointerEvents === "none" || style.visibility === "hidden" || style.display === "none" || el.disabled) continue;
          if (el.closest("[hidden], .np[hidden], [data-desktop-only]")) continue;
          const r = el.getBoundingClientRect();
          if (!r.width || !r.height) continue;
          // Off screen (a shelf's later cards, a view that is not shown): not pressable right now.
          if (r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) continue;
          if (!el.offsetParent && style.position !== "fixed") continue;
          const covered = el.closest(".view, .np, .sheet, .dock, .player, .navbar");
          if (covered?.classList.contains("view") && covered.hidden) continue;
          // An inline <label>-wrapped switch: the label row is the target.
          if (el.matches("input[type=checkbox]") && el.closest("label")) { const l = el.closest("label").getBoundingClientRect(); if (l.height >= 44) continue; }
          if (el.matches("input[type=range]") && r.height >= 44) continue;
          if (Math.min(r.width, r.height) < 43.5) out.push(`${el.tagName.toLowerCase()}.${String(el.className).split(" ")[0]} ${Math.round(r.width)}x${Math.round(r.height)} "${(el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 20)}"`);
        }
        return out;
      });
      for (const item of bad) offenders.add(`${screen}: ${item}`);
    });
    assert.deepEqual([...offenders], []);
  } finally { await app.close(); }
});

test("no hover-only controls: the row menu, hearts and actions are visible without a pointer", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await openLibraryTracks(page);
    const state = await page.evaluate(() => {
      const row = document.querySelector(".library-hub .row");
      const more = row.querySelector('.icon-btn[data-action="menu"]');
      const s = getComputedStyle(more);
      const actions = getComputedStyle(row.querySelector(".actions"));
      const time = getComputedStyle(row.querySelector(".time"));
      return { opacity: s.opacity, display: s.display, pe: actions.pointerEvents, actionsOpacity: actions.opacity, timeOpacity: time.opacity };
    });
    assert.deepEqual(state, { opacity: "1", display: "flex", pe: "auto", actionsOpacity: "1", timeOpacity: "1" });
    // Nothing relies on :hover to appear: with hover unavailable no rule hides content.
    const hoverRules = await page.evaluate((helper) => {
      const rules = new Function(`return (${helper})()`)();
      return rules.filter((rule) => /html\[data-layout="phone"\][^{]*:hover/.test(rule.cssText) && /(opacity: 0;|display: none)/.test(rule.cssText)).length;
    }, allRules.toString());
    assert.equal(hoverRules, 0);
    assert.equal(await page.locator(".tooltip").count(), 0);
  } finally { await app.close(); }
});

test("touch behaviour: no tap highlight, no double-tap zoom, contained overscroll, no text selection outside inputs", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    const s = await page.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      const button = getComputedStyle(document.querySelector(".dock-tab"));
      const view = getComputedStyle(document.querySelector(".home-view"));
      const body = getComputedStyle(document.body);
      return {
        touch: root.touchAction, root: root.overscrollBehaviorY, view: view.overscrollBehaviorY,
        highlight: button.webkitTapHighlightColor, select: body.userSelect,
        viewport: document.querySelector('meta[name="viewport"]').content,
      };
    });
    assert.equal(s.touch, "manipulation");
    assert.equal(s.view, "contain");
    assert.match(s.highlight, /rgba\(0, 0, 0, 0\)|transparent/);
    assert.equal(s.select, "none");
    assert.match(s.viewport, /viewport-fit=cover/);
    // Press feedback is a dim: an :active rule lowers the opacity.
    const dims = await page.evaluate((helper) => new Function(`return (${helper})()`)().some((rule) => /:active/.test(rule.cssText) && /opacity: 0\.6/.test(rule.cssText)), allRules.toString());
    assert.ok(dims);
  } finally { await app.close(); }
});

test("states on a phone: loading skeleton, an illustrated empty state, an error with the VPN card and a retry", async () => {
  const app = await phoneApp({ config: { ...CONFIG, failing: ["library"], failKind: "offline", pageDelay: 400 } });
  try {
    const { page } = app;
    await tab(page, "library");
    await tap(page, '.lib-sections [data-section="tracks"]');
    await page.waitForSelector(".library-hub .net-hint");
    const card = await page.locator(".library-hub .net-hint").boundingBox();
    assert.ok(card.x >= 0 && card.x + card.width <= 412, "the VPN card fits the screen");
    await tap(page, ".library-hub .net-hint .net-retry");
    await tab(page, "profile");
    await tap(page, '[data-row="settings"]');
    await pressBack(page);
    await tab(page, "library");
    await tap(page, '.lib-sections [data-section="albums"]');
    assert.ok(await visible(page, ".il").count() >= 1, "illustrated empty state");
  } finally { await app.close(); }
});

test("sign-in screen and sign-out on a phone", async () => {
  const app = await openApp({ config: { signedIn: false }, phone: true });
  try {
    const { page } = app;
    await page.waitForSelector(".login");
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    await tap(page, ".login .btn");
    await page.waitForSelector(".dock");
    await tab(page, "profile");
    await tap(page, '[data-row="signout"]');
    await page.waitForSelector("dialog[open]");
    await tap(page, "dialog .btn.danger");
    await page.waitForSelector(".login");
    assert.equal(await page.locator(".dock").count(), 0);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("dark theme and another accent apply on the phone layout", async () => {
  const app = await phoneApp({ colorScheme: "dark" });
  try {
    const { page } = app;
    assert.equal(await page.locator('meta[name="theme-color"]').getAttribute("content"), "#000000");
    await tab(page, "profile");
    await tap(page, '[data-row="settings"]');
    await tap(page, '.accent-opt[data-v="green"]');
    assert.equal(await page.evaluate(() => document.documentElement.dataset.accent), "green");
    await tap(page, '.theme-opt[data-v="light"]');
    await page.waitForFunction(() => document.querySelector('meta[name="theme-color"]').content === "#ffffff");
  } finally { await app.close(); }
});

// -------------------------------------------- the 0.6.0 screens on a phone

const calledCommands = (page) => page.evaluate(() => window.__mock.calls.map((c) => c.command));

test("Android: updates, window size, tray, shortcuts and track export are hidden, their commands are never sent", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await tab(page, "profile");
    await tap(page, '[data-row="settings"]');
    await page.waitForSelector(".settings .set-body");
    await page.waitForTimeout(300);
    const text = (await page.locator(".settings").innerText()).toLowerCase();
    for (const word of ["обновлени", "размер окна", "мини-плеер", "системный трей", "горячие клавиши", "устройство вывода", "визуализац"]) assert.ok(!text.includes(word), `"${word}" is not shown`);
    // What stays: sound options, the equaliser, Selena's tastes, the sleep timer, lyrics.
    for (const word of ["эквалайзер", "selena", "таймер сна", "предзагрузка"]) assert.ok(text.includes(word.toLowerCase()), `"${word}" stays`);
    // The jump chips of the screen do not lead to hidden sections.
    const chips = await page.locator(".settings-nav .btn").evaluateAll((nodes) => nodes.filter((n) => getComputedStyle(n).display !== "none").map((n) => n.textContent.trim()));
    assert.deepEqual(chips, ["Звук", "Оформление", "Эквалайзер"]);
    const commands = await calledCommands(page);
    for (const command of ["update_state", "update_check", "update_download", "output_devices", "set_output_device", "set_close_to_tray", "set_window_mode", "set_fullscreen"]) assert.ok(!commands.includes(command), `${command} was not called`);
    // The track menu has no "save audio file": export is a stub on Android.
    await pressBack(page);
    await openLibraryTracks(page);
    await tap(page, '.library-hub .row [data-action="menu"]');
    await see(page, ".sheet");
    const items = await page.locator(".sheet .sheet-item").evaluateAll((nodes) => nodes.map((n) => n.dataset.action));
    assert.ok(items.includes("play-next") && items.includes("radio") && items.includes("taste"), `menu: ${items}`);
    assert.ok(!items.includes("export"), "no export item");
    await pressBack(page);
    // Hardware keyboard shortcuts do nothing on the phone.
    await page.keyboard.press("/");
    await page.keyboard.press("F11");
    assert.notEqual(await view(page), "search");
    assert.equal(await page.evaluate(() => window.__mock.count("set_fullscreen")), 0);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("the window mini player (desktop) never shows on a phone; the phone's own mini player does", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await playFromHome(page);
    assert.equal(await page.locator(".mini-player").isVisible(), false);
    assert.equal(await page.evaluate(() => document.documentElement.dataset.windowMode ?? "none"), "none");
    assert.equal(await page.locator(".player").isVisible(), true);
    assert.equal(await page.locator(".player .title").innerText(), await page.evaluate(() => window.__mock.state.player.track.title));
  } finally { await app.close(); }
});

test("smart playlists: a row of the Library tab opens them as a page, a rule loads tracks, a tap plays, Back returns", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await tab(page, "library");
    await tap(page, ".lib-summary.smart");
    await page.waitForFunction(() => document.querySelector(".main").dataset.view === "smart");
    await see(page, ".smart-view .row");
    assert.equal(await page.locator(".smart-view h1").first().innerText(), "Давно не слушал");
    assert.equal(await page.locator(".dock-tab[aria-current=page]").getAttribute("data-tab"), "library");
    // The rule picker and its buttons fit the screen and are fingertip-sized.
    const boxes = await page.locator(".smart-view .smart-tools > *").evaluateAll((nodes) => nodes.map((n) => { const r = n.getBoundingClientRect(); return { l: r.left, r: r.right, h: r.height }; }));
    assert.ok(boxes.length >= 4);
    for (const box of boxes) assert.ok(box.l >= 0 && box.r <= 412 && box.h >= 44, JSON.stringify(box));
    // Switching the rule reloads the list.
    await page.locator(".smart-view select").selectOption({ index: 1 });
    await see(page, ".smart-view .row");
    assert.equal(await page.locator(".smart-view h1").first().innerText(), "Недавно добавлено");
    await tap(page, ".smart-view .row");
    await page.waitForFunction(() => window.__mock.count("play_tracks") > 0);
    // The editor is a dialog with a rule choice a thumb can use.
    await tap(page, ".smart-view .smart-tools .btn:nth-of-type(2)");
    await page.waitForSelector("dialog[open]");
    assert.ok((await page.locator("dialog select.field").boundingBox()).height >= 44);
    await pressBack(page);
    assert.equal(await page.locator("dialog[open]").count(), 0);
    assert.equal(await pressBack(page), true);
    assert.equal(await view(page), "libraryHub");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("bulk actions: Select turns taps into choosing, the bar scrolls sideways inside the screen, queue and cancel work", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await openLibraryTracks(page);
    await tap(page, ".library-hub .select-toggle");
    await see(page, ".library-hub .selection-bar");
    // A tap chooses a row instead of playing it.
    const plays = await page.evaluate(() => window.__mock.count("play_tracks"));
    await visible(page, ".library-hub .row").nth(1).tap();
    await visible(page, ".library-hub .row").nth(2).tap();
    assert.equal(await page.evaluate(() => window.__mock.count("play_tracks")), plays);
    assert.equal(await page.locator(".library-hub .selection-bar strong").innerText(), "Выбрано: 2");
    assert.equal(await page.locator(".library-hub .row.multi-selected").count(), 2);
    // The bar does not widen the screen and its buttons are 44 px tall.
    const bar = await page.locator(".library-hub .selection-bar").evaluate((n) => ({ w: n.getBoundingClientRect().width, sw: n.scrollWidth, scroll: getComputedStyle(n).overflowX, wrap: getComputedStyle(n).flexWrap, buttons: [...n.querySelectorAll("button")].map((b) => b.getBoundingClientRect().height) }));
    assert.ok(bar.w <= 412 && bar.scroll === "auto" && bar.wrap === "nowrap");
    assert.ok(bar.buttons.every((h) => h >= 44), `button heights ${bar.buttons}`);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    await tap(page, '.library-hub .selection-bar [data-batch="queue"]');
    await page.waitForFunction(() => window.__mock.count("play_last") >= 2);
    // Leaving the mode restores the rows.
    await tap(page, ".library-hub .select-toggle");
    assert.equal(await page.locator(".library-hub .selection-bar").isVisible(), false);
    await visible(page, ".library-hub .row").nth(3).tap();
    await page.waitForFunction((n) => window.__mock.count("play_tracks") > n, plays);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("downloads: the offline-playlists sync card is a card of the screen (not a desktop strip) and its controls are fingertip-sized", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await tab(page, "library");
    await tap(page, '.nav-btn[aria-label="Загрузки"]');
    await see(page, ".sync-panel");
    const card = await page.locator(".sync-panel").evaluate((n) => { const r = n.getBoundingClientRect(); const s = getComputedStyle(n); return { l: r.left, r: r.right, radius: parseFloat(s.borderTopLeftRadius), border: s.borderTopWidth, label: n.querySelector("label").getBoundingClientRect().height }; });
    assert.ok(card.l >= 12 && card.r <= 400, "inside the gutters");
    assert.ok(card.radius >= 14 && card.border !== "0px", "a bordered card");
    assert.ok(card.label >= 44, "the checkbox row is a 44 px target");
    assert.equal(await page.locator(".sync-panel .set-note").first().isVisible(), true);
    // The sync command is a normal (non-stub) command: the button works once a playlist is synced.
    assert.equal(await page.locator(".sync-panel .btn").first().evaluate((n) => n.getBoundingClientRect().height >= 44), true);
  } finally { await app.close(); }
});

test("Studio on a phone: Onest, the window frame and the sidebar chrome are gone, tokens drive the colours", async () => {
  for (const colorScheme of ["light", "dark"]) {
    const app = await phoneApp({ colorScheme });
    try {
      const { page } = app;
      const s = await page.evaluate(() => {
        const main = getComputedStyle(document.querySelector(".main"));
        const shell = getComputedStyle(document.querySelector(".shell"));
        const dock = getComputedStyle(document.querySelector(".player-dock"));
        const root = getComputedStyle(document.documentElement);
        return { font: getComputedStyle(document.body).fontFamily, mainBorder: main.borderTopWidth, mainRadius: main.borderTopLeftRadius, pad: shell.paddingTop, dock: dock.display, bg: main.backgroundColor, page: root.getPropertyValue("--page-top").trim() };
      });
      assert.match(s.font, /Onest/);
      assert.equal(s.mainBorder, "0px");
      assert.equal(s.mainRadius, "0px");
      assert.equal(s.pad, "0px");
      assert.equal(s.dock, "contents");
      assert.equal(s.bg, colorScheme === "dark" ? "rgb(0, 0, 0)" : "rgb(255, 255, 255)");
    } finally { await app.close(); }
  }
});

test("reduced motion: pages, the player sheet, action sheets and the dock are not animated at all", async () => {
  const app = await phoneApp({ reducedMotion: "reduce" });
  try {
    const { page } = app;
    await playFromHome(page);
    await tab(page, "library");
    await tap(page, ".lib-summary.smart");
    const names = await page.evaluate(() => {
      const name = (sel) => { const el = document.querySelector(sel); return el ? getComputedStyle(el).animationName : "missing"; };
      return { page: name(".main > .view:not([hidden])"), dock: getComputedStyle(document.querySelector(".dock-tab")).transitionDuration };
    });
    assert.equal(names.page, "none");
    assert.match(names.dock, /^(0s|1e-05s)/);
    await tap(page, ".player .now");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".np")).animationName), "none");
    await pressBack(page);
    await openLibraryTracks(page).catch(() => {});
    await tap(page, '.row [data-action="menu"]');
    await see(page, ".sheet");
    assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".sheet")).animationName), "none");
  } finally { await app.close(); }
});

test("sign-in on Android: a large primary button, the VPN how-to when VK does not load, a way to start again", async () => {
  const app = await openApp({ config: { signedIn: false, persistent: true, platform: "android", loginPending: true }, phone: true });
  try {
    const { page } = app;
    await page.waitForSelector(".login");
    const box = await page.locator(".login .btn.primary").boundingBox();
    assert.ok(box.height >= 54 && box.width >= 300, `primary button ${box.width}x${box.height}`);
    assert.equal(await page.locator(".login .note").isVisible(), false, "the keystore keeps the session: no warning");
    await page.locator(".login .btn.primary").tap();
    await page.waitForSelector(".login-recovery:not([hidden])");
    const restart = await page.locator(".login-restart").boundingBox();
    assert.ok(restart.height >= 44, `restart ${restart.height}`);
    await page.evaluate(() => window.__mock.emit("pm-login", "loadfailed"));
    await page.waitForSelector(".login .net-hint");
    const hint = await page.locator(".login .net-hint").boundingBox();
    assert.ok(hint.x >= 0 && hint.x + hint.width <= 412, "the VPN hint fits");
    assert.ok((await page.locator(".login .net-hint .net-retry").boundingBox()).height >= 44);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("error states are touchable too: the inline retry and VPN links of a failed Home and Library are 44 px", async () => {
  const app = await phoneApp({ config: { ...CONFIG, failing: ["library", "playlists", "catalog_mixes", "recommendations", "new_releases"], failKind: "offline" } });
  try {
    const { page } = app;
    for (const screen of ["home", "library"]) {
      if (screen === "library") { await tab(page, "library"); await tap(page, '.lib-sections [data-section="tracks"]'); await page.waitForSelector(".library-hub .net-hint"); }
      await page.waitForTimeout(500);
      const small = await page.evaluate(() => [...document.querySelectorAll(".main > .view:not([hidden]) :is(button, a, .link-text)")]
        .filter((el) => el.offsetParent && !el.closest("[hidden]") && !el.disabled)
        .map((el) => { const r = el.getBoundingClientRect(); return { r, text: (el.textContent || el.getAttribute("aria-label") || "").trim().slice(0, 24) }; })
        .filter(({ r }) => r.top >= 0 && r.bottom <= innerHeight && Math.min(r.width, r.height) < 43.5)
        .map(({ r, text }) => `${Math.round(r.width)}x${Math.round(r.height)} ${text}`));
      assert.deepEqual(small, [], screen);
    }
  } finally { await app.close(); }
});

test("rows keep their height: the clickable artist name does not stretch a 56 px row (360 px phone)", async () => {
  const app = await phoneApp({ phone: "small" });
  try {
    const { page } = app;
    await openLibraryTracks(page);
    const heights = await page.locator(".library-hub .row").evaluateAll((rows) => rows.slice(0, 5).map((r) => Math.round(r.getBoundingClientRect().height)));
    assert.deepEqual(heights, [56, 56, 56, 56, 56]);
    const overflow = await page.locator(".library-hub .row").first().evaluate((r) => [...r.children].every((c) => c.getBoundingClientRect().height <= r.getBoundingClientRect().height + 0.5));
    assert.equal(overflow, true);
  } finally { await app.close(); }
});

test("collections: pinning survives reload, opens the playlist and stays private to the account", async () => {
  const app = await phoneApp();
  try {
    const { page } = app;
    await tab(page, "library");
    await tap(page, '.lib-sections [data-section="playlists"]');
    await see(page, '.lib-grid .lib-pin');
    await tap(page, '.lib-grid .lib-pin');
    await tap(page, '.lib-sections [data-section="all"]');
    assert.equal(await page.locator('.lib-pinned .lib-collection').count(), 1);
    await page.reload();
    await see(page, '.dock');
    await tab(page, "library");
    await see(page, '.lib-pinned .card');
    await tap(page, '.lib-pinned .card');
    assert.equal(await view(page), "playlists");
    await pressBack(page);
    await page.evaluate(async () => {
      const {store, set} = await import('/js/store.js');
      set('session', {...store.session, profile:{id:8, first_name:'Другой', last_name:'Аккаунт'}});
    });
    assert.equal(await page.locator('.lib-pinned').isVisible(), false);
    await page.evaluate(async () => {
      const {store, set} = await import('/js/store.js');
      set('session', {...store.session, profile:{id:7, first_name:'Иван', last_name:'Петров'}});
    });
    await tap(page, '.lib-pinned .lib-pin');
    assert.equal(await page.locator('.lib-pinned').isVisible(), false);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("collections: list layout persists for both albums and playlists", async () => {
  const app = await phoneApp({phone:"small"});
  try {
    const {page} = app;
    await tab(page,"library");
    await tap(page, '.lib-sections [data-section="playlists"]');
    await tap(page, '.lib-layout[data-layout-kind="playlist"]');
    assert.equal(await page.locator('.lib-grid.list-layout').count(),2);
    await page.reload();
    await see(page,'.dock');
    await tab(page,"library");
    await tap(page, '.lib-sections [data-section="playlists"]');
    await see(page, '.lib-grid.list-layout .card');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),false);
    await tap(page, '.lib-layout[data-layout-kind="playlist"]');
    assert.equal(await page.locator('.lib-grid.list-layout').count(),0);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("player: releasing a sheet drag does not activate the content under the finger", async () => {
  const app = await phoneApp();
  try {
    const result = await app.page.evaluate(async () => {
      const {attachSheetDrag} = await import('/js/mobile/np-sheet.js');
      const sheet = document.createElement('div');
      const content = document.createElement('span');
      sheet.append(content); document.body.append(sheet);
      let clicks = 0;
      content.addEventListener('click', () => clicks++);
      attachSheetDrag(sheet, () => {});
      const pointer = (type, y) => content.dispatchEvent(new PointerEvent(type, {bubbles:true,pointerId:99,pointerType:'touch',clientX:50,clientY:y}));
      pointer('pointerdown', 50); pointer('pointermove', 65); pointer('pointerup', 65);
      content.dispatchEvent(new MouseEvent('click', {bubbles:true,cancelable:true,detail:1}));
      const afterDrag = clicks;
      const button = document.createElement('button');
      sheet.append(button); button.addEventListener('click', () => clicks++);
      button.dispatchEvent(new PointerEvent('pointerdown', {bubbles:true,pointerId:100,pointerType:'touch'}));
      button.dispatchEvent(new MouseEvent('click', {bubbles:true,cancelable:true,detail:1}));
      sheet.remove();
      return {afterDrag, afterTap:clicks};
    });
    assert.deepEqual(result, {afterDrag:0,afterTap:1});
  } finally { await app.close(); }
});

const IMPORT_RESULTS = {
  "Singer One": [{track_id:8001,artist:"Singer",title:"One"}],
  "Singer Two": [{track_id:8002,artist:"Singer",title:"Two"}],
  "Singer Three": [{track_id:8003,artist:"Singer",title:"Three"}],
};
async function openImport(page) {
  await tab(page,"library");
  await tap(page,'.nav-actions .nav-btn:last-child');
  await tap(page,'.sheet [data-action="import-playlist"]');
  await see(page,'.playlist-import-view textarea');
}
test("listen later: save the current queue and position, restore it after reload, then remove it", async()=>{
  const app=await phoneApp();
  try {
    const {page}=app;
    await playFromHome(page);
    await page.evaluate(async()=>{await (await import('/js/api.js')).api.seek(42);});
    await tap(page,'.player .now');
    await tap(page,'.np-quick button:last-child');
    await tap(page,'.sheet [data-action="listen-later"]');
    await pressBack(page);
    await page.reload(); await see(page,'.dock');
    await tab(page,'library');
    await see(page,'.later-card');
    assert.match(await page.locator('.later-detail').innerText(),/0:42/);
    await tap(page,'.later-play');
    const restored=await page.evaluate(()=>window.__mock.last('load_queue'));
    assert.equal(restored.args.position,42);
    assert.equal(await page.evaluate(()=>window.__mock.last('toggle').command),'toggle');
    await tap(page,'.later-remove');
    assert.equal(await page.locator('.later-section').isVisible(),false);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("playlist import: review matches before writing, exclude a track, and open the created playlist", async()=>{
  const app=await phoneApp({phone:'small',config:{...CONFIG,searchResults:IMPORT_RESULTS}});
  try {
    const {page}=app; await openImport(page);
    await page.locator('.import-text').fill('Singer — One\nSinger — Two');
    await tap(page,'.import-input .btn.primary');
    await see(page,'.import-review');
    assert.equal(await page.evaluate(()=>window.__mock.count('create_playlist')),0);
    await page.locator('.import-choice input').last().uncheck();
    await tap(page,'.import-review .btn.primary');
    await see(page,'.import-body > .btn.primary');
    assert.equal(await page.evaluate(()=>window.__mock.count('add_to_playlist')),1);
    assert.equal(await page.evaluate(()=>window.__mock.last('add_to_playlist').args.track.title),'One');
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    await tap(page,'.import-body > .btn.primary');
    assert.equal(await view(page),'playlists');
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("playlist import: leaving during search cancels remaining work and never creates a playlist", async()=>{
  const app=await phoneApp({config:{...CONFIG,searchDelay:300,searchResults:IMPORT_RESULTS}});
  try {
    const {page}=app; await openImport(page);
    await page.locator('.import-text').fill('Singer — One\nSinger — Two');
    await tap(page,'.import-input .btn.primary');
    await pressBack(page);await page.waitForTimeout(400);
    assert.equal(await page.evaluate(()=>window.__mock.count('search')),1);
    assert.equal(await page.evaluate(()=>window.__mock.count('create_playlist')),0);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("playlist import: Stop keeps the partial playlist and sends no further additions", async()=>{
  const app=await phoneApp({config:{...CONFIG,importAddDelay:400,searchResults:IMPORT_RESULTS}});
  try {
    const {page}=app;await openImport(page);
    await page.locator('.import-text').fill('Singer — One\nSinger — Two\nSinger — Three');
    await tap(page,'.import-input .btn.primary');await see(page,'.import-review');
    await tap(page,'.import-review .btn.primary');
    await page.waitForFunction(()=>window.__mock.count('add_to_playlist')===1);
    await tap(page,'.import-body > .btn:not(.primary)');
    await page.waitForTimeout(500);
    assert.equal(await page.evaluate(()=>window.__mock.count('add_to_playlist')),1);
    assert.match(await page.locator('.import-status').innerText(),/Добавлено: 1.*Не обработано: 2/);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});

test("collections: search filters playlists without losing focus and clearing restores every card", async()=>{
  const app=await phoneApp({phone:'small'});
  try {
    const {page}=app;await tab(page,'library');
    await tap(page,'.lib-sections [data-section="playlists"]');
    const search=page.locator('[data-pane="playlists"] .lib-collection-search input');
    await search.fill('РАБОТА');
    assert.equal(await page.locator('[data-pane="playlists"] .lib-grid .card').count(),1);
    assert.equal(await search.evaluate(el=>document.activeElement===el),true);
    await search.fill('нет такого');
    assert.equal(await page.locator('[data-pane="playlists"] .lib-grid .card').count(),0);
    await tap(page,'[data-pane="playlists"] .lib-collection-search .clear');
    assert.equal(await page.locator('[data-pane="playlists"] .lib-grid .card').count(),3);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("listen later: another account cannot see the saved snapshot or its stream URL", async()=>{
  const app=await phoneApp();
  try {
    const {page}=app;await playFromHome(page);
    await page.evaluate(async()=>{const {store}=await import('/js/store.js');store.queue.tracks[store.queue.current].stream_url='https://private.invalid/stream';(await import('/js/listen-later.js')).saveForLater();});
    assert.equal(await page.evaluate(()=>JSON.parse(localStorage.getItem('pm.personal.7.listenLater')).tracks[0].stream_url),null);
    await tab(page,'library');await see(page,'.later-card');
    await page.evaluate(async()=>{const {store,set}=await import('/js/store.js');set('session',{...store.session,profile:{id:8}});});
    assert.equal(await page.locator('.later-section').isVisible(),false);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});

test("playlist import: a failed addition keeps confirmed tracks and reports unprocessed rows",async()=>{
  const app=await phoneApp({config:{...CONFIG,importAddFails:8002,searchResults:IMPORT_RESULTS}});
  try {
    const {page}=app;await openImport(page);
    await page.locator('.import-text').fill('Singer — One\nSinger — Two\nSinger — Three');
    await tap(page,'.import-input .btn.primary');await see(page,'.import-review');
    await tap(page,'.import-review .btn.primary');await see(page,'.import-body > .btn.primary');
    assert.equal(await page.evaluate(()=>window.__mock.count('add_to_playlist')),2);
    assert.match(await page.locator('.import-status').innerText(),/Добавлено: 1.*Ошибок: 1.*Не обработано: 1/);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("playlist import: account changes erase input and invalidate a pending match",async()=>{
  const app=await phoneApp({config:{...CONFIG,searchDelay:300,searchResults:IMPORT_RESULTS}});
  try {
    const {page}=app;await openImport(page);
    await page.locator('.import-text').fill('Singer — One\nSinger — Two');await tap(page,'.import-input .btn.primary');
    await page.evaluate(async()=>{const {store,set}=await import('/js/store.js');set('session',{...store.session,profile:{id:8}});});
    await page.waitForTimeout(400);
    assert.equal(await page.locator('.import-text').inputValue(),'');
    assert.equal(await page.locator('.import-rows > *').count(),0);
    assert.equal(await page.evaluate(()=>window.__mock.count('create_playlist')),0);
    assert.equal(await page.evaluate(()=>window.__mock.count('search')),1);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});

async function openAudioUpload(page) {
  await tab(page,'library');await tap(page,'.nav-btn[aria-label="Действия"]');
  await tap(page,'.sheet [data-action="upload-audio"]');await see(page,'.audio-upload-view');
}
test("audio upload: choosing is separate from uploading, metadata is required and success refreshes Library",async()=>{
  const app=await phoneApp({phone:'small'});
  try {
    const {page}=app;await openAudioUpload(page);
    assert.equal(await page.locator('.upload-body > .btn.primary').isDisabled(),true);
    await tap(page,'.upload-file .btn');
    assert.equal(await page.locator('.upload-body > label input').last().inputValue(),'chosen');
    assert.equal(await page.evaluate(()=>window.__mock.count('upload_audio')),0);
    await page.locator('.upload-body > label input').first().fill('My artist');
    await tap(page,'.upload-body > .btn.primary');
    assert.equal(await page.evaluate(()=>window.__mock.count('upload_audio')),1);
    assert.equal(await page.evaluate(()=>window.__mock.last('upload_audio').args.artist),'My artist');
    assert.match(await page.locator('.upload-body .import-status').innerText(),/Трек загружен/);
    assert.equal(await page.locator('.upload-filename').innerText(),'Файл не выбран');
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("audio upload: leaving a pending picker discards its eventual private file handle",async()=>{
  const app=await phoneApp({config:{...CONFIG,audioPickDelay:350}});
  try {
    const {page}=app;await openAudioUpload(page);await tap(page,'.upload-file .btn');await pressBack(page);
    await page.waitForTimeout(400);
    assert.equal(await page.evaluate(()=>window.__mock.count('discard_audio')),1);
    assert.equal(await page.evaluate(()=>window.__mock.count('upload_audio')),0);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("audio upload: cancel requests native cleanup and late responses cannot show success",async()=>{
  const app=await phoneApp({config:{...CONFIG,audioUploadDelay:400}});
  try {
    const {page}=app;await openAudioUpload(page);await tap(page,'.upload-file .btn');
    await page.locator('.upload-body > label input').first().fill('Singer');
    await tap(page,'.upload-body > .btn.primary');await tap(page,'.upload-body > .btn:not(.primary)');
    await page.waitForTimeout(450);
    assert.equal(await page.evaluate(()=>window.__mock.count('discard_audio')),1);
    assert.match(await page.locator('.upload-body .import-status').innerText(),/Остановка запрошена/);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
test("audio upload: ambiguous writes discard the handle to prevent a quick duplicate retry",async()=>{
  const app=await phoneApp({config:{...CONFIG,audioUploadFails:'unconfirmed'}});
  try {
    const {page}=app;await openAudioUpload(page);await tap(page,'.upload-file .btn');
    await page.locator('.upload-body > label input').first().fill('Singer');await tap(page,'.upload-body > .btn.primary');
    assert.equal(await page.locator('.upload-body > .btn.primary').isDisabled(),true);
    assert.equal(await page.evaluate(()=>window.__mock.count('discard_audio')),1);
    assert.equal(await page.evaluate(()=>window.__mock.count('upload_audio')),1);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});

test("row gestures: a rerender during a swipe keeps the same track and rejects a changed identity",async()=>{
  const app=await phoneApp();
  try {
    const {page}=app;await openLibraryTracks(page);
    await page.evaluate(()=>{
      function swipe(changeIdentity) {
        const row=document.querySelector('.library-hub .row');
        const send=(target,type,x)=>target.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerId:99,pointerType:'touch',clientX:x,clientY:200}));
        send(row,'pointerdown',20);send(row,'pointermove',40);
        const fresh=row.cloneNode(true);
        if(changeIdentity)fresh.dataset.trackKey='different-track';
        row.replaceWith(fresh);
        send(fresh,'pointermove',120);send(fresh,'pointerup',120);
      }
      swipe(false);swipe(true);
    });
    await page.waitForFunction(()=>window.__mock.count('play_next')===1);
    assert.equal(await page.evaluate(()=>window.__mock.count('play_next')),1);
    assert.deepEqual(app.problems,[]);
  } finally {await app.close();}
});
