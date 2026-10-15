import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

const rows = (page) => page.locator(".main > .view:not([hidden]) .row");

async function waitForLibrary(page) {
  await page.waitForFunction(() => document.querySelectorAll(".row").length > 0);
  await page.waitForFunction(() => !document.querySelector(".main > .view:not([hidden]) .status:not([hidden])"));
}

test("signed out: shows the sign-in screen, and signing in opens the library", async () => {
  const app = await openApp({ config: { signedIn: false } });
  try {
    const { page } = app;
    await page.waitForSelector(".login");
    assert.equal(await page.locator(".login .btn").innerText(), "Войти через VK");
    await page.click(".login .btn");
    await page.waitForSelector(".shell");
    await waitForLibrary(page);
    assert.equal(await page.evaluate(() => window.__mock.count("login_start")), 1);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("a warning is shown when the sign-in cannot be remembered", async () => {
  const app = await openApp({ config: { signedIn: false, persistent: false } });
  try {
    await app.page.waitForSelector(".login .note:not([hidden])");
  } finally { await app.close(); }
});

test("the library renders only the visible rows but scrolls through all of them", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    assert.match(await page.locator(".main > .view:not([hidden]) .meta").innerText(), /1234 треков/);
    const domRows = await rows(page).count();
    assert.ok(domRows > 5 && domRows < 40, `expected a window of rows, got ${domRows}`);
    await page.evaluate(() => { const l = document.querySelector(".main > .view:not([hidden]) .list"); l.scrollTop = l.scrollHeight; });
    await page.waitForFunction(() => [...document.querySelectorAll(".row .title")].some((e) => e.textContent === "Трек 1234"));
    assert.ok((await rows(page).count()) < 40);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("filtering ignores case and treats ё as е", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.fill(".main > .view:not([hidden]) input[type=search]", "елка");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .meta")?.textContent !== undefined);
    const titles = await page.locator(".main > .view:not([hidden]) .row .title").allInnerTexts();
    assert.ok(titles.length > 0 && titles.every((t) => t.startsWith("Ёлка")), titles.join(","));
    await page.fill(".main > .view:not([hidden]) input[type=search]", "нет такого трека");
    await page.waitForSelector(".main > .view:not([hidden]) .empty-state:not([hidden])");
    assert.equal(await rows(page).count(), 0);
  } finally { await app.close(); }
});

test("double click plays from that row with the visible list as the queue", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.fill(".main > .view:not([hidden]) input[type=search]", "Исполнитель 5");
    await page.waitForFunction(() => document.querySelectorAll(".row").length > 3);
    await rows(page).nth(2).dblclick();
    const call = await page.evaluate(() => window.__mock.last("play_tracks"));
    assert.equal(call.args.index, 2);
    assert.ok(call.args.tracks.length > 50 && call.args.tracks.every((t) => t.artist === "Исполнитель 5"));
    // The player bar follows the backend state.
    await page.waitForFunction(() => { const t = document.querySelector(".player .title")?.textContent; return t && t !== "Ничего не играет"; });
    assert.equal(await page.locator(".player .play").getAttribute("aria-label"), "Пауза");
    assert.equal(await page.locator(".row[aria-selected=true]").count(), 1);
  } finally { await app.close(); }
});

test("transport: play/pause, next, previous, seek, volume, shuffle and repeat reach the backend", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");

    await page.click(".player .play");
    assert.equal(await page.evaluate(() => window.__mock.count("toggle")), 1);
    await page.waitForFunction(() => document.querySelector(".player .play").getAttribute("aria-label") === "Играть");

    await page.click("button[aria-label=Следующий]");
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 2");
    await page.click("button[aria-label=Предыдущий]");
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");

    const seek = page.locator(".player .seek input");
    await seek.evaluate((el) => { el.value = "60"; el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); });
    assert.equal((await page.evaluate(() => window.__mock.last("seek"))).args.seconds, 60);
    assert.equal(await page.locator(".player .seek span").first().innerText(), "1:00");

    await page.locator(".player .volume input").evaluate((el) => { el.value = "40"; el.dispatchEvent(new Event("input", { bubbles: true })); });
    assert.equal((await page.evaluate(() => window.__mock.last("set_volume"))).args.volume, 0.4);

    await page.click("button[aria-label='Случайный порядок']");
    assert.equal((await page.evaluate(() => window.__mock.last("set_shuffle"))).args.on, true);
    await page.waitForFunction(() => document.querySelector("button[aria-label='Случайный порядок']").getAttribute("aria-pressed") === "true");

    for (const expected of ["all", "one", "off"]) {
      await page.locator(".controls .icon-btn").last().click();
      await page.waitForFunction((v) => window.__mock.state.player.repeat === v, expected);
    }
  } finally { await app.close(); }
});

