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

test("pending sign-in can be brought back, reports verification and recovers after cancellation", async () => {
  const app = await openApp({ config: { signedIn: false, loginPending: true } });
  try {
    const { page } = app;
    await page.click(".login .btn");
    await page.getByRole("button", { name: "Я уже вошёл — проверить" }).click();
    assert.equal(await page.evaluate(() => window.__mock.count("login_start")), 2);
    await page.evaluate(() => window.__mock.emit("pm-login", "pending:unauthorized"));
    await page.waitForFunction(() => document.querySelector(".login .message").textContent.includes("VK пока не подтвердил"));
    await page.evaluate(() => window.__mock.emit("pm-login", "cancelled"));
    await page.getByRole("button", { name: "Войти через VK" }).click();
    await page.evaluate(() => window.__mock.emit("pm-login", "error:timeout"));
    await page.waitForFunction(() => document.querySelector(".login .message").textContent === "Сервер не отвечает.");
    await page.click(".login .btn");
    await page.evaluate(() => {
      window.__mock.state.session.signed_in = true;
      window.__mock.emit("pm-login", "success");
    });
    await waitForLibrary(page);
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
    await second.locator("[data-action=menu]").click();
    await page.click(".menu [data-action=play-last]");
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
    assert.equal(await page.locator(".card").count(), 3);
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
    // The same element shows "Ищем текст…" while the request is in flight.
    await page.waitForFunction(() => document.querySelector(".ly-message")?.textContent === "Текст не найден.");
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
    await rows(page).nth(1).click({ button: "right" });
    await page.click(".menu [data-action=radio]");
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
    await rows(page).first().locator("[data-action=menu]").click();
    await page.click(".menu [data-action=radio]");
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

// ---------------------------------------------------------------- downloads

test("downloads: a row button saves the track, shows it everywhere, lists it with its size, and removes it", async () => {
  const app = await openApp({ config: { signedIn: true, downloadDelay: 200 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).nth(1).hover();
    const button = rows(page).nth(1).locator("[data-action=download]");
    await button.click();
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row")[1]?.querySelector(".download.busy"));
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row")[1]?.querySelector(".download.on"));
    assert.match(await page.locator(".toast").innerText(), /Скачано: Трек 2/);
    assert.equal(await rows(page).nth(1).locator(".download").isVisible(), true, "a downloaded mark stays visible");

    await page.click(".nav button[data-view=downloads]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Трек 2");
    assert.match(await page.locator(".main > .view:not([hidden]) .meta").innerText(), /1 трек.*4 МБ на диске/);

    await rows(page).first().hover();
    await rows(page).first().locator("[data-action=download]").click();
    await page.waitForSelector(".main > .view:not([hidden]) .empty-state:not([hidden])");
    assert.match(await page.locator(".toast").innerText(), /Удалено с компьютера/);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("downloads: a failed download says why and leaves the button ready", async () => {
  const app = await openApp({ config: { signedIn: true, downloadFails: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().hover();
    await rows(page).first().locator("[data-action=download]").click();
    await page.waitForSelector(".toast.error");
    assert.equal(await rows(page).first().locator(".download").getAttribute("aria-pressed"), "false");
    assert.equal(await rows(page).first().locator(".download").isDisabled(), false);
  } finally { await app.close(); }
});

// ---------------------------------------------------------------- playlists

test("playlists: create one, add a track to it from the menu, rename it, remove the track, delete it", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    // Add to a new playlist straight from a library row.
    await rows(page).nth(2).click({ button: "right" });
    await page.click(".menu [data-action=add-to-playlist]");
    await page.waitForSelector(".menu .menu-item[data-action=add-to]");
    assert.deepEqual(await page.locator(".menu .menu-item[data-action=add-to] .menu-label").allInnerTexts(), ["Дорога", "Работа"], "only own playlists");
    await page.click(".menu [data-action=new-playlist]");
    await page.fill(".dialog input[name=title]", "  Ночь  ");
    await page.fill(".dialog textarea[name=description]", "для поездок");
    await page.click(".dialog .btn.primary");
    await page.waitForFunction(() => window.__mock.state.lastAdded?.playlist === "Ночь");
    assert.equal(await page.evaluate(() => window.__mock.state.lastAdded.track), "Трек 3");

    await page.click(".nav button[data-view=playlists]");
    await page.waitForFunction(() => document.querySelector(".grid .card .name")?.textContent === "Ночь");
    assert.match(await page.locator(".grid .card").first().innerText(), /1 треков/);
    await page.click(".grid .card >> nth=0");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .view:not([hidden]) .row .title")?.textContent === "Трек 3");

    await page.click(".pl-owner .btn >> nth=0");
    await page.fill(".dialog input[name=title]", "Ночная дорога");
    await page.click(".dialog .btn.primary");
    await page.waitForFunction(() => [...document.querySelectorAll(".view-header h1")].some((e) => e.textContent === "Ночная дорога"));

    const row = page.locator(".main > .view:not([hidden]) .view:not([hidden]) .row").first();
    await row.click({ button: "right" });
    await page.click(".menu [data-action=remove-from-playlist]");
    await page.waitForSelector(".main > .view:not([hidden]) .view:not([hidden]) .empty-state:not([hidden])");

    await page.click(".pl-owner .btn >> nth=1");
    await page.click(".dialog .btn.danger");
    await page.waitForFunction(() => ![...document.querySelectorAll(".grid .card .name")].some((e) => e.textContent.startsWith("Ночн")));
    assert.match(await page.locator(".toast").innerText(), /удалён/);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("playlists: someone else's playlist cannot be edited, and the menu closes with Escape", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=playlists]");
    await page.waitForSelector(".grid .card");
    await page.locator(".grid .card", { hasText: "Чужой" }).click();
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .view:not([hidden]) .row"));
    assert.equal(await page.locator(".pl-owner .btn >> nth=0").isVisible(), false);
    const row = page.locator(".main > .view:not([hidden]) .view:not([hidden]) .row").first();
    await row.click({ button: "right" });
    assert.equal(await page.locator(".menu [data-action=remove-from-playlist]").count(), 0);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => !document.querySelector(".menu"));
    // Cancelling a new playlist creates nothing.
    await page.click(".view-header .btn.primary >> nth=0").catch(() => {});
  } finally { await app.close(); }
});

test("playlists: an empty name cannot be submitted, and cancel leaves things as they were", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=playlists]");
    await page.waitForSelector(".grid .card");
    await page.click(".main > .view:not([hidden]) .view:not([hidden]) .view-header .btn.primary");
    await page.waitForSelector(".dialog[open]");
    assert.equal(await page.locator(".dialog .btn.primary").isDisabled(), true);
    await page.fill(".dialog input[name=title]", "   ");
    assert.equal(await page.locator(".dialog .btn.primary").isDisabled(), true);
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => !document.querySelector(".dialog"));
    assert.equal(await page.evaluate(() => window.__mock.count("create_playlist")), 0);
  } finally { await app.close(); }
});

// ------------------------------------------------------ artists and albums

const visibleView = ".main > .view:not([hidden])";

async function openArtistFromRow(page, index = 0) {
  await waitForLibrary(page);
  await rows(page).nth(index).locator(".artist").click();
  await page.waitForSelector(".artist-view:not([hidden])");
}

test("artist page: opens from a track row's artist, plays its tracks with the visible list as the queue", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await openArtistFromRow(page, 0);
    assert.equal(await page.locator(".artist-view h1").innerText(), "Исполнитель 1");
    assert.equal((await page.evaluate(() => window.__mock.last("artist_tracks"))).args.artistId, "art1");
    await page.waitForFunction(() => document.querySelectorAll(".artist-view .row").length > 5);
    // The catalogue's photo and the albums grid arrive with the albums.
    await page.waitForSelector(".artist-view .page-art img");
    await page.waitForFunction(() => document.querySelectorAll(".artist-view .artist-albums .card").length === 4);
    assert.equal(await page.locator(".nav button[aria-current=page]").getAttribute("data-view"), "library", "the sidebar keeps the screen it came from");

    await page.locator(".artist-view .row").nth(2).dblclick();
    const call = await page.evaluate(() => window.__mock.last("play_tracks"));
    assert.equal(call.args.index, 2);
    assert.equal(call.args.tracks.length, 30);
    assert.ok(call.args.tracks.every((t) => t.title.startsWith("Хит")));
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Хит 3");

    await page.click(".artist-view button[aria-label=Назад]");
    await page.waitForSelector(".artist-view", { state: "hidden" });
    assert.equal(await page.locator(`${visibleView} h1`).first().innerText(), "Моя музыка");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("artist page: an artist known only by name is looked up, and an unknown one falls back to search", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    // Track 13 links no artist: "Исполнитель 0" is not known to the mock.
    await page.fill(`${visibleView} input[type=search]`, "Трек 13");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Трек 13");
    await rows(page).first().locator(".artist").click();
    await page.waitForSelector(".artist-view .page-note:not([hidden])");
    assert.equal((await page.evaluate(() => window.__mock.last("find_artist"))).args.name, "Исполнитель 0");
    await page.waitForFunction(() => window.__mock.count("search_albums") === 1 && window.__mock.count("search") === 1);
    assert.equal(await page.evaluate(() => window.__mock.count("artist_tracks")), 0);
  } finally { await app.close(); }
});

test("album page: opens from the artist's albums, plays, follows and unfollows; the Albums shelf follows", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await openArtistFromRow(page, 4); // Исполнитель 5
    await page.waitForSelector(".artist-view .artist-albums .card");
    await page.locator(".artist-view .artist-albums .card").first().click();
    await page.waitForSelector(".album-view:not([hidden])");
    await page.waitForFunction(() => document.querySelectorAll(".album-view .row").length === 12);
    assert.equal(await page.locator(".album-view h1").innerText(), "Альбом 1");
    assert.match(await page.locator(".album-view .page-note").innerText(), /2011 · 12 треков/);
    assert.equal(await page.locator(".album-view .page-artists").innerText(), "Исполнитель 5");
    const asked = await page.evaluate(() => window.__mock.last("album_tracks"));
    assert.deepEqual([asked.args.album.album_id, asked.args.album.owner_id, asked.args.album.access_key], [1, -2000, "k"]);

    await page.locator(".album-view .row").first().dblclick();
    const call = await page.evaluate(() => window.__mock.last("play_tracks"));
    assert.equal(call.args.tracks.length, 12);

    const follow = page.locator(".album-view .follow");
    assert.equal(await follow.innerText(), "Сохранить альбом");
    await follow.click();
    await page.waitForFunction(() => document.querySelector(".album-view .follow").getAttribute("aria-pressed") === "true");
    assert.equal((await page.evaluate(() => window.__mock.last("follow_album"))).args.follow, true);
    assert.equal(await follow.innerText(), "Убрать из альбомов");
    assert.match(await page.locator(".toast").innerText(), /сохранён/);

    await page.click(".nav button[data-view=playlists]");
    await page.waitForFunction(() => [...document.querySelectorAll(".followed-albums .card .name")].map((e) => e.textContent).join() === "Альбом 1");
    await page.locator(".followed-albums .card").first().click();
    await page.waitForSelector(".album-view:not([hidden])");
    await page.waitForFunction(() => document.querySelector(".album-view .follow").getAttribute("aria-pressed") === "true");
    await page.click(".album-view .follow");
    await page.waitForFunction(() => document.querySelector(".album-view .follow").getAttribute("aria-pressed") === "false");
    assert.equal((await page.evaluate(() => window.__mock.last("follow_album"))).args.follow, false);
    await page.click(".album-view button[aria-label=Назад]");
    await page.waitForFunction(() => document.querySelectorAll(".followed-albums .card").length === 0);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("album page: a refused follow leaves the button as it was; an album VK will not open says so", async () => {
  const app = await openApp({ config: { signedIn: true, followFails: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).nth(1).click({ button: "right" }); // track 2 names its album
    await page.click(".menu [data-action=go-album]");
    await page.waitForSelector(".album-view:not([hidden])");
    await page.waitForFunction(() => document.querySelectorAll(".album-view .row").length === 12);
    // The engine found the key the track did not carry.
    assert.equal((await page.evaluate(() => window.__mock.last("album_tracks"))).args.album.access_key, null);
    await page.click(".album-view .follow");
    await page.waitForSelector(".toast.error");
    assert.equal(await page.locator(".album-view .follow").getAttribute("aria-pressed"), "false");
    assert.equal(await page.locator(".album-view .follow").isDisabled(), false);
  } finally { await app.close(); }
  const denied = await openApp({ config: { signedIn: true, albumDenied: true } });
  try {
    const { page } = denied;
    await waitForLibrary(page);
    await rows(page).nth(1).click({ button: "right" });
    await page.click(".menu [data-action=go-album]");
    await page.waitForSelector(".album-view .status.error");
    assert.match(await page.locator(".album-view .status.error").innerText(), /Не удалось загрузить треки этого альбома/);
  } finally { await denied.close(); }
});

test("track menu: go to artist and album only when the track has them; several artists get a sub-menu", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().click({ button: "right" }); // track 1: an artist, no album
    assert.equal(await page.locator(".menu [data-action=go-artist]").count(), 1);
    assert.equal(await page.locator(".menu [data-action=go-album]").count(), 0);
    await page.keyboard.press("Escape");
    await rows(page).nth(3).click({ button: "right" }); // track 4: two artists and an album
    await page.click(".menu [data-action=go-artist]");
    assert.deepEqual(await page.locator(".menu [data-action=go-artist-item] .menu-label").allInnerTexts(), ["Исполнитель 4", "Гость"]);
    await page.click(".menu [data-action=go-artist-item] >> nth=1");
    await page.waitForFunction(() => document.querySelector(".artist-view:not([hidden]) h1")?.textContent === "Гость");
    assert.equal((await page.evaluate(() => window.__mock.last("artist_tracks"))).args.artistId, "art99");
  } finally { await app.close(); }
});

test("back navigation: playlist → artist → album → back → back returns to the open playlist", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=playlists]");
    await page.locator(".grid .card", { hasText: "Дорога" }).click();
    await page.waitForFunction(() => [...document.querySelectorAll(".row .title")].some((e) => e.textContent.startsWith("Дорога:")));
    const playlistRow = page.locator(`${visibleView} .view:not([hidden]) .row`).first();
    await playlistRow.click({ button: "right" });
    await page.click(".menu [data-action=go-artist]");
    await page.waitForSelector(".artist-view .artist-albums .card");
    assert.equal(await page.locator(".nav button[aria-current=page]").getAttribute("data-view"), "playlists");
    await page.locator(".artist-view .artist-albums .card").nth(1).click();
    await page.waitForFunction(() => document.querySelector(".album-view:not([hidden]) h1")?.textContent === "Альбом 2");

    await page.click(".album-view button[aria-label=Назад]");
    await page.waitForSelector(".artist-view:not([hidden])");
    assert.equal(await page.locator(".artist-view h1").innerText(), "Исполнитель 1");
    assert.equal(await page.evaluate(() => window.__mock.count("artist_tracks")), 1, "the artist page comes back as it was");
    await page.click(".artist-view button[aria-label=Назад]");
    await page.waitForSelector(".artist-view", { state: "hidden" });
    await page.waitForFunction(() => [...document.querySelectorAll(".main > .view:not([hidden]) .view:not([hidden]) .row .title")].some((e) => e.textContent.startsWith("Дорога:")));
    assert.equal(await page.evaluate(() => window.__mock.count("playlist_tracks")), 1, "the playlist was not reloaded");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("search: Artists and Albums tabs search VK, open their pages, and keep the tab on the way back", async () => {
  const hostile = `<img src=x onerror="window.__pwned=1">`;
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=search]");
    const input = page.locator(`${visibleView} input[type=search]`);
    await input.fill(hostile);
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length > 0);
    assert.equal(await page.evaluate(() => window.__mock.count("search_artists")), 0, "other tabs load when shown");

    await page.click(`${visibleView} [role=tab][data-tab=artists]`);
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .artist-card").length === 2);
    assert.equal((await page.evaluate(() => window.__mock.last("search_artists"))).args.query, hostile);
    assert.equal(await page.locator(`${visibleView} .artist-card .name`).first().innerText(), `${hostile} артист`);
    assert.equal(await page.locator(`${visibleView} .tab-pane:not([hidden]) .row`).count(), 0, "the track list is hidden");
    await page.locator(`${visibleView} .artist-card`).nth(1).click();
    await page.waitForFunction(() => document.querySelector(".artist-view:not([hidden]) h1")?.textContent === "Исполнитель 2");
    assert.equal(await page.locator(".nav button[aria-current=page]").getAttribute("data-view"), "search");
    await page.click(".artist-view button[aria-label=Назад]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) [role=tab][aria-selected=true]")?.dataset.tab === "artists");

    await page.click(`${visibleView} [role=tab][data-tab=albums]`);
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .album-card").length === 5);
    await page.locator(`${visibleView} .album-card`).first().click();
    await page.waitForSelector(".album-view:not([hidden])");
    assert.equal(await page.locator(".album-view h1").innerText(), `${hostile} альбом 1`);

    await page.click(".album-view button[aria-label=Назад]");
    await input.fill("empty");
    await page.waitForSelector(`${visibleView} .empty-state:not([hidden])`);
    assert.match(await page.locator(`${visibleView} .tab-pane:not([hidden]) .empty-state`).innerText(), /Альбомы не найдены/);
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    assert.equal(await page.locator("img[src=x]").count(), 0);
  } finally { await app.close(); }
});

