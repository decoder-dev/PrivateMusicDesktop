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
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent.startsWith("Трек"));
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
      for (const el of document.querySelectorAll("*")) {
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