test("a dragged seek bar is not yanked back by position updates", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");
    const seek = page.locator(".player .seek input");
    await seek.evaluate((el) => { el.value = "90"; el.dispatchEvent(new Event("input", { bubbles: true })); });
    await page.evaluate(() => { window.__mock.state.player.position = 3; window.__mock.emitState(); });
    assert.equal(await seek.inputValue(), "90", "the thumb stays where the user holds it");
  } finally { await app.close(); }
});

test("hover actions add to the queue, and the queue view shows and edits it", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => window.__mock.state.queue.length > 0);
    const second = rows(page).nth(1);
    await second.hover();
    await second.locator("[data-action=play-last]").click();
    assert.equal((await page.evaluate(() => window.__mock.last("play_last"))).args.track.title, "Трек 2");

    await page.click(".nav button[data-view=queue]");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length > 3);
    assert.equal(await page.locator(".main > .view:not([hidden]) .row[aria-selected=true]").count(), 1);
    const before = await page.evaluate(() => window.__mock.state.queue.length);
    await rows(page).nth(3).hover();
    await rows(page).nth(3).locator("[data-action=remove]").click();
    await page.waitForFunction((n) => window.__mock.state.queue.length === n - 1, before);
    await rows(page).nth(2).dblclick();
    assert.equal((await page.evaluate(() => window.__mock.last("jump"))).args.index, 2);
  } finally { await app.close(); }
});

test("search: debounced, stale answers are dropped, more pages load on scroll", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=search]");
    const input = page.locator(".main > .view:not([hidden]) input[type=search]");
    await input.fill("slow");
    await input.fill("fast");   // a newer query must win even though "slow" answers later
    await page.waitForFunction(() => [...document.querySelectorAll(".main > .view:not([hidden]) .row .title")].some((e) => e.textContent.startsWith("fast")));
    await page.waitForTimeout(500);
    const titles = await rows(page).locator(".title").allInnerTexts();
    assert.ok(titles.every((t) => t.startsWith("fast")), titles.join(","));
    const searches = await page.evaluate(() => window.__mock.calls.filter((c) => c.command === "search").map((c) => c.args.query));
    assert.ok(!searches.includes("slo"), "typing is debounced, not sent per keystroke");

    await page.evaluate(() => { const l = document.querySelector(".main > .view:not([hidden]) .list"); l.scrollTop = l.scrollHeight; });
    await page.waitForFunction(() => window.__mock.calls.some((c) => c.command === "search" && c.args.offset > 0));

    await input.fill("empty");
    await page.waitForSelector(".main > .view:not([hidden]) .status:not([hidden])");
    assert.match(await page.locator(".main > .view:not([hidden]) .status").innerText(), /Ничего не найдено/);
  } finally { await app.close(); }
});

test("playlists open into their tracks and back", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=playlists]");
    await page.waitForSelector(".card");
    assert.equal(await page.locator(".card").count(), 2);
    await page.locator(".card").first().click();
    await page.waitForFunction(() => [...document.querySelectorAll(".row .title")].some((e) => e.textContent.startsWith("Дорога:")));
    assert.equal(await page.locator(".main > .view:not([hidden]) .row").count(), 3);
    await page.click("button[aria-label=Назад]");
    await page.waitForSelector(".card");
  } finally { await app.close(); }
});