test("artist page: opens from the player bar and from the full-screen view", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).nth(5).dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 6");
    await page.click(".player .artist");
    await page.waitForFunction(() => document.querySelector(".artist-view:not([hidden]) h1")?.textContent === "Исполнитель 6");
    await page.click(".artist-view button[aria-label=Назад]");
    await page.waitForSelector(".artist-view", { state: "hidden" });

    await page.click(".player .volume .icon-btn:last-child");
    await page.waitForSelector(".np:not([hidden])");
    await page.click(".np-artist");
    await page.waitForSelector(".np", { state: "hidden" });
    await page.waitForSelector(".artist-view:not([hidden])");
    assert.equal(await page.evaluate(() => window.__mock.state.visualizer), false, "closing the view stops the analysis");
  } finally { await app.close(); }
});

// ------------------------------------------------- friends, history, statistics

const view = ".main > .view:not([hidden])";

// Nested screens keep hidden siblings in the DOM: count what is really on screen.
const shown = (page, selector) => page.locator(`.main ${selector}:visible`);
const waitShown = (page, selector, n) =>
  page.waitForFunction(([sel, count]) => [...document.querySelectorAll(`.main ${sel}`)].filter((e) => e.offsetParent !== null).length === count, [selector, n]);

async function openSection(page, id) {
  await page.click(`.nav button[data-view=${id}]`);
}