test("titles from VK are text, never markup", async () => {
  const hostile = `<img src=x onerror="window.__pwned=1"><script>window.__pwned=2</script>`;
  const app = await openApp({ config: { signedIn: true, hostileTitle: hostile } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent.includes("<img"));
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    assert.equal(await page.locator(".row img[src=x], .player img[src=x], .row script").count(), 0);
    assert.equal(await page.locator(".row .title").first().innerText(), hostile);
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

test("flat: nothing in the UI animates or transitions", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    const offenders = await page.evaluate(() => {
      const bad = [];
      // The full-screen view is the one place allowed to have effects.
      for (const el of document.querySelectorAll("*:not(.np):not(.np *)")) {
        const s = getComputedStyle(el);
        const durations = [...s.transitionDuration.split(","), ...s.animationDuration.split(",")].map((d) => parseFloat(d));
        if (durations.some((d) => d > 0) || s.animationName !== "none") bad.push(el.tagName + "." + el.className);
        if (s.boxShadow !== "none" || s.textShadow !== "none" || s.filter !== "none" || s.backdropFilter !== "none") bad.push("effect:" + el.tagName + "." + el.className);
      }
      return bad;
    });
    assert.deepEqual(offenders, []);
    assert.equal(await page.evaluate(() => document.getAnimations().length), 0);
  } finally { await app.close(); }
});

test("keyboard: Space toggles, arrows seek, Ctrl+arrows skip, M mutes, / opens search", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");
    await page.locator("body").click({ position: { x: 600, y: 300 } });
    await page.keyboard.press("Space");
    assert.equal(await page.evaluate(() => window.__mock.count("toggle")), 1);
    await page.keyboard.press("ArrowRight");
    assert.equal((await page.evaluate(() => window.__mock.last("seek"))).args.seconds, 5);
    await page.keyboard.press("Control+ArrowRight");
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 2");
    await page.keyboard.press("m");
    assert.equal((await page.evaluate(() => window.__mock.last("set_volume"))).args.volume, 0);
    await page.keyboard.press("m");
    assert.equal((await page.evaluate(() => window.__mock.last("set_volume"))).args.volume, 1);
    await page.keyboard.press("/");
    await page.waitForSelector(".main > .view:not([hidden]) input[type=search]:focus");
    // Typing in the search box must not trigger shortcuts.
    const toggles = await page.evaluate(() => window.__mock.count("toggle"));
    await page.keyboard.type("a b");
    assert.equal(await page.evaluate(() => window.__mock.count("toggle")), toggles);
  } finally { await app.close(); }
});

test("a session that expires sends the user back to sign-in", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => window.__mock.sessionExpired());
    await page.waitForSelector(".login");
    assert.match(await page.locator(".toast").innerText(), /Сессия VK истекла/);
  } finally { await app.close(); }
});

test("sign out clears the screen and returns to sign-in", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".account .link");
    await page.waitForSelector(".login");
    assert.equal(await page.evaluate(() => window.__mock.count("logout")), 1);
  } finally { await app.close(); }
});

test("backend errors are shown in plain words", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = app;
    await page.waitForSelector(".main > .view:not([hidden]) .status.error");
    assert.match(await page.locator(".main > .view:not([hidden]) .status.error").innerText(), /Нет подключения/);
  } finally { await app.close(); }
});

test("the saved queue comes back after a restart without starting to play", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).nth(4).dblclick();
    await page.waitForFunction(() => window.__mock.state.queue.length > 100);
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "state", ...window.__mock.state.player, position: 42 }));
    await page.waitForTimeout(100);
    const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("pm.queue.v1")));
    assert.equal(saved.tracks[0].stream_url, null, "expiring stream URLs are never saved");
    assert.ok(saved.tracks.length <= 500);
    assert.equal(saved.tracks[saved.index].title, "Трек 5");
  } finally { await app.close(); }
});

test("layouts: narrow window collapses the sidebar; dark scheme is readable; screenshots", async () => {
  for (const [name, viewport, colorScheme] of [["wide-light", { width: 1120, height: 740 }, "light"], ["wide-dark", { width: 1120, height: 740 }, "dark"], ["narrow-dark", { width: 760, height: 620 }, "dark"]]) {
    const app = await openApp({ config: { signedIn: true }, viewport, colorScheme });
    try {
      const { page } = app;
      await waitForLibrary(page);
      await rows(page).nth(2).dblclick();
      await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 3");
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
      assert.equal(overflow, false, `${name}: horizontal overflow`);
      if (process.env.PM_SCREENSHOTS) await page.screenshot({ path: `${process.env.PM_SCREENSHOTS}/${name}.png` });
    } finally { await app.close(); }
  }
});