test("friends: the list shows names and photos, and a friend opens into tracks that play, like and have a menu", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "friends");
    await page.waitForFunction(() => document.querySelectorAll(".friend").length === 3);
    assert.deepEqual(await page.locator(".friend .friend-name").allInnerTexts(), ["Анна Смирнова", "Борис Закрытый", "Ёлка Пустая"]);
    assert.equal(await page.locator(".friend img.avatar").count(), 1, "only Анна has a photo; the others get a placeholder");
    assert.equal(await page.locator(".friend .avatar.empty").count(), 2);
    assert.equal(await page.locator(".nav button[data-view=friends]").getAttribute("aria-current"), "page");

    await page.click(".friend >> nth=0");
    await waitShown(page, ".row", 5);
    assert.equal(await shown(page, "h1").innerText(), "Анна Смирнова");
    assert.equal(await shown(page, ".friend-photo img").count(), 1);
    assert.equal(await page.evaluate(() => window.__mock.last("friend_tracks").args.ownerId), 11);
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);

    await shown(page, ".row").nth(2).dblclick();
    const play = await page.evaluate(() => window.__mock.last("play_tracks"));
    assert.equal(play.args.index, 2);
    assert.equal(play.args.tracks.length, 5);
    assert.ok(play.args.tracks.every((t) => t.owner_id === 11));
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Друг 11: трек 3");

    // The heart adds to My music, as everywhere else.
    await shown(page, ".row").nth(1).hover();
    await shown(page, ".row").nth(1).locator("[data-action=like]").click();
    await page.waitForFunction(() => window.__mock.count("add_to_library") === 1);
    assert.equal(await page.evaluate(() => window.__mock.last("add_to_library").args.track.title), "Друг 11: трек 2");

    // The track menu is the usual one.
    await shown(page, ".row").nth(3).click({ button: "right" });
    await page.waitForSelector(".menu [data-action=play-next]");
    assert.ok(await page.locator(".menu [data-action=add-to-playlist]").count() > 0);
    await page.keyboard.press("Escape");
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

test("friends: playlists open into tracks, with Back at every step", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "friends");
    await page.waitForSelector(".friend");
    await page.click(".friend >> nth=0");
    await waitShown(page, ".row", 5);
    await shown(page, "[role=tab][data-tab=playlists]").click();
    await waitShown(page, ".friend-playlists .card", 2);
    assert.equal(await page.evaluate(() => window.__mock.last("friend_playlists").args.ownerId), 11);
    assert.equal(await shown(page, ".row").count(), 0, "the tracks tab is hidden");

    await page.locator(".friend-playlists .card", { hasText: "Плейлист друга" }).click();
    await waitShown(page, ".row", 3);
    assert.ok((await shown(page, ".row .title").first().innerText()).startsWith("Плейлист друга:"));
    const opened = await page.evaluate(() => window.__mock.last("playlist_tracks").args.playlist);
    assert.deepEqual([opened.owner_id, opened.playlist_id, opened.access_key], [11, 1, "fk"]);
    // Playing from here queues the playlist.
    await shown(page, ".row").first().dblclick();
    assert.equal((await page.evaluate(() => window.__mock.last("play_tracks"))).args.tracks.length, 3);
    // Someone else's playlist offers no editing.
    assert.equal(await shown(page, ".pl-owner .btn").count(), 0);

    await shown(page, ".icon-btn[aria-label=Назад]").click();
    await shown(page, "[role=tab][data-tab=playlists][aria-selected=true]").waitFor();
    await shown(page, ".icon-btn[aria-label='К друзьям']").click();
    await waitShown(page, ".friend", 3);
    // Another friend starts on Tracks again.
    await page.click(".friend >> nth=2");
    await shown(page, "[role=tab][data-tab=tracks][aria-selected=true]").waitFor();
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