// ------------------------------------------------------------- full screen

const bandsOf = (level) => Array.from({ length: 48 }, (_, i) => Math.max(0, Math.min(1, level * (1 - i / 60))));
const inked = (page) => page.evaluate(() => {
  const canvas = document.querySelector(".np-canvas");
  const { data } = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height);
  let count = 0;
  for (let i = 3; i < data.length; i += 4) if (data[i] > 0) count += 1;
  return count;
});

async function playAndOpen(app, { index = 0 } = {}) {
  const { page } = app;
  await waitForLibrary(page);
  await rows(page).nth(index).dblclick();
  await page.waitForFunction(() => { const t = document.querySelector(".player .title")?.textContent; return t && t !== "Ничего не играет"; });
  await page.click(".player .volume .icon-btn:last-child");
  await page.waitForSelector(".np:not([hidden])");
}

test("full screen: opens from the player bar, asks the engine for the spectrum only while open", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    assert.equal(await page.locator(".np-title").innerText(), "Трек 1");
    assert.equal(await page.evaluate(() => window.__mock.state.visualizer), true);
    await page.keyboard.press("Escape");
    await page.waitForSelector(".np", { state: "hidden" });
    assert.equal(await page.evaluate(() => window.__mock.state.visualizer), false);
    await page.keyboard.press("f");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await page.evaluate(() => window.__mock.state.visualizer), true);
    await page.keyboard.press("f");
    await page.waitForSelector(".np", { state: "hidden" });
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("full screen: every visualizer mode draws, and 'off' stops the analysis", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    const seen = [];
    for (let i = 0; i < 4; i += 1) {
      await page.evaluate((bands) => window.__mock.emit("pm-event", { type: "spectrum", bands }), bandsOf(0.9));
      const mode = await page.locator(".np").getAttribute("data-mode");
      seen.push(mode);
      assert.ok((await inked(page)) > 500, `${mode} should draw something`);
      await page.keyboard.press("v");
    }
    assert.deepEqual(seen, ["bars", "mirror", "wave", "radial"]);
    assert.equal(await page.locator(".np").getAttribute("data-mode"), "off");
    assert.equal(await page.evaluate(() => window.__mock.state.visualizer), false, "off asks the engine to stop analysing");
    assert.equal(await page.locator(".np-canvas").isVisible(), false);
    assert.equal(JSON.parse(await page.evaluate(() => localStorage.getItem("pm.settings.v1"))).visualizer, "off");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("full screen: the bars sink to nothing once playback pauses", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.evaluate((bands) => window.__mock.emit("pm-event", { type: "spectrum", bands }), bandsOf(1));
    const loud = await inked(page);
    assert.ok(loud > 500);
    await page.keyboard.press(" ");
    await page.waitForFunction((before) => {
      const c = document.querySelector(".np-canvas");
      const { data } = c.getContext("2d").getImageData(0, 0, c.width, c.height);
      let n = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i] > 0) n += 1;
      return n < before * 0.4; // resting stubs remain; the tall bars are gone
    }, loud, { timeout: 5000 });
  } finally { await app.close(); }
});

test("full screen: the cover's colours come from the engine, or from the name when that fails", async () => {
  for (const [config, expected] of [[{ signedIn: true }, "#28a050"], [{ signedIn: true, paletteFails: true }, /^hsl\(/]]) {
    const app = await openApp({ config });
    try {
      const { page } = app;
      await playAndOpen(app); // "Трек 1" has a cover
      await page.waitForTimeout(150);
      const colour = await page.evaluate(() => document.querySelector(".np").style.getPropertyValue("--np-a"));
      if (typeof expected === "string") assert.equal(colour, expected);
      else assert.match(colour, expected);
      assert.equal(await page.locator(".np-cover img").count(), 1);
    } finally { await app.close(); }
  }
});

test("full screen: window full screen is requested, and released when the view closes", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.click(".np-top .np-btn:last-child");
    await page.waitForFunction(() => window.__mock.state.fullscreen === true);
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => window.__mock.state.fullscreen === false);
  } finally { await app.close(); }
});

test("full screen: transport works, titles are text, and nothing overflows when narrow", async () => {
  const app = await openApp({ config: { signedIn: true, hostileTitle: "<img src=x onerror=alert(1)>" }, viewport: { width: 700, height: 620 } });
  try {
    const { page } = app;
    await playAndOpen(app);
    assert.equal(await page.locator(".np-title img").count(), 0);
    assert.equal(await page.locator(".np-title").innerText(), "<img src=x onerror=alert(1)>");
    await page.click(".np-controls .np-btn:nth-child(4)");
    await page.waitForFunction(() => document.querySelector(".np-title")?.textContent === "Трек 2");
    assert.match(await page.locator(".np-next").innerText(), /^Далее: /);
    await page.click(".np-play");
    await page.waitForFunction(() => window.__mock.state.player.status === "paused");
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth), false);
    if (process.env.PM_SCREENSHOTS) {
      for (const mode of ["bars", "mirror", "wave", "radial"]) {
        await page.evaluate((bands) => window.__mock.emit("pm-event", { type: "spectrum", bands }), bandsOf(0.8));
        await page.screenshot({ path: `${process.env.PM_SCREENSHOTS}/np-${mode}.png` });
        await page.keyboard.press("v");
      }
    }
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("full screen: reduced motion starts with the visualizer off", async () => {
  const app = await openApp({ config: { signedIn: true }, reducedMotion: "reduce" });
  try {
    await playAndOpen(app);
    assert.equal(await app.page.locator(".np").getAttribute("data-mode"), "off");
    assert.equal(await app.page.evaluate(() => window.__mock.state.visualizer), false);
  } finally { await app.close(); }
});

// ------------------------------------------------------------------ lyrics

const at = (page, position) =>
  page.evaluate((p) => window.__mock.emit("pm-event", { type: "state", ...window.__mock.state.player, position: p }), position);

test("lyrics: timed lines follow the song, click to jump, gaps show a note", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app); // "Трек 1": timed
    await page.waitForFunction(() => document.querySelectorAll(".ly-line").length === 40);
    assert.equal(await page.locator(".ly-line.gap").innerText(), "♪");
    assert.match(await page.locator(".ly-source").innerText(), /LRCLIB/);
    await at(page, 17);
    await page.waitForSelector(".ly-line.active");
    assert.equal(await page.locator(".ly-line.active").innerText(), "Строка 2");
    assert.equal(await page.locator(".ly-line.active").count(), 1);
    await at(page, 100);
    await page.waitForFunction(() => document.querySelector(".ly-line.active")?.textContent === "Строка 19");
    if (process.env.PM_SCREENSHOTS) await page.screenshot({ path: `${process.env.PM_SCREENSHOTS}/np-lyrics.png` });
    await page.locator(".ly-line", { hasText: "Строка 10" }).click();
    assert.equal(await page.evaluate(() => window.__mock.last("seek").args.seconds), 55);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("lyrics: the active line is brought into view", async () => {
  const app = await openApp({ config: { signedIn: true }, reducedMotion: "reduce" });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.waitForFunction(() => document.querySelectorAll(".ly-line").length === 40);
    await at(page, 180);
    await page.waitForFunction(() => {
      const line = document.querySelector(".ly-line.active");
      const body = document.querySelector(".ly-body").getBoundingClientRect();
      if (!line) return false;
      const box = line.getBoundingClientRect();
      return box.top >= body.top && box.bottom <= body.bottom;
    });
  } finally { await app.close(); }
});