test("friends: private audio gets a plain message, an empty friend an empty state", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "friends");
    await page.waitForSelector(".friend");
    await page.click(".friend >> nth=1");
    await shown(page, ".status.error").first().waitFor();
    const text = await shown(page, ".status.error").first().innerText();
    assert.equal(text, "Музыка может быть скрыта владельцем или недоступна для этой сессии VK.");
    assert.ok(!text.includes("VK error"), "VK's own wording is not shown");
    assert.equal(await shown(page, ".row").count(), 0);
    await shown(page, "[role=tab][data-tab=playlists]").click();
    await shown(page, "[data-pane=playlists] .status.error").waitFor();
    assert.equal(await shown(page, "[data-pane=playlists] .status.error").innerText(), text);

    await shown(page, ".icon-btn[aria-label='К друзьям']").click();
    await page.click(".friend >> nth=2");
    await shown(page, "[data-pane=tracks] .empty-state").waitFor();
    assert.equal(await shown(page, "[data-pane=tracks] .empty-state").innerText(), "В этом разделе нет доступной музыки.");
    await shown(page, "[role=tab][data-tab=playlists]").click();
    await page.waitForFunction(() => [...document.querySelectorAll(".main [data-pane=playlists] .status")].some((e) => e.offsetParent && e.textContent.includes("нет или они скрыты")));
  } finally { await app.close(); }
});

test("friends: a network failure is shown as such, and a long list loads completely", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["friends"] } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "friends");
    await shown(page, ".status.error").first().waitFor();
    assert.equal(await shown(page, ".status.error").first().innerText(), "Нет подключения к интернету.");
  } finally { await app.close(); }
  const long = await openApp({ config: { signedIn: true, friendsTotal: 450 } });
  try {
    const { page } = long;
    await waitForLibrary(page);
    await openSection(page, "friends");
    await page.waitForFunction(() => document.querySelectorAll(".friend").length === 450);
    assert.equal(await page.evaluate(() => window.__mock.count("friends")), 3, "200 + 200 + 50");
  } finally { await long.close(); }
});

test("friends: names are text, never markup", async () => {
  const hostile = `<img src=x onerror="window.__pwned=1">`;
  const app = await openApp({ config: { signedIn: true, hostileFriend: hostile } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "friends");
    await page.waitForSelector(".friend");
    assert.ok((await page.locator(".friend .friend-name").first().innerText()).startsWith("<img"));
    await page.click(".friend >> nth=0");
    await shown(page, "h1").waitFor();
    assert.ok((await shown(page, "h1").innerText()).startsWith("<img"));
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    assert.equal(await page.locator("img[src=x]").count(), 0);
  } finally { await app.close(); }
});

test("history: recent tracks play as a queue, filter, drop one at a time and clear", async () => {
  const app = await openApp({ config: { signedIn: true, history: [1, 2, 3, 4] } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "history");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length === 4);
    assert.deepEqual(await rows(page).locator(".title").allInnerTexts(), ["Из истории 1", "Из истории 2", "Из истории 3", "Из истории 4"]);
    assert.match(await page.locator(`${view} .footer-note`).innerText(), /только на этом компьютере/);

    await rows(page).nth(1).dblclick();
    const play = await page.evaluate(() => window.__mock.last("play_tracks"));
    assert.equal(play.args.index, 1);
    assert.equal(play.args.tracks.length, 4);

    await page.fill(`${view} input[type=search]`, "истории 3");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length === 1);
    await page.fill(`${view} input[type=search]`, "");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length === 4);

    await rows(page).nth(1).click({ button: "right" });
    await page.click(".menu [data-action=remove-from-history]");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length === 3);
    assert.equal(await page.evaluate(() => window.__mock.last("history_remove").args.trackId), "7_2");
    assert.equal(await page.evaluate(() => window.__mock.state.history.length), 3);

    await page.click(`${view} .view-header .btn.danger-text`);
    await page.click(".dialog .btn:not(.danger)");
    assert.equal(await page.evaluate(() => window.__mock.count("history_clear")), 0, "cancelling keeps the history");
    await page.click(`${view} .view-header .btn.danger-text`);
    await page.click(".dialog .btn.danger");
    await page.waitForSelector(`${view} .empty-state:not([hidden])`);
    assert.equal(await page.locator(`${view} .empty-state`).innerText(), "История пуста. Здесь появятся треки, которые вы прослушали.");
    assert.equal(await page.locator(`${view} .view-header .btn.danger-text`).isDisabled(), true);
    assert.equal(await page.evaluate(() => window.__mock.state.history.length), 0);
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