test("lyrics: text from the service is shown as text, plain lyrics have no timing", async () => {
  const hostile = "<img src=x onerror=alert(1)>";
  const app = await openApp({ config: { signedIn: true, hostileLyric: hostile } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.waitForFunction(() => document.querySelectorAll(".ly-line").length === 40);
    assert.equal(await page.locator(".ly-line").first().innerText(), hostile);
    assert.equal(await page.locator(".ly img").count(), 0);
    await page.click(".np-controls .np-btn:nth-child(4)"); // next: "Трек 2" has plain lyrics
    await page.waitForSelector(".ly-text");
    assert.match(await page.locator(".ly-text").innerText(), /вторая <b>строка<\/b>/);
    assert.equal(await page.locator(".ly b").count(), 0);
    assert.equal(await page.locator(".ly-line").count(), 0);
    assert.match(await page.locator(".ly-source").innerText(), /VK/);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("lyrics: 'not found' and failures are different messages, and a failure can be retried", async () => {
  let app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app, { index: 4 }); // "Трек 5": none exist
    await page.waitForSelector(".ly-message");
    assert.equal(await page.locator(".ly-message").innerText(), "Текст не найден.");
  } finally { await app.close(); }

  app = await openApp({ config: { signedIn: true, lyricsFail: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.waitForSelector(".ly-message .np-chip");
    assert.match(await page.locator(".ly-message").innerText(), /Не удалось загрузить текст/);
    await page.click(".ly-message .np-chip");
    await page.waitForFunction(() => window.__mock.count("lyrics") === 2);
  } finally { await app.close(); }
});

test("lyrics: L toggles the panel and the choice sticks; hidden lyrics are not fetched", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.waitForSelector(".ly-line");
    await page.keyboard.press("l");
    assert.equal(await page.locator(".ly").isVisible(), false);
    assert.equal(JSON.parse(await page.evaluate(() => localStorage.getItem("pm.settings.v1"))).lyrics, false);
    const before = await page.evaluate(() => window.__mock.count("lyrics"));
    await page.click(".np-controls .np-btn:nth-child(4)");
    await page.waitForFunction(() => document.querySelector(".np-title")?.textContent === "Трек 2");
    assert.equal(await page.evaluate(() => window.__mock.count("lyrics")), before);
    await page.keyboard.press("l");
    await page.waitForSelector(".ly-text");
  } finally { await app.close(); }
});

// ------------------------------------------------------------------- hearts

async function searchFor(page, query) {
  await page.click(".nav button[data-view=search]");
  await page.locator(".main > .view:not([hidden]) input[type=search]").fill(query);
  await page.waitForFunction((q) => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent.startsWith(q), query);
}

test("hearts: adding from search puts the copy on top of My music, a second click takes it out", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await searchFor(page, "песня");
    const heart = rows(page).first().locator(".like");
    assert.equal(await heart.getAttribute("aria-pressed"), "false");
    await rows(page).first().hover();
    await heart.click();
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .like")?.getAttribute("aria-pressed") === "true");
    assert.equal(await page.locator(".toast").innerText(), "Добавлено в «Моя музыка»");
    assert.equal(await heart.isVisible(), true, "a liked heart stays visible without hover");

    await page.click(".nav button[data-view=library]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "песня 1");
    assert.match(await page.locator(".main > .view:not([hidden]) .meta").innerText(), /1235/);

    await searchFor(page, "песня");
    await rows(page).first().hover();
    await rows(page).first().locator(".like").click();
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .like")?.getAttribute("aria-pressed") === "false");
    const removed = await page.evaluate(() => window.__mock.last("remove_from_library").args.track);
    assert.equal(removed.owner_id, 7, "the library copy is removed, not the search result");
    await page.click(".nav button[data-view=library]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Трек 1");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("hearts: the player bar and full screen show and change the state of the playing track", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForSelector(".player .like.on");
    await page.click(".player .volume .icon-btn:last-child");
    await page.waitForSelector(".np-like.on");
    await page.click(".np-like");
    await page.waitForSelector(".np-like:not(.on)");
    await page.keyboard.press("Escape");
    await page.waitForSelector(".player .like:not(.on)");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Трек 2");
    assert.equal(await page.evaluate(() => window.__mock.count("remove_from_library")), 1);
  } finally { await app.close(); }
});

test("hearts: a refused change says why and leaves the heart as it was", async () => {
  const app = await openApp({ config: { signedIn: true, libraryWriteFails: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await searchFor(page, "песня");
    await rows(page).first().hover();
    await rows(page).first().locator(".like").click();
    await page.waitForSelector(".toast.error");
    assert.match(await page.locator(".toast.error").innerText(), /Access denied/);
    assert.equal(await rows(page).first().locator(".like").getAttribute("aria-pressed"), "false");
  } finally { await app.close(); }
});

// ------------------------------------------------------------- review fixes

test("arrow keys on the focused volume slider change the volume, not the position", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => window.__mock.state.player.status === "playing");
    await page.focus(".player .volume input[type=range]");
    const seeks = await page.evaluate(() => window.__mock.count("seek"));
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await page.waitForFunction(() => window.__mock.state.player.volume < 1);
    assert.equal(await page.evaluate(() => window.__mock.count("seek")), seeks);
  } finally { await app.close(); }
});

test("signing out and in again does not leave the old screens listening", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    const count = () => page.evaluate(async () => (await import("./js/store.js")).listenerCount("player"));
    const first = await count();
    for (let i = 0; i < 3; i += 1) {
      await page.click(".account .link");
      await page.waitForSelector(".login");
      await page.click(".login .btn");
      await page.waitForSelector(".shell");
      await waitForLibrary(page);
    }
    assert.equal(await count(), first);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("hearts wait until My music is known, so a click cannot add a duplicate", async () => {
  const app = await openApp({ config: { signedIn: true, pageDelay: 400 } });
  try {
    const { page } = app;
    await page.waitForFunction(() => document.querySelectorAll(".row").length > 0);
    await searchFor(page, "песня");
    assert.equal(await rows(page).first().locator(".like").isDisabled(), true);
    await page.waitForFunction(() => !document.querySelector(".main > .view:not([hidden]) .row .like")?.disabled, null, { timeout: 15000 });
  } finally { await app.close(); }
});

// ---------------------------------------------------------------- settings

test("settings: output device, tray, full screen defaults, all applied and remembered", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    assert.equal(await page.evaluate(() => window.__mock.state.closeToTray), true, "the saved choice is sent at start");
    await page.click(".nav button[data-view=settings]");
    await page.waitForFunction(() => document.querySelectorAll(".settings select")[0]?.options.length === 3);
    assert.match(await page.locator(".settings").innerText(), /Версия 0\.1\.0/);

    await page.selectOption(".settings select[aria-label=\"Устройство вывода\"]", "Наушники");
    await page.waitForFunction(() => window.__mock.state.outputDevice === "Наушники");
    await page.uncheck("#set-tray");
    await page.waitForFunction(() => window.__mock.state.closeToTray === false);
    await page.selectOption(".settings select[aria-label=\"Визуализация\"]", "radial");
    await page.uncheck("#set-lyrics");
    const saved = JSON.parse(await page.evaluate(() => localStorage.getItem("pm.settings.v1")));
    assert.deepEqual([saved.outputDevice, saved.closeToTray, saved.visualizer, saved.lyrics], ["Наушники", false, "radial", false]);

    // The full-screen view follows the new defaults.
    await rows(page).first().dblclick().catch(() => {});
    await page.click(".nav button[data-view=library]");
    await rows(page).first().dblclick();
    await page.keyboard.press("f");
    await page.waitForSelector(".np:not([hidden])");
    assert.equal(await page.locator(".np").getAttribute("data-mode"), "radial");
    assert.equal(await page.locator(".np").getAttribute("data-lyrics"), "off");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("settings: the saved output is used at the next start, and a desktop without a tray says so", async () => {
  const app = await openApp({ config: { signedIn: true, noTray: true } });
  try {
    const { page } = app;
    await page.evaluate(() => localStorage.setItem("pm.settings.v1", JSON.stringify({ outputDevice: "Наушники" })));
    await page.reload();
    await waitForLibrary(page);
    await page.waitForFunction(() => window.__mock.state.outputDevice === "Наушники");
    await page.click(".nav button[data-view=settings]");
    await page.waitForFunction(() => document.querySelector("#set-tray")?.disabled === true);
    assert.match(await page.locator(".settings").innerText(), /нет трея/);
    if (process.env.PM_SCREENSHOTS) await page.screenshot({ path: `${process.env.PM_SCREENSHOTS}/settings.png`, fullPage: true });
  } finally { await app.close(); }
});

// ------------------------------------------------------------------ for you

test("for you: the mix and recommendations load on first look, play, and refresh", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    assert.equal(await page.evaluate(() => window.__mock.count("mix")), 0, "nothing is fetched before it is opened");
    await page.click(".nav button[data-view=forYou]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
    assert.match(await page.locator(".main > .view:not([hidden]) .meta").innerText(), /100/);
    await page.click(".main > .view:not([hidden]) .tabs button[data-source=recs]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Рек 1");
    await page.click(".main > .view:not([hidden]) .tabs button[data-source=mix]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
    assert.equal(await page.evaluate(() => window.__mock.count("mix")), 1, "switching back uses what was loaded");
    await page.click(".main > .view:not([hidden]) .view-header .btn.primary");
    await page.waitForFunction(() => window.__mock.last("play_tracks")?.args.tracks.length === 100);
    await page.click(".main > .view:not([hidden]) .view-header .set-inline .btn");
    await page.waitForFunction(() => window.__mock.count("mix") === 2);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("for you: a failure says so in the list", async () => {
  const app = await openApp({ config: { signedIn: true, forYouFails: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=forYou]");
    await page.waitForSelector(".main > .view:not([hidden]) .status.error");
    assert.match(await page.locator(".main > .view:not([hidden]) .status.error").innerText(), /Нет подключения/);
  } finally { await app.close(); }
});

test("similar tracks: the row button plays the track followed by VK's radio from it", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).nth(1).hover();
    await rows(page).nth(1).locator("[data-action=radio]").click();
    await page.waitForFunction(() => window.__mock.last("play_tracks")?.args.tracks.length === 21);
    const call = await page.evaluate(() => window.__mock.last("play_tracks").args);
    assert.equal(call.tracks[0].title, "Трек 2");
    assert.equal(call.tracks[1].title, "Похожий 1");
    assert.equal(call.index, 0);
  } finally { await app.close(); }
});

test("similar tracks: when VK has none, nothing changes and the user is told", async () => {
  const app = await openApp({ config: { signedIn: true, noSimilar: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().hover();
    await rows(page).first().locator("[data-action=radio]").click();
    await page.waitForFunction(() => document.querySelector(".toast")?.textContent === "VK не нашёл похожих треков.");
    assert.equal(await page.evaluate(() => window.__mock.count("play_tracks")), 0);
  } finally { await app.close(); }
});

test("equaliser: presets and sliders reach the engine, off sends nothing, and it comes back on restart", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=settings]");
    await page.waitForSelector(".eq");
    await page.selectOption(".eq select", "bass");
    await page.waitForFunction(() => JSON.stringify(window.__mock.state.eq) === JSON.stringify([7, 6, 4, 2, 0, 0, 0, 0, 0, 0]));
    assert.equal(await page.isChecked("#eq-on"), true, "picking a preset turns it on");

    // Drag the 1 kHz slider up with the keyboard: a custom curve.
    await page.focus(".eq-slider >> nth=5");
    await page.keyboard.press("ArrowUp");
    await page.keyboard.press("ArrowUp");
    await page.waitForFunction(() => window.__mock.state.eq?.[5] === 2);
    assert.equal(await page.locator(".eq select").inputValue(), "custom");
    assert.equal(await page.evaluate(() => window.__mock.count("seek")), 0, "arrows on a slider never seek");

    await page.uncheck("#eq-on");
    await page.waitForFunction(() => window.__mock.state.eq === null);

    await page.check("#eq-on");
    await page.waitForFunction(() => window.__mock.state.eq?.[5] === 2);
    await page.reload();
    await waitForLibrary(page);
    await page.waitForFunction(() => window.__mock.state.eq?.[0] === 7 && window.__mock.state.eq?.[5] === 2);
    if (process.env.PM_SCREENSHOTS) {
      await page.click(".nav button[data-view=settings]");
      await page.locator(".eq").screenshot({ path: `${process.env.PM_SCREENSHOTS}/eq.png` });
    }
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});