test("history: a track counted while another plays shows up when the track changes", async () => {
  const app = await openApp({ config: { signedIn: true, history: [1] } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "history");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .row").length === 1);
    const before = await page.evaluate(() => window.__mock.count("history"));
    // The engine counts the second track while the first one plays.
    await page.evaluate(() => {
      window.__mock.state.history.unshift({ track: { ...window.__mock.state.history[0].track, track_id: 77, title: "Новый в истории" }, played_at: 1 });
    });
    assert.equal(await rows(page).count(), 1, "nothing reloads by itself");
    await rows(page).first().dblclick();
    await page.waitForFunction((n) => window.__mock.count("history") > n, before);
    await page.waitForFunction(() => [...document.querySelectorAll(".main > .view:not([hidden]) .row .title")].some((e) => e.textContent === "Новый в истории"));
  } finally { await app.close(); }
});

test("history: empty history shows its message and nothing to clear", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "history");
    await page.waitForSelector(`${view} .empty-state:not([hidden])`);
    assert.equal(await page.locator(`${view} .view-header .btn.danger-text`).isDisabled(), true);
  } finally { await app.close(); }
});

test("statistics: totals, tops, busiest times, and the period filter", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "stats");
    await page.waitForSelector(`${view} [data-stat=time]`);
    const text = (selector) => page.locator(`${view} ${selector}`).innerText();
    assert.equal(await text("[data-stat=time]"), "1 ч 30 мин");
    assert.equal(await text("[data-stat=plays]"), "40");
    assert.equal(await text("[data-stat=days]"), "6");
    assert.deepEqual(await page.locator(`${view} [data-section=artists] .stat-row`).allInnerTexts().then((rowsText) => rowsText.map((r) => r.replace(/\s+/g, " "))), ["1 Кино ×12", "2 Исполнитель 2 ×7"]);
    assert.match(await page.locator(`${view} [data-section=tracks] .stat-row`).first().innerText(), /Группа крови\s+Кино\s+×5/);
    assert.equal(await page.locator(`${view} .hour-bar`).count(), 24);
    assert.equal(await page.locator(`${view} .hour-bar.peak`).getAttribute("data-hour"), "19");
    assert.equal(await text("[data-fact=hour]"), "Чаще всего: 19:00");
    assert.equal(await text("[data-fact=weekday]"), "Любимый день недели: среда");
    assert.match(await text("[data-fact=day]"), /^Самый музыкальный день: 4 марта$/);
    assert.match(await text(".page-note"), /не отправляются в VK/);
    assert.match(await text(".stat-controls"), /март 2026/);

    // Two months have data: the chooser lists them, newest first.
    assert.deepEqual(await page.locator(`${view} .stat-controls select option`).evaluateAll((o) => o.map((e) => e.value)), ["2026-03", "2026-02"]);
    await page.selectOption(`${view} .stat-controls select`, "2026-02");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) [data-stat=plays]")?.textContent === "25");
    assert.deepEqual((await page.evaluate(() => window.__mock.last("listening_stats"))).args, { scope: "month", choice: "2026-02" });

    await page.click(`${view} [role=tab][data-scope=year]`);
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) [data-stat=plays]")?.textContent === "65");
    assert.equal((await page.evaluate(() => window.__mock.last("listening_stats"))).args.scope, "year");
    assert.equal(await page.locator(`${view} .stat-controls select`).isHidden(), true, "one year: nothing to choose");
    assert.equal(await text(".stat-period"), "2026");

    await page.click(`${view} [role=tab][data-scope=all]`);
    await page.waitForFunction(() => window.__mock.last("listening_stats").args.scope === "all" && document.querySelector(".main > .view:not([hidden]) .stat-controls .stat-period")?.hidden);
    assert.equal(await page.locator(`${view} [role=tab][data-scope=all]`).getAttribute("aria-selected"), "true");
    assert.equal(await page.locator(`${view} [data-stat=plays]`).innerText(), "65");
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

test("statistics: reset asks first, then shows the empty state; privacy note stays", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "stats");
    await page.waitForSelector(`${view} [data-stat=time]`);
    await page.click(`${view} .stat-foot .btn`);
    assert.match(await page.locator(".dialog .dialog-text").innerText(), /История прослушивания останется/);
    await page.click(".dialog .btn:not(.danger)");
    assert.equal(await page.evaluate(() => window.__mock.count("reset_listening_stats")), 0);
    await page.click(`${view} .stat-foot .btn`);
    await page.click(".dialog .btn.danger");
    await page.waitForSelector(`${view} .stat-empty`);
    assert.equal(await page.evaluate(() => window.__mock.count("reset_listening_stats")), 1);
    assert.match(await page.locator(`${view} .stat-empty`).innerText(), /Пока мало данных/);
    assert.equal(await page.locator(`${view} .stat-foot .btn`).isHidden(), true, "nothing left to reset");
    assert.match(await page.locator(`${view} .stat-foot .page-note`).innerText(), /только на этом устройстве/);
    assert.equal(await page.locator(`${view} [data-stat=time]`).count(), 0);
  } finally { await app.close(); }
});

test("statistics: empty data and failures are explained", async () => {
  const empty = await openApp({ config: { signedIn: true, statsEmpty: true } });
  try {
    const { page } = empty;
    await waitForLibrary(page);
    await openSection(page, "stats");
    await page.waitForSelector(`${view} .stat-empty`);
    assert.match(await page.locator(`${view} .stat-controls`).innerText(), /\S/, "the current month is named even without data");
    assert.equal(await page.locator(`${view} .stat-foot .btn`).isHidden(), true);
  } finally { await empty.close(); }
  const failing = await openApp({ config: { signedIn: true, statsFail: true } });
  try {
    const { page } = failing;
    await waitForLibrary(page);
    await openSection(page, "stats");
    await page.waitForSelector(`${view} .status.error`);
    assert.equal(await page.locator(`${view} .status.error`).innerText(), "Invalid request.");
  } finally { await failing.close(); }
});

test("the window tells the engine its UTC offset, and the English UI has the new sections", async () => {
  const app = await openApp({ config: { signedIn: true }, language: "en-US" });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.waitForFunction(() => window.__mock.count("set_utc_offset") > 0);
    const expected = await page.evaluate(() => -new Date().getTimezoneOffset());
    assert.equal(await page.evaluate(() => window.__mock.last("set_utc_offset").args.minutes), expected);
    assert.deepEqual(await page.locator(".nav button .label").allInnerTexts(), ["My music", "For you", "Playlists", "Friends", "Search", "Queue", "History", "Statistics", "Downloads", "Settings"]);
    await openSection(page, "stats");
    await page.waitForSelector(`${view} [data-stat=time]`);
    assert.equal(await page.locator(`${view} [data-stat=time]`).innerText(), "1 h 30 min");
    assert.equal(await page.locator(`${view} [data-fact=weekday]`).innerText(), "Favourite day of the week: Wednesday");
    assert.equal(await page.locator(`${view} [data-fact=day]`).innerText(), "Busiest day: March 4");
    await openSection(page, "friends");
    await page.waitForSelector(".friend");
    assert.equal(await shown(page, "h1").innerText(), "Friends' music");
  } finally { await app.close(); }
});

test("the sidebar still fits the smallest window with the new sections", async () => {
  const app = await openApp({ config: { signedIn: true }, viewport: { width: 780, height: 540 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    const fits = await page.evaluate(() => {
      const sidebar = document.querySelector(".sidebar");
      const account = document.querySelector(".account").getBoundingClientRect();
      return { overflow: sidebar.scrollHeight > sidebar.clientHeight, accountBottom: account.bottom, sidebarBottom: sidebar.getBoundingClientRect().bottom };
    });
    assert.equal(fits.overflow, false);
    assert.ok(fits.accountBottom <= fits.sidebarBottom + 1);
    // The last entry can be reached.
    await page.locator(".nav button[data-view=settings]").scrollIntoViewIfNeeded();
    await page.click(".nav button[data-view=settings]");
    await page.waitForSelector(".settings:not([hidden])");
  } finally { await app.close(); }
});
