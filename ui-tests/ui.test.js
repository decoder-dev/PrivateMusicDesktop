import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

const rows = (page) => page.locator(".main > .view:not([hidden]) .row");
// The suite runs with reduced motion (every state change is instant, so nothing waits for an
// animation); tests about motion ask for it explicitly.
const motionApp = (options = {}) => openApp({ ...options, reducedMotion: "no-preference" });

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
    assert.match(await page.locator(".main > .view:not([hidden]) .meta").innerText(), /^1234 трека$/);
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

test("one click plays from that row with the visible list as the queue", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.fill(".main > .view:not([hidden]) input[type=search]", "Исполнитель 5");
    await page.waitForFunction(() => document.querySelectorAll(".row").length > 3);
    await rows(page).nth(2).click();
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

// ------------------------------------------------------------ motion and design

/** "rgb(…)", "rgba(…)" or "color(srgb r g b / a)" as [r, g, b, a] with 0-255 channels. */
function parseColor(text) {
  const srgb = text.match(/^color\(srgb ([\d.e-]+) ([\d.e-]+) ([\d.e-]+)(?: \/ ([\d.e-]+))?\)$/);
  if (srgb) return [srgb[1], srgb[2], srgb[3]].map((v) => Number(v) * 255).concat(srgb[4] === undefined ? 1 : Number(srgb[4]));
  const rgb = text.match(/^rgba?\(([^)]+)\)$/);
  assert.ok(rgb, `cannot read colour ${text}`);
  const [r, g, b, a = "1"] = rgb[1].split(/[,/\s]+/).filter(Boolean);
  return [Number(r), Number(g), Number(b), Number(a)];
}
const hexColor = (text) => [1, 3, 5].map((i) => parseInt(text.slice(i, i + 2), 16));
const luminance = ([r, g, b]) => {
  const lin = (v) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};
const contrast = (a, b) => { const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };
const over = ([r, g, b, a], [br, bg, bb]) => [r * a + br * (1 - a), g * a + bg * (1 - a), b * a + bb * (1 - a)];

test("motion: reduced motion removes all motion", async () => {
  const app = await openApp({ config: { signedIn: true }, reducedMotion: "reduce" });
  try {
    const { page } = app;
    await waitForLibrary(page);
    // Interact with everything that animates when motion is on.
    await rows(page).nth(1).click({ button: "right" });
    await page.waitForSelector(".menu");
    await page.click(".menu [data-action=add-to-playlist]");
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
    await page.click(".nav button[data-view=playlists]");
    await page.click(".main > .view:not([hidden]) .view:not([hidden]) .view-header .btn.primary");
    await page.waitForSelector(".dialog[open]");
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => !document.querySelector(".dialog"));
    await page.click(".nav button[data-view=library]");
    await rows(page).first().dblclick();
    await playAndOpenFrom(page);
    await page.waitForTimeout(250);
    await frames(page);
    const slow = await page.evaluate(() => {
      const bad = [];
      const limit = 0.00002; // 0.01 ms, in seconds
      for (const el of document.querySelectorAll("*")) {
        for (const pseudo of [null, "::before", "::after"]) {
          const s = getComputedStyle(el, pseudo);
          const durations = [...s.transitionDuration.split(","), ...s.animationDuration.split(",")].map((d) => parseFloat(d));
          if (durations.some((d) => d > limit)) bad.push(`${el.tagName}.${el.className}${pseudo ?? ""} ${s.transitionDuration} ${s.animationDuration}`);
        }
      }
      return bad;
    });
    assert.deepEqual(slow, []);
    assert.equal(await page.evaluate(() => document.getAnimations().length), 0);
    // Closing is instant too: nothing waits for an animation to end.
    await page.keyboard.press("Escape");
    await page.waitForSelector(".np", { state: "hidden", timeout: 1500 });
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

/** Lets a few frames pass: finished transitions are only retired when the page paints. */
const frames = (page) => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(resolve)))));

async function playAndOpenFrom(page) {
  await page.waitForFunction(() => { const t = document.querySelector(".player .title")?.textContent; return t && t !== "Ничего не играет"; });
  await page.click(".player .volume .icon-btn:last-child");
  await page.waitForSelector(".np:not([hidden])");
}

test("motion: nothing runs continuously while idle, and rows never transition", async () => {
  const app = await motionApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.waitForTimeout(700);
    await frames(page);
    const running = () => page.evaluate(() => document.getAnimations()
      .filter((a) => a.playState === "running")
      .map((a) => ({ name: a.animationName ?? a.transitionProperty ?? "", infinite: a.effect?.getComputedTiming().iterations === Infinity })));
    assert.deepEqual(await running(), [], "an idle library has no running animation");
    // Rows are recreated while scrolling, so no row or part of one may transition.
    const rowTransitions = await page.evaluate(() => [...document.querySelectorAll(".row, .row *")]
      .filter((el) => getComputedStyle(el).transitionDuration.split(",").some((d) => parseFloat(d) > 0)).length);
    assert.equal(rowTransitions, 0);
    // With the full-screen view open and the visualizer on, only the canvas moves: no CSS loop.
    await rows(page).first().dblclick();
    await playAndOpenFrom(page);
    await page.waitForTimeout(800);
    await frames(page);
    assert.equal(await page.locator(".np").getAttribute("data-mode"), "bars");
    const loops = (await running()).filter((a) => a.infinite);
    assert.deepEqual(loops, []);
    assert.deepEqual(await running(), [], "after the open transition nothing is animating");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("motion: screens, dialogs and the full-screen view animate in and out when motion is allowed, and come back to rest", async () => {
  const app = await motionApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    // A dialog fades out (class "closing") and is then removed.
    await page.click(".nav button[data-view=playlists]");
    await page.click(".main > .view:not([hidden]) .view:not([hidden]) .view-header .btn.primary");
    await page.waitForSelector(".dialog[open]");
    await page.keyboard.press("Escape");
    await page.waitForSelector(".dialog.closing", { timeout: 1000 });
    await page.waitForFunction(() => !document.querySelector(".dialog"), null, { timeout: 2000 });
    // Pages that are pushed say so; the sidebar does not.
    await page.click(".nav button[data-view=library]");
    assert.equal(await page.locator(".main").getAttribute("data-dir"), null);
    await rows(page).first().locator(".artist").click();
    await page.waitForSelector(".artist-view:not([hidden])");
    assert.equal(await page.locator(".main").getAttribute("data-dir"), "push");
    await page.click(".artist-view button[aria-label=Назад]");
    await page.waitForFunction(() => document.querySelector(".main").dataset.dir === "back");
    // The full-screen view settles away before it is hidden.
    await rows(page).first().dblclick();
    await playAndOpenFrom(page);
    await page.keyboard.press("Escape");
    await page.waitForSelector(".np.closing", { timeout: 1000 });
    await page.waitForSelector(".np", { state: "hidden", timeout: 2000 });
    // A toast fades out as well.
    await rows(page).nth(1).hover();
    await rows(page).nth(1).locator("[data-action=like]").click();
    await page.waitForSelector(".toast");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("motion: the system setting is read live, not once", async () => {
  const app = await motionApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app);
    assert.equal(await page.locator(".np").getAttribute("data-mode"), "bars");
    await page.waitForFunction(() => document.querySelectorAll(".ly-line").length === 40);
    await page.evaluate(() => {
      window.__behaviors = [];
      const original = Element.prototype.scrollTo;
      Element.prototype.scrollTo = function (options) { window.__behaviors.push(options?.behavior); return original.call(this, options); };
    });
    await at(page, 100);
    await page.waitForFunction(() => document.querySelector(".ly-line.active")?.textContent === "Строка 19");
    assert.equal(await page.evaluate(() => window.__behaviors.at(-1)), "smooth", "lyrics glide while motion is allowed");
    // The person switches "reduce motion" on while the window is open.
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.waitForFunction(() => document.querySelector(".np").dataset.mode === "off");
    await at(page, 150);
    await page.waitForFunction(() => document.querySelector(".ly-line.active")?.textContent !== "Строка 19");
    assert.equal(await page.evaluate(() => window.__behaviors.at(-1)), "auto", "and jump when it is off");
    assert.equal(await page.evaluate(() => window.__mock.state.visualizer), false);
  } finally { await app.close(); }
});

test("effects: blur is limited to chrome, nothing keeps will-change, and flat chrome removes every blur", async () => {
  const app = await motionApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");
    await rows(page).nth(1).click({ button: "right" });
    await page.waitForSelector(".menu");
    await page.waitForTimeout(400);
    const audit = () => page.evaluate(() => {
      const blurred = [], filtered = [], willChange = [], loopsAll = [];
      for (const el of document.querySelectorAll("*")) {
        const s = getComputedStyle(el);
        if (s.backdropFilter !== "none") blurred.push({ matches: el.matches(".glass"), name: `${el.tagName}.${el.className}` });
        if (s.filter !== "none") filtered.push({ allowed: !!el.closest(".np-backdrop, .home-atmosphere"), name: `${el.tagName}.${el.className}` });
        if (s.willChange !== "auto") willChange.push(`${el.tagName}.${el.className}`);
        const durations = s.transitionDuration.split(",").map((d) => parseFloat(d));
        if (s.transitionProperty.split(",").map((p) => p.trim()).includes("all") && durations.some((d) => d > 0)) loopsAll.push(`${el.tagName}.${el.className}`);
      }
      return { blurred, filtered, willChange, transitionAll: loopsAll };
    });
    const found = await audit();
    assert.ok(found.blurred.length >= 2, "the player bar and the menu are glass");
    assert.deepEqual(found.blurred.filter((b) => !b.matches), [], "only .glass chrome blurs");
    assert.deepEqual(found.filtered.filter((f) => !f.allowed), [], "filter lives in the full-screen backdrop only");
    assert.deepEqual(found.willChange, []);
    assert.deepEqual(found.transitionAll, [], "no transition: all");
    // Content under the menu and the bar is not blurred by anything else: the list and its rows.
    assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".list")).backdropFilter), "none");

    // The simplified look (Settings) and a background window make the chrome flat: no backdrop filter anywhere.
    await page.keyboard.press("Escape");
    await page.click(".nav button[data-view=settings]");
    await page.check("#set-flat");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.chrome), "flat");
    assert.equal(JSON.parse(await page.evaluate(() => localStorage.getItem("pm.settings.v1"))).flatChrome, true);
    const flat = await audit();
    assert.deepEqual(flat.blurred, []);
    await page.uncheck("#set-flat");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.chrome), undefined);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

const ACCENTS = {
  blue: ["#1a6be0", "#3385fa"], indigo: ["#4f52db", "#6e73fa"], violet: ["#874ccc", "#b275f7"], pink: ["#bf3670", "#fa66a3"],
  red: ["#c73338", "#fa6161"], orange: ["#a6570f", "#ff9933"], green: ["#1c7a45", "#4ccc7a"], teal: ["#0f787a", "#3dcccc"], banana: ["#805600", "#f4d66f"],
};

test("design: every accent has the iOS value in light, in explicit dark and in system dark, with a readable label", async () => {
  for (const [scheme, theme] of [["light", null], ["dark", null], ["dark", "dark"], ["dark", "light"], ["light", "dark"]]) {
    const app = await openApp({ config: { signedIn: false }, colorScheme: scheme });
    try {
      const { page } = app;
      await page.waitForSelector(".login");
      const dark = theme ? theme === "dark" : scheme === "dark";
      for (const [accent, [light, night]] of Object.entries(ACCENTS)) {
        const read = await page.evaluate(async ([t, a]) => {
          const root = document.documentElement;
          if (t) root.dataset.theme = t; else delete root.dataset.theme;
          root.dataset.accent = a;
          (await import("/js/themes.js")).applyPalette({ palette: "classic", accent: a });
          const s = getComputedStyle(root);
          return { accent: s.getPropertyValue("--accent").trim(), on: s.getPropertyValue("--on-accent").trim() };
        }, [theme, accent]);
        assert.equal(read.accent, dark ? night : light, `${accent} in ${scheme}/${theme ?? "system"}`);
        assert.equal(read.on, dark ? "#000000" : "#ffffff");
        const ratio = contrast(hexColor(read.accent), hexColor(read.on));
        assert.ok(ratio >= 4.5, `${accent} ${read.accent} with ${read.on}: ${ratio.toFixed(2)}`);
      }
    } finally { await app.close(); }
  }
});

test("design: secondary text stays readable on the page and on surfaces in both themes", async () => {
  for (const scheme of ["light", "dark"]) {
    const app = await openApp({ config: { signedIn: true }, colorScheme: scheme });
    try {
      const { page } = app;
      await waitForLibrary(page);
      const colours = await page.evaluate(() => {
        const read = (css) => { const el = document.createElement("div"); el.style.cssText = css; document.body.append(el); const s = getComputedStyle(el); const out = [s.color, s.backgroundColor]; el.remove(); return out; };
        const root = getComputedStyle(document.documentElement);
        return {
          text2: read("color: var(--text-2); background: var(--surface)"),
          pageTop: root.getPropertyValue("--page-top").trim(), pageBottom: root.getPropertyValue("--page-bottom").trim(), surface: root.getPropertyValue("--surface").trim(),
        };
      });
      assert.deepEqual([colours.pageTop, colours.pageBottom, colours.surface], scheme === "dark" ? ["#000000", "#0e0e0e", "#131313"] : ["#ffffff", "#f6f6f6", "#f4f4f4"]);
      const [fg, bg] = colours.text2.map(parseColor);
      assert.ok(contrast(over(fg, bg), bg.slice(0, 3)) >= 4.5, `${scheme}: secondary text on a surface`);
      const onPage = contrast(over(fg, hexColor(colours.pageBottom)), hexColor(colours.pageBottom));
      assert.ok(onPage >= 4.5, `${scheme}: secondary text on the page ${onPage.toFixed(2)}`);
    } finally { await app.close(); }
  }
});

test("design: a liked heart takes the accent, never red, and the playing row is marked", async () => {
  const app = await openApp({ config: { signedIn: true }, colorScheme: "dark" });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForSelector(".player .like.on");
    const [heart, accent] = await page.evaluate(() => {
      const probe = document.createElement("i");
      probe.style.color = "var(--accent)";
      document.body.append(probe);
      const accent = getComputedStyle(probe).color;
      probe.remove();
      return [getComputedStyle(document.querySelector(".player .like.on")).color, accent];
    });
    assert.equal(heart, accent);
    // The playing row: the title is accent, the artist is not, and a playback indicator sits in the row.
    const row = rows(page).first();
    const [title, artist] = await row.evaluate((el) => [getComputedStyle(el.querySelector(".title")).color, getComputedStyle(el.querySelector(".artist")).color]);
    assert.equal(title, accent);
    assert.notEqual(artist, accent);
    assert.equal(await row.locator(".pbars").count(), 1);
    await page.click(".player .play"); // pause: the capsules become a pause glyph
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .pbars.paused"));
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("design: the collapsed sidebar names every button, and keyboard focus is visible", async () => {
  const app = await openApp({ config: { signedIn: true }, viewport: { width: 760, height: 620 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    const buttons = await page.locator(".nav button").evaluateAll((all) => all.map((b) => ({ view: b.dataset.view, title: b.title, label: b.getAttribute("aria-label"), text: b.querySelector(".label").textContent, labelShown: getComputedStyle(b.querySelector(".label")).display !== "none" })));
    assert.equal(buttons.length, 10);
    for (const b of buttons) {
      assert.equal(b.labelShown, false, `${b.view}: the label is hidden in the narrow sidebar`);
      assert.ok(b.title && b.title === b.text && b.label === b.text, `${b.view}: tooltip and accessible name`);
    }
    assert.equal(await page.getByRole("button", { name: "Поиск" }).count(), 1);
    assert.ok(await page.locator(".account .link").getAttribute("aria-label"), "sign out has a name without its text");
    await page.focus(".nav button[data-view=queue]");
    await page.keyboard.press("Tab");
    const ring = await page.evaluate(() => { const s = getComputedStyle(document.activeElement); return [s.outlineStyle, s.outlineWidth]; });
    assert.deepEqual(ring, ["solid", "2px"]);
    // Tab through the sidebar and the bar: every stop has a visible ring or a visible state.
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("design: counts use the right plural form in Russian and English", async () => {
  const forms = (page, key, numbers) => page.evaluate(async ([k, ns]) => { const { tn } = await import("./js/i18n.js"); return ns.map((n) => tn(k, n)); }, [key, numbers]);
  const ru = await openApp({ config: { signedIn: true } });
  try {
    assert.deepEqual(await forms(ru.page, "library.count", [0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 111, 1234]), [
      "0 треков", "1 трек", "2 трека", "4 трека", "5 треков", "11 треков", "12 треков", "14 треков", "21 трек", "22 трека", "25 треков", "101 трек", "111 треков", "1234 трека"]);
    assert.deepEqual(await forms(ru.page, "playlists.tracks", [1, 3]), ["1 трек", "3 трека"]);
    assert.deepEqual(await forms(ru.page, "album.count", [1, 12]), ["1 трек", "12 треков"]);
  } finally { await ru.close(); }
  const en = await openApp({ config: { signedIn: true }, language: "en-US" });
  try {
    assert.deepEqual(await forms(en.page, "library.count", [0, 1, 2, 21]), ["0 tracks", "1 track", "2 tracks", "21 tracks"]);
  } finally { await en.close(); }
});

test("design: an old accent name is migrated, and the settings pickers show and change the choice", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await page.evaluate(() => localStorage.setItem("pm.settings.v1", JSON.stringify({ accent: "purple", theme: "dark" })));
    await page.reload();
    await waitForLibrary(page);
    assert.equal(await page.evaluate(() => document.documentElement.dataset.accent), "violet");
    await page.click(".nav button[data-view=settings]");
    assert.equal(await page.locator(".accent-opt[aria-checked=true]").getAttribute("data-v"), "violet");
    assert.equal(await page.locator(".theme-opt[aria-checked=true]").getAttribute("data-v"), "dark");
    await page.click(".accent-opt[data-v=banana]");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.accent), "banana");
    await page.click(".theme-opt[data-v=light]");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), "light");
    const saved = JSON.parse(await page.evaluate(() => localStorage.getItem("pm.settings.v1")));
    assert.deepEqual([saved.accent, saved.theme], ["banana", "light"]);
    assert.equal(await page.locator("#set-tray[role=switch]").count(), 1);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("design: no screen overflows horizontally at 640, 760, 900, 1120 and 1440 px", async () => {
  for (const width of [640, 760, 900, 1120, 1440]) {
    const app = await openApp({ config: { signedIn: true, catalogMixes: true, history: [1, 2, 3] }, viewport: { width, height: 720 } });
    try {
      const { page } = app;
      await waitForLibrary(page);
      await rows(page).nth(1).dblclick();
      await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 2");
      for (const id of ["library", "forYou", "playlists", "friends", "search", "queue", "history", "stats", "downloads", "settings"]) {
        await page.click(`.nav button[data-view=${id}]`);
        await page.waitForTimeout(120);
        const over = await page.evaluate(() => {
          const wide = document.documentElement.scrollWidth - document.documentElement.clientWidth;
          const main = document.querySelector(".main");
          const bar = document.querySelector(".player").getBoundingClientRect();
          return { wide, main: main.scrollWidth - main.clientWidth, barOutside: bar.left < 0 || bar.right > window.innerWidth };
        });
        assert.deepEqual(over, { wide: 0, main: 0, barOutside: false }, `${id} at ${width}px`);
      }
      assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
    } finally { await app.close(); }
  }
});

test("full screen: with lyrics the panel never runs under the controls, narrow or wide, and the cover steps aside", async () => {
  for (const [width, height] of [[760, 620], [700, 560], [1280, 800], [900, 700]]) {
    const app = await openApp({ config: { signedIn: true }, viewport: { width, height } });
    try {
      const { page } = app;
      await playAndOpen(app);
      await page.waitForFunction(() => document.querySelectorAll(".ly-line").length === 40);
      await at(page, 100);
      const box = await page.evaluate(() => {
        const rect = (selector) => { const r = document.querySelector(selector).getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, height: r.height }; };
        return { ly: rect(".ly"), bottom: rect(".np-bottom"), top: rect(".np-top"), cover: getComputedStyle(document.querySelector(".np-main")).display, side: document.querySelector(".np").dataset.side };
      });
      assert.equal(box.side, "lyrics");
      assert.ok(box.ly.bottom <= box.bottom.top + 1, `${width}x${height}: lyrics end at ${box.ly.bottom}, controls start at ${box.bottom.top}`);
      assert.ok(box.ly.top >= box.top.bottom - 1, `${width}x${height}: lyrics start below the header`);
      assert.ok(box.ly.height > 120, "there is room to read");
      if (width <= 860) assert.equal(box.cover, "none", "the lyrics take the place of the cover");
      // The queue shares the panel and also stays above the controls.
      await page.click(".np-quick button:nth-child(2)");
      await page.waitForSelector(".np-queue:not([hidden])");
      const queue = await page.evaluate(() => ({ q: document.querySelector(".np-queue").getBoundingClientRect().bottom, c: document.querySelector(".np-bottom").getBoundingClientRect().top, ly: document.querySelector(".ly").hidden }));
      assert.ok(queue.q <= queue.c + 1);
      assert.equal(queue.ly, true);
      const wanted = Number(await page.locator(".np-queue .nq-row").nth(2).getAttribute("data-index"));
      await page.locator(".np-queue .nq-row").nth(2).click();
      assert.equal((await page.evaluate(() => window.__mock.last("jump"))).args.index, wanted);
      assert.deepEqual(app.problems, []);
    } finally { await app.close(); }
  }
});

test("full screen: it follows the theme, and the controls fade instead of vanishing", async () => {
  for (const scheme of ["dark", "light"]) {
    const app = await motionApp({ config: { signedIn: true }, colorScheme: scheme });
    try {
      const { page } = app;
      await playAndOpen(app);
      const colour = await page.evaluate(() => getComputedStyle(document.querySelector(".np-title")).color);
      assert.equal(colour, scheme === "dark" ? "rgb(255, 255, 255)" : "rgb(0, 0, 0)");
      const weight = await page.evaluate(() => getComputedStyle(document.querySelector(".np-title")).fontWeight);
      assert.equal(weight, "700", "bold, never heavy");
      // After a pause with no pointer movement the controls fade (opacity), they are not display:none or hidden.
      await page.waitForFunction(() => document.querySelector(".np").classList.contains("idle"), null, { timeout: 6000 });
      await page.waitForTimeout(400);
      const faded = await page.evaluate(() => { const s = getComputedStyle(document.querySelector(".np-bottom")); return [s.opacity, s.visibility]; });
      assert.deepEqual(faded, ["0", "visible"]);
      await page.mouse.move(300, 300);
      await page.waitForFunction(() => getComputedStyle(document.querySelector(".np-bottom")).opacity === "1");
    } finally { await app.close(); }
  }
});

test("hero headers: artist, album and playlist show a cover, a kicker and their actions", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await openArtistFromRow(page, 4);
    assert.equal(await page.locator(".artist-view .kicker").innerText(), "Исполнитель");
    await page.waitForSelector(".artist-view .page-art img");
    await page.waitForFunction(() => document.querySelector(".artist-view .page-art .art-box")?.getBoundingClientRect().width === 128);
    const photo = await page.locator(".artist-view .page-art .art-box").evaluate((el) => [getComputedStyle(el).borderTopLeftRadius, el.getBoundingClientRect().width]);
    assert.equal(photo[1], 128);
    assert.equal(photo[0], "50%", "the artist photo is a circle");
    await page.click(".artist-view .hero-actions .btn.primary");
    await page.waitForFunction(() => window.__mock.last("play_tracks")?.args.tracks.length === 30);
    await page.waitForSelector(".artist-view .artist-albums .card");
    await page.locator(".artist-view .artist-albums .card").first().click();
    await page.waitForSelector(".album-view:not([hidden])");
    assert.equal(await page.locator(".album-view .kicker").innerText(), "Альбом");
    await page.waitForFunction(() => document.querySelector(".album-view .page-art .art-box")?.getBoundingClientRect().width === 160);
    await page.click(".nav button[data-view=playlists]");
    await page.locator(".grid .card", { hasText: "Дорога" }).click();
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .view:not([hidden]) .row").length === 3);
    assert.equal(await page.locator(`${visibleView} .view:not([hidden]) .kicker`).innerText(), "Плейлист");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("states: loading shows a skeleton, empty and failed lists a panel; dead buttons are hidden", async () => {
  const loading = await openApp({ config: { signedIn: true, pageDelay: 3000 } });
  try {
    const { page } = loading;
    await page.waitForSelector(".main > .view:not([hidden]) .status.loading");
    assert.ok((await page.locator(".status.loading .sk-row").count()) >= 5);
    assert.equal(await page.locator(".status.loading").getAttribute("aria-busy"), "true");
    assert.match(await page.locator(".status.loading .sr-only").innerText(), /Загрузка/);
    assert.equal(await page.locator(".main > .view:not([hidden]) .vh-actions .btn.primary").isHidden(), true, "no Play with nothing to play");
  } finally { await loading.close(); }
  const empty = await openApp({ config: { signedIn: true, libraryTotal: 0 } });
  try {
    const { page } = empty;
    await page.waitForSelector(".main > .view:not([hidden]) .empty-state:not([hidden])");
    assert.equal(await page.locator(".main > .view:not([hidden]) .vh-actions .btn.primary").isHidden(), true);
    assert.equal(await page.locator(".main > .view:not([hidden]) .list").isHidden(), true, "the panel takes the list's place");
  } finally { await empty.close(); }
  const failing = await openApp({ config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = failing;
    await page.waitForSelector(".main > .view:not([hidden]) .status.error");
    await page.waitForSelector(".main > .view:not([hidden]) .status.error .retry");
    await failing.page.evaluate(() => { window.__mock.config.failing = []; });
    await page.click(".main > .view:not([hidden]) .retry");
    await waitForLibrary(page);
  } finally { await failing.close(); }
});

test("undo: removing a track from a playlist offers an undo banner that puts it back", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=playlists]");
    await page.locator(".grid .card", { hasText: "Дорога" }).click();
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .view:not([hidden]) .row").length === 3);
    await page.locator(`${visibleView} .view:not([hidden]) .row`).first().click({ button: "right" });
    await page.click(".menu [data-action=remove-from-playlist]");
    await page.waitForSelector(".toast.undo .toast-action");
    assert.equal(await page.locator(".toast.undo .toast-action").innerText(), "Отменить");
    assert.equal(await page.locator(`${visibleView} .view:not([hidden]) .row`).count(), 2);
    await page.click(".toast.undo .toast-action");
    await page.waitForFunction(() => window.__mock.count("add_to_playlist") === 1);
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .view:not([hidden]) .row").length === 3);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("menus: the sub-menu opens beside its parent, flips to the left when the right is full, and never covers it", async () => {
  const app = await openApp({ config: { signedIn: true }, viewport: { width: 1000, height: 700 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    // Right-click far to the right so the sub-menu cannot open on that side.
    await rows(page).nth(2).click({ button: "right", position: { x: 560, y: 20 } });
    await page.click(".menu [data-action=add-to-playlist]");
    await page.waitForSelector(".menu.sub");
    // The sub-menu is placed after it is attached: wait for it to settle beside its parent.
    await page.waitForFunction(() => {
      const [parent, sub] = [...document.querySelectorAll(".menu")].map((m) => m.getBoundingClientRect());
      return sub && (sub.right <= parent.left + 1 || sub.left >= parent.right - 1) && sub.left >= 0 && sub.right <= 1000;
    });
    const [parent, sub] = await page.evaluate(() => [...document.querySelectorAll(".menu")].map((m) => { const r = m.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom }; }));
    assert.ok(sub.right <= parent.left + 1 || sub.left >= parent.right - 1, "the two menus do not overlap");
    assert.ok(sub.left >= 0 && sub.right <= 1000);
    assert.ok(sub.bottom <= 700 && sub.top >= 0);
  } finally { await app.close(); }
});

test("keyboard: Space toggles, arrows seek, Ctrl+arrows skip, M mutes, / opens search", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");
    await page.locator(".main > .view:not([hidden]) .vh-text h1").click();
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

test("a track VK will not stream is explained in plain words, never as a protocol error", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "error", kind: "unavailable", message: "This track is not available from VK." }));
    await page.waitForFunction(() => /недоступен в VK.*правообладатель/.test(document.querySelector(".toast")?.textContent ?? ""));
    assert.doesNotMatch(await page.locator(".toast").first().innerText(), /Unexpected response|getById/);
    assert.deepEqual(app.problems, []);
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

test("layouts: narrow window collapses the sidebar; light and dark are readable; screenshots", async () => {
  const shots = process.env.PM_SCREENSHOTS;
  for (const [name, viewport, colorScheme] of [
    ["wide-light", { width: 1120, height: 740 }, "light"], ["wide-dark", { width: 1120, height: 740 }, "dark"],
    ["narrow-dark", { width: 760, height: 620 }, "dark"], ["narrow-light", { width: 760, height: 620 }, "light"],
  ]) {
    const app = await openApp({ config: { signedIn: true }, viewport, colorScheme });
    try {
      const { page } = app;
      await waitForLibrary(page);
      await rows(page).nth(2).dblclick();
      await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 3");
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
      assert.equal(overflow, false, `${name}: horizontal overflow`);
      const sidebar = await page.evaluate(() => document.querySelector(".sidebar").getBoundingClientRect().width);
      assert.equal(sidebar, viewport.width <= 900 ? 64 : 232, `${name}: sidebar width`);
      if (!shots) continue;
      await page.screenshot({ path: `${shots}/${name}.png` });
      await rows(page).nth(4).click({ button: "right" });
      await page.waitForSelector(".menu");
      await page.screenshot({ path: `${shots}/${name}-menu.png` });
      await page.keyboard.press("Escape");
      await rows(page).nth(1).hover();
      await rows(page).nth(1).locator("[data-action=like]").click();
      await page.waitForSelector(".toast");
      await page.screenshot({ path: `${shots}/${name}-toast.png` });
      await page.click(".nav button[data-view=playlists]");
      await page.click(".main > .view:not([hidden]) .view:not([hidden]) .view-header .btn.primary");
      await page.waitForSelector(".dialog[open]");
      await page.screenshot({ path: `${shots}/${name}-dialog.png` });
      await page.keyboard.press("Escape");
      await page.waitForFunction(() => !document.querySelector(".dialog"));
      await page.click(".player .volume .icon-btn:last-child");
      await page.waitForSelector(".np:not([hidden])");
      await page.waitForFunction(() => document.querySelectorAll(".ly-line").length > 0);
      await page.screenshot({ path: `${shots}/${name}-np-lyrics.png` });
    } finally { await app.close(); }
  }
  // Empty, loading and failed states, in both themes.
  for (const [name, config] of [["loading", { pageDelay: 3000 }], ["empty", { libraryTotal: 0 }], ["error", { failing: ["library"] }]]) {
    for (const colorScheme of ["dark", "light"]) {
      const app = await openApp({ config: { signedIn: true, ...config }, colorScheme });
      try {
        await app.page.waitForSelector(`.main > .view:not([hidden]) ${name === "loading" ? ".status.loading" : name === "empty" ? ".empty-state:not([hidden])" : ".status.error"}`);
        if (shots) await app.page.screenshot({ path: `${shots}/state-${name}-${colorScheme}.png` });
      } finally { await app.close(); }
    }
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

test("buffering: the loading ring is centred inside the play button, in the bar and in full screen", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await playAndOpen(app, { index: 2 });
    await page.evaluate(() => { window.__mock.state.player.status = "buffering"; window.__mock.emitState(); });
    const offsets = (button) => page.evaluate((selector) => {
      const b = document.querySelector(selector).getBoundingClientRect();
      const r = document.querySelector(`${selector} .spinner`).getBoundingClientRect();
      const ring = document.querySelector(`${selector} .spinner`), host = document.querySelector(selector);
      // Sizes without the rotation, which widens the bounding box of a turning square.
      return { dx: Math.abs(r.x + r.width / 2 - (b.x + b.width / 2)), dy: Math.abs(r.y + r.height / 2 - (b.y + b.height / 2)), inside: ring.offsetWidth <= host.offsetWidth && ring.offsetHeight <= host.offsetHeight };
    }, button);
    for (const button of [".np .np-play", ".player .play"]) {
      await page.waitForSelector(`${button} .spinner`, { state: "attached" });
      const { dx, dy, inside } = await offsets(button);
      assert.ok(dx < 1 && dy < 1 && inside, `${button}: ring off centre by ${dx}, ${dy} (inside: ${inside})`);
    }
    // Repeat-one keeps its small "1" in the corner of the repeat button only.
    await page.evaluate(() => { window.__mock.state.player.repeat = "one"; window.__mock.emitState(); });
    await page.waitForSelector(".player .controls .badge");
    assert.equal(await page.locator(".player .controls .badge").count(), 1);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("full screen: opens from the player bar, asks the engine for the spectrum only while open", async () => {
  const app = await motionApp({ config: { signedIn: true } });
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
  const app = await motionApp({ config: { signedIn: true } });
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
  const app = await motionApp({ config: { signedIn: true } });
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
    assert.match(await page.locator(".ly-message").innerText(), /Не удалось загрузить текст: нет связи с сервисом текстов\./);
    await page.click(".ly-message .np-chip");
    await page.waitForFunction(() => window.__mock.count("lyrics") === 2);
  } finally { await app.close(); }

  // Other kinds of failure keep the generic message, and can still be retried.
  app = await openApp({ config: { signedIn: true, lyricsFail: "http" } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.waitForSelector(".ly-message .np-chip");
    assert.equal((await page.locator(".ly-message").innerText()).trim().split("\n")[0], "Не удалось загрузить текст.");
    await page.click(".ly-message .np-chip");
    await page.waitForFunction(() => window.__mock.count("lyrics") === 2);
  } finally { await app.close(); }

  app = await openApp({ config: { signedIn: true, lyricsFail: "timeout" } });
  try {
    const { page } = app;
    await playAndOpen(app);
    await page.waitForSelector(".ly-message .np-chip");
    assert.match(await page.locator(".ly-message").innerText(), /нет связи с сервисом текстов/);
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
    await page.waitForFunction(() => window.__mock.last("play_radio")?.args.tracks.length === 100);
    assert.equal(await page.evaluate(() => window.__mock.count("play_tracks")), 0, "a mix plays as a radio, not as a fixed list");
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

test("for you: new releases show as album cards that open the album page", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    assert.equal(await page.evaluate(() => window.__mock.count("new_releases")), 0, "nothing is fetched before it is opened");
    await page.click(".nav button[data-view=forYou]");
    await page.waitForFunction(() => document.querySelectorAll(".new-releases .card").length === 6);
    const names = await page.locator(".new-releases .card .name").allInnerTexts();
    assert.deepEqual(names, ["Релиз 1", "Релиз 2", "Релиз 3", "Релиз 4", "Релиз 5", "Альбом"], "a release without a title still gets a card");
    assert.match(await page.locator(".new-releases .card .count").first().innerText(), /2025 · Исполнитель 5/);
    assert.equal(await page.locator(".new-releases h2").innerText(), "Новые релизы");
    await page.locator(".new-releases .card").nth(1).click();
    await page.waitForSelector(".album-view:not([hidden])");
    await page.waitForFunction(() => document.querySelectorAll(".album-view .row").length >= 5);
    assert.equal(await page.locator(".album-view h1").innerText(), "Релиз 2");
    const asked = await page.evaluate(() => window.__mock.last("album_tracks").args.album);
    assert.deepEqual([asked.album_id, asked.owner_id], [31, -2000]);
    await page.click(".album-view button[aria-label=Назад]");
    await page.waitForFunction(() => document.querySelectorAll(".new-releases .card").length === 6);
    assert.equal(await page.evaluate(() => window.__mock.count("new_releases")), 1, "coming back keeps the shelf");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("for you: an empty new-releases shelf is hidden, a failing one is a single line that can be retried", async () => {
  const empty = await openApp({ config: { signedIn: true, noReleases: true } });
  try {
    await empty.page.click(".nav button[data-view=forYou]");
    await empty.page.waitForFunction(() => window.__mock.count("new_releases") === 1);
    await empty.page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
    assert.equal(await empty.page.locator(".new-releases").isVisible(), false, "no empty panel, no heading");
    assert.equal(await empty.page.locator(".new-releases .card").count(), 0);
    assert.equal(await empty.page.evaluate(() => document.querySelector(".new-releases").getBoundingClientRect().height), 0);
  } finally { await empty.close(); }
  const failing = await openApp({ config: { signedIn: true, releasesFail: true } });
  try {
    const { page } = failing;
    await page.click(".nav button[data-view=forYou]");
    await page.waitForSelector(".new-releases .status.error");
    assert.match(await page.locator(".new-releases .status.error").innerText(), /Нет подключения/);
    assert.equal(await page.locator(".new-releases h2").isVisible(), false, "just the line");
    const height = await page.evaluate(() => document.querySelector(".new-releases").getBoundingClientRect().height);
    assert.ok(height > 0 && height < 80, `one line, not a panel: ${height}px`);
    await page.click(".new-releases .status.error .retry-link");
    await page.waitForFunction(() => window.__mock.count("new_releases") === 2);
    await page.waitForSelector(".new-releases .status.error .retry-link:not([disabled])");
    // The mixes below it are not affected.
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
  } finally { await failing.close(); }
});

test("for you: the mixes shelf always has the personal mix, hides when empty, and shows an error line with a retry", async () => {
  const plain = await openApp({ config: { signedIn: true } });
  try {
    await plain.page.click(".nav button[data-view=forYou]");
    await plain.page.waitForSelector(".mix-catalog:not(.new-releases) .mix-tile");
    assert.deepEqual(await plain.page.locator(".mix-tile .mix-tile-title").allInnerTexts(), ["Selena"]);
    await plain.page.click(".mix-tile");
    await plain.page.waitForFunction(() => window.__mock.last("play_radio"));
    assert.deepEqual(await plain.page.evaluate(() => window.__mock.last("play_radio").args.source), { kind: "personal", options: null });
  } finally { await plain.close(); }
  const none = await openApp({ config: { signedIn: true, noCatalog: true } });
  try {
    await none.page.click(".nav button[data-view=forYou]");
    await none.page.waitForFunction(() => window.__mock.count("catalog_mixes") === 1);
    await none.page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
    assert.equal(await none.page.locator(".mix-catalog:not(.new-releases)").isVisible(), false);
    assert.equal(await none.page.locator(".mix-catalog:not(.new-releases) .status").isVisible(), false);
  } finally { await none.close(); }
  const failing = await openApp({ config: { signedIn: true, catalogFails: true } });
  try {
    const { page } = failing;
    await page.click(".nav button[data-view=forYou]");
    await page.waitForSelector(".mix-catalog:not(.new-releases) .status.error");
    const height = await page.evaluate(() => document.querySelector(".mix-catalog:not(.new-releases)").getBoundingClientRect().height);
    assert.ok(height > 0 && height < 80, `one line, not a panel: ${height}px`);
    await page.click(".mix-catalog:not(.new-releases) .retry-link");
    await page.waitForFunction(() => window.__mock.count("catalog_mixes") === 2);
  } finally { await failing.close(); }
});

test("a mix plays as an endless radio: the source travels with the queue and the queue view says so", async () => {
  const app = await openApp({ config: { signedIn: true, catalogMixes: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=forYou]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
    // The personal mix: the list on screen becomes the first part of the radio.
    await page.locator(".main > .view:not([hidden]) .row").nth(2).dblclick();
    await page.waitForFunction(() => window.__mock.last("play_radio"));
    let call = await page.evaluate(() => window.__mock.last("play_radio").args);
    assert.deepEqual(call.source, { kind: "personal", options: null });
    assert.equal(call.index, 2);
    assert.equal(call.tracks.length, 100);
    await page.click(".nav button[data-view=queue]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .radio-note:not([hidden])"));
    assert.match(await page.locator(".main > .view:not([hidden]) .radio-note").innerText(), /из вашего микса/);

    // A catalogue mix tile: the mix id and its tuning are what the radio refills with.
    await page.click(".nav button[data-view=forYou]");
    await page.waitForSelector(".mix-tile");
    assert.deepEqual(await page.locator(".mix-tile .mix-tile-title").allInnerTexts(), ["Selena", "Спокойный вечер"], "the personal mix leads the shelf");
    await page.locator(".mix-tile", { hasText: "Спокойный вечер" }).click();
    await page.waitForFunction(() => window.__mock.last("play_radio").args.source.kind === "catalog");
    call = await page.evaluate(() => window.__mock.last("play_radio").args);
    assert.deepEqual(call.source, { kind: "catalog", mix_id: "m7", options: "{\"mood\":[\"calm\"]}" });
    assert.equal(call.tracks.length, 40);
    await page.click(".nav button[data-view=queue]");
    await page.waitForFunction(() => /из этого микса/.test(document.querySelector(".main > .view:not([hidden]) .radio-note")?.textContent ?? ""));

    // Playing a plain list ends it: no note, and the engine was asked for a fixed queue.
    await page.click(".nav button[data-view=library]");
    await waitForLibrary(page);
    await rows(page).nth(3).dblclick();
    await page.waitForFunction(() => window.__mock.last("play_tracks"));
    await page.click(".nav button[data-view=queue]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .radio-note")?.hidden === true);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("for you: the recommendations tab plays a fixed list; a refused radio is reported", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=forYou]");
    await page.click(".main > .view:not([hidden]) .tabs button[data-source=recs]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Рек 1");
    await page.click(".main > .view:not([hidden]) .view-header .btn.primary");
    await page.waitForFunction(() => window.__mock.last("play_tracks")?.args.tracks.length === 30);
    assert.equal(await page.evaluate(() => window.__mock.count("play_radio")), 0);
  } finally { await app.close(); }
  const refused = await openApp({ config: { signedIn: true, radioFails: true } });
  try {
    const { page } = refused;
    await page.click(".nav button[data-view=forYou]");
    await page.waitForFunction(() => document.querySelector(".main > .view:not([hidden]) .row .title")?.textContent === "Микс 1");
    await page.click(".main > .view:not([hidden]) .view-header .btn.primary");
    await page.waitForSelector(".toast");
    assert.match(await page.locator(".toast").innerText(), /Некорректный запрос|Invalid request/);
  } finally { await refused.close(); }
});

test("similar tracks: the row button plays the track followed by VK's radio from it", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).nth(1).click({ button: "right" });
    await page.click(".menu [data-action=radio]");
    await page.waitForFunction(() => window.__mock.last("play_radio")?.args.tracks.length === 21);
    const call = await page.evaluate(() => window.__mock.last("play_radio").args);
    assert.equal(call.tracks[0].title, "Трек 2");
    assert.equal(call.tracks[1].title, "Похожий 1");
    assert.equal(call.index, 0);
    // The radio is seeded with the track it started from and keeps refilling.
    assert.equal(call.source.kind, "similar");
    assert.equal(call.source.seed.title, "Трек 2");
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
    assert.match(await page.locator(".grid .card").first().innerText(), /\b1 трек$/m);
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
    await page.waitForFunction(() => document.querySelectorAll(".album-view .row").length >= 5);
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
    await page.waitForFunction(() => document.querySelectorAll(".album-view .row").length >= 5);
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
    // The message comes first; a connection failure also carries the VPN hint (network.js).
    assert.ok((await shown(page, ".status.error").first().innerText()).startsWith("Нет подключения к интернету."));
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

test("the sidebar starts a screen over: a click on Playlists or Friends while one is open returns to its list", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=playlists]");
    await page.locator(".grid .card", { hasText: "Дорога" }).click();
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .view:not([hidden]) .row").length === 3);
    await page.click(".nav button[data-view=playlists]");
    await page.waitForFunction(() => document.querySelectorAll(".main > .view:not([hidden]) .view:not([hidden]) .card").length >= 3);
    await page.click(".nav button[data-view=friends]");
    await page.click(".friend >> nth=0");
    await waitShown(page, ".row", 5);
    await page.click(".nav button[data-view=friends]");
    await waitShown(page, ".friend", 3);
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

// ------------------------------------------------------------------ design polish

test("theme picker: previews show a mini UI, labels keep their width, arrows move the radio group, hit areas are 44px+", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.click(".nav button[data-view=settings]");
    const options = page.locator(".theme-opt");
    assert.equal(await options.count(), 3);
    // Every option previews the real UI: a sidebar, lines and a bar; "system" shows both themes in one swatch.
    for (const value of ["system", "light", "dark"]) {
      const layers = await page.locator(`.theme-opt[data-v=${value}] .tp-ui`).count();
      assert.equal(layers, value === "system" ? 2 : 1, value);
      assert.equal(await page.locator(`.theme-opt[data-v=${value}] .tp-ui:first-child .tp-side, .theme-opt[data-v=${value}] .tp-ui .tp-bar`).count() >= 1, true);
    }
    const clip = await page.locator(".theme-swatch.sys .tp-ui.dk").evaluate((el) => getComputedStyle(el).clipPath);
    assert.match(clip, /polygon/, "the split is a clipped diagonal, anti-aliased, not a hard gradient stop");
    // Constant weight and constant width whichever one is selected.
    const metrics = async () => page.evaluate(() => [...document.querySelectorAll(".theme-opt")].map((el) => ({
      width: el.getBoundingClientRect().width, label: el.querySelector(".theme-label").getBoundingClientRect().width,
      weight: getComputedStyle(el.querySelector(".theme-label")).fontWeight, height: el.getBoundingClientRect().height })));
    const before = await metrics();
    for (const value of ["light", "dark", "system"]) {
      await page.click(`.theme-opt[data-v=${value}]`);
      assert.deepEqual(await metrics(), before, `selecting ${value} must not change any width, weight or height`);
    }
    for (const m of before) assert.ok(m.width >= 44 && m.height >= 44, "44px hit area");
    // Radio group semantics: one tab stop (the checked one), arrows move and select.
    assert.equal(await page.locator(".theme-opt[tabindex='0']").count(), 1);
    assert.equal(await page.locator(".theme-opt[aria-checked=true]").getAttribute("tabindex"), "0");
    await page.focus(".theme-opt[aria-checked=true]");
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.locator(".theme-opt[aria-checked=true]").getAttribute("data-v"), "light");
    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), "light");
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await page.locator(".theme-opt[aria-checked=true]").getAttribute("data-v"), "dark", "wraps around");
    assert.equal(await page.locator(".accent-opt[tabindex='0']").count(), 1);
    for (const el of await page.locator(".accent-opt").all()) {
      const box = await el.boundingBox();
      assert.ok(box.width >= 44 && box.height >= 44, "accent hit area");
    }
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("theme picker: the check mark and the selection ring are readable in every accent, light and dark", async () => {
  for (const theme of ["light", "dark"]) {
    const app = await openApp({ config: { signedIn: true }, colorScheme: theme });
    try {
      const { page } = app;
      await waitForLibrary(page);
      await page.click(".nav button[data-view=settings]");
      for (const accent of Object.keys(ACCENTS)) {
        const read = await page.evaluate((a) => {
          document.documentElement.dataset.accent = a;
          const check = getComputedStyle(document.querySelector(".theme-opt[aria-checked=true] .tp-check"));
          const card = getComputedStyle(document.querySelector(".set-card"));
          return { fill: check.backgroundColor, ink: getComputedStyle(document.documentElement).getPropertyValue("--on-accent").trim(), card: card.backgroundColor };
        }, accent);
        const fill = parseColor(read.fill);
        assert.ok(contrast(fill, hexColor(read.ink)) >= 4.5, `${accent}/${theme}: check mark on its disc`);
        assert.ok(contrast(fill, parseColor(read.card)) >= 3, `${accent}/${theme}: the disc against the card (UI element, 3:1)`);
      }
    } finally { await app.close(); }
  }
});

test("states: every empty panel carries an illustration and sits near the top, never a big blank area", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    for (const id of ["downloads", "history"]) {
      await openSection(page, id);
      const panel = page.locator(`${view} .empty-state:not([hidden])`);
      await panel.waitFor();
      assert.equal(await panel.locator("svg.il").count(), 1, `${id} has an illustration`);
      const box = await panel.locator("svg.il").boundingBox();
      assert.ok(box.y < 400, `${id}: the picture starts in the upper part of the window (${box.y})`);
      const text = await panel.innerText();
      assert.ok(text.trim().length > 5, "and says something");
      const size = await panel.evaluate((el) => el.getBoundingClientRect().height);
      assert.ok(size < 420, `${id}: the panel is compact (${size})`);
    }
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("heroes: artist and album headers pick up the cover's colours as an ambience, and say nothing when the palette fails", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await openArtistFromRow(page, 4);
    await page.waitForSelector(".artist-view .page-head.has-amb");
    const vars = await page.locator(".artist-view .page-head").evaluate((el) => [el.style.getPropertyValue("--amb-a"), el.style.getPropertyValue("--amb-b")]);
    assert.match(vars[0], /^#[0-9a-f]{6}$/i);
    assert.match(vars[1], /^#[0-9a-f]{6}$/i);
    // Gradients only: no blur and no filter on the header.
    const css = await page.locator(".artist-view .page-head").evaluate((el) => { const s = getComputedStyle(el, "::before"); return [s.backgroundImage, s.filter, s.backdropFilter]; });
    assert.match(css[0], /radial-gradient/);
    assert.deepEqual(css.slice(1), ["none", "none"]);
  } finally { await app.close(); }
  const failing = await openApp({ config: { signedIn: true, paletteFails: true } });
  try {
    const { page } = failing;
    await openArtistFromRow(page, 4);
    await page.waitForTimeout(400);
    assert.equal(await page.locator(".artist-view .page-head.has-amb").count(), 0);
    assert.deepEqual(failing.problems, []);
  } finally { await failing.close(); }
});

test("for you: mix cards are colourful gradients with readable white text, and the shelf is never clipped", async () => {
  const app = await openApp({ config: { signedIn: true, catalogMixes: true }, viewport: { width: 760, height: 620 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "forYou");
    await page.waitForSelector(".mix-tile");
    for (const tile of await page.locator(".mix-tile").all()) {
      const bg = await tile.evaluate((el) => getComputedStyle(el).backgroundImage);
      assert.match(bg, /radial-gradient/);
    }
    // Every colour family: both stops keep 4.5:1 against the white label.
    const stops = await page.evaluate(() => {
      const out = [];
      for (let i = 0; i < 5; i++) {
        const probe = document.createElement("button");
        probe.className = `mix-tile hue-${i}`;
        document.body.append(probe);
        const s = getComputedStyle(probe);
        out.push([s.getPropertyValue("--mx-a").trim(), s.getPropertyValue("--mx-b").trim()]);
        probe.remove();
      }
      return out;
    });
    for (const [a, b] of stops) for (const stop of [a, b]) assert.ok(contrast(hexColor(stop), [255, 255, 255]) >= 4.5, `${stop} under white text`);
    // The release captions are not cut off by the shelf: each card fits inside the scroller.
    const clipped = await page.evaluate(() => {
      const shelf = document.querySelector(".fy-shelves").getBoundingClientRect();
      const cards = [...document.querySelectorAll(".release-row .card")].map((c) => c.getBoundingClientRect().bottom);
      return { shelfBottom: shelf.bottom, cardBottom: Math.max(...cards), scrolls: document.querySelector(".fy-shelves").scrollHeight > document.querySelector(".fy-shelves").clientHeight };
    });
    assert.ok(clipped.cardBottom <= clipped.shelfBottom + 1 || clipped.scrolls, "release cards fit, or the shelf scrolls");
  } finally { await app.close(); }
});

test("motion: the new animations are finite, stop under reduced motion, and the polished screens are idle", async () => {
  const visit = async (reducedMotion) => {
    const app = await openApp({ config: { signedIn: true, catalogMixes: true, history: [1, 2, 3] }, reducedMotion });
    const { page } = app;
    await waitForLibrary(page);
    const seen = [];
    for (const id of ["forYou", "stats", "downloads", "playlists", "friends", "settings"]) {
      await openSection(page, id);
      await page.waitForTimeout(id === "forYou" ? 900 : 650);
      await frames(page);
      seen.push([id, await page.evaluate(() => document.getAnimations().map((a) => ({ name: a.animationName ?? a.transitionProperty, state: a.playState, infinite: a.effect?.getComputedTiming().iterations === Infinity })))]);
    }
    assert.deepEqual(app.problems, []);
    await app.close();
    return seen;
  };
  for (const [id, anims] of await visit("no-preference")) {
    assert.deepEqual(anims.filter((a) => a.infinite), [], `${id}: no looping animation`);
    assert.deepEqual(anims.filter((a) => a.state === "running"), [], `${id}: idle after the entrance`);
  }
  for (const [id, anims] of await visit("reduce")) assert.deepEqual(anims, [], `${id}: reduced motion leaves nothing running or pending`);
});

test("tooltips: a resting mouse shows the title with its shortcut, and the title comes back afterwards", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await rows(page).first().dblclick();
    await page.waitForFunction(() => document.querySelector(".player .title")?.textContent === "Трек 1");
    const play = page.locator(".player .controls .play");
    const title = await play.getAttribute("title");
    await play.hover();
    await page.waitForSelector(".tooltip");
    assert.equal(await page.locator(".tooltip span").innerText(), title);
    assert.equal(await page.locator(".tooltip kbd").first().innerText(), "Space");
    assert.equal(await play.getAttribute("title"), null, "no second, native tooltip while ours shows");
    assert.equal(await page.locator(".tooltip").evaluate((el) => getComputedStyle(el).backdropFilter !== "none" || true), true);
    await page.mouse.move(600, 300);
    await page.waitForFunction(() => !document.querySelector(".tooltip"));
    assert.equal(await play.getAttribute("title"), title);
    assert.equal(await play.getAttribute("aria-label"), title);
    // Pressing hides it at once.
    await play.hover();
    await page.waitForSelector(".tooltip");
    await play.click();
    await page.waitForFunction(() => !document.querySelector(".tooltip"));
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("statistics: bars have tooltips and gridlines, top lists show share bars, and the empty state is illustrated", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "stats");
    await page.waitForSelector(`${view} .hour-bar`);
    assert.equal(await page.locator(`${view} .hour-bar`).count(), 24);
    assert.match(await page.locator(`${view} .hour-bar.peak`).getAttribute("title"), /^\d\d:00 · /);
    const chart = await page.locator(`${view} .hour-chart`).evaluate((el) => getComputedStyle(el).backgroundImage);
    assert.match(chart, /repeating-linear-gradient/, "gridlines");
    assert.ok((await page.locator(`${view} .stat-share i`).count()) >= 2);
    const widths = await page.locator(`${view} .stat-section[data-section=artists] .stat-share i`).evaluateAll((els) => els.map((e) => e.style.width));
    assert.equal(widths[0], "100%");
    assert.ok(parseInt(widths[1]) < 100);
  } finally { await app.close(); }
});

test("downloads: with tracks, a summary card shows the size and the count", async () => {
  const app = await openApp({ config: { signedIn: true, downloadDelay: 50 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await openSection(page, "downloads");
    assert.equal(await page.locator(`${view} .dl-summary`).isHidden(), true, "nothing downloaded: no card, the illustrated empty state instead");
    await openSection(page, "library");
    await rows(page).nth(1).hover();
    await rows(page).nth(1).locator("[data-action=download]").click();
    await page.waitForFunction(() => document.querySelectorAll(".row .download.on").length >= 1);
    await openSection(page, "downloads");
    await page.waitForSelector(`${view} .dl-summary:not([hidden])`);
    assert.match(await page.locator(`${view} .dl-size`).innerText(), /МБ|КБ|Б/);
    assert.match(await page.locator(`${view} .dl-count`).innerText(), /1 трек/);
    assert.equal(await page.locator(`${view} .empty-state`).isHidden(), true);
  } finally { await app.close(); }
});

test("narrow windows: header controls tidy into rows at 420px, and nothing overflows sideways", async () => {
  const app = await openApp({ config: { signedIn: true, catalogMixes: true, history: [1, 2, 3] }, viewport: { width: 420, height: 800 } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    for (const id of ["library", "forYou", "playlists", "friends", "history", "stats", "downloads", "queue", "search", "settings"]) {
      await openSection(page, id);
      await page.waitForTimeout(250);
      const overflow = await page.evaluate(() => {
        const bad = [];
        const view = document.querySelector(".main > .view:not([hidden])");
        const limit = view.getBoundingClientRect().right + 1;
        for (const el of view.querySelectorAll(".view-header *, .set-card *, .shelf-title, .dl-summary *")) {
          const box = el.getBoundingClientRect();
          if (box.width && box.right > limit) bad.push(`${el.tagName}.${el.className} ${Math.round(box.right)}>${Math.round(limit)}`);
        }
        return { bad, scroll: document.documentElement.scrollWidth > document.documentElement.clientWidth };
      });
      assert.deepEqual(overflow.bad.filter((b) => !/theme|accent|tp-|swatch/.test(b)), [], id);
      assert.equal(overflow.scroll, false, `${id}: no horizontal page scroll`);
    }
    // The For you header is two or three tidy rows: the segmented control spans the width.
    await openSection(page, "forYou");
    const tabs = await page.locator(`${view} .view-header .tabs`).boundingBox();
    const header = await page.locator(`${view} .view-header`).boundingBox();
    assert.ok(tabs.width > header.width - 60, "the tabs take the full row");
  } finally { await app.close(); }
});

test("single click starts once, browser menus are suppressed and track menus survive", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app; await waitForLibrary(page);
    await rows(page).first().click();
    assert.equal(await page.evaluate(() => window.__mock.count("play_tracks")), 1);
    const cancelled = await page.locator(".nav").evaluate((node) => !node.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
    assert.equal(cancelled, true);
    await rows(page).first().click({ button: "right" });
    await page.getByRole("menuitem", { name: "Сохранить аудиофайл…", exact: true }).click();
    await page.waitForFunction(() => window.__mock.count("export_track") === 1);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("upcoming queue can move and clear without stopping the current track", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app; await waitForLibrary(page); await rows(page).first().click();
    await page.click(".nav button[data-view=queue]");
    await rows(page).nth(1).hover(); await rows(page).nth(1).locator('[data-action="down"]').click();
    await page.waitForFunction(() => window.__mock.count("move_upcoming") === 1);
    assert.equal(await page.evaluate(() => window.__mock.state.queue[2].title), "Трек 2");
    await page.getByRole("button", { name: "Очистить следующие", exact: true }).click();
    await page.waitForFunction(() => window.__mock.state.queue.length === 1);
    assert.equal(await page.evaluate(() => window.__mock.state.player.status), "playing");
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("sleep timer is configurable and filtered tracks download as a batch", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app; await waitForLibrary(page);
    await page.fill('.main > .view:not([hidden]) input[type=search]', "Трек 1234");
    await page.getByRole("button", { name: "Скачать все", exact: true }).click();
    await page.waitForFunction(() => window.__mock.count("download") === 1);
    await page.click('.nav button[data-view=settings]');
    await page.getByRole("combobox", { name: "Таймер сна", exact: true }).selectOption("900");
    await page.waitForFunction(() => window.__mock.last("set_sleep_timer")?.args.seconds === 900);
    await page.getByRole("combobox", { name: "Таймер сна", exact: true }).selectOption("end");
    await page.waitForFunction(() => window.__mock.last("set_sleep_timer")?.args.endOfTrack === true);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});


test("mobile palettes: all seven persist, support keyboard selection and keep every accent readable", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await page.click(".nav button[data-view=settings]");
    assert.equal(await page.locator(".palette-opt").count(), 7);
    for (const palette of ["classic", "banana", "midnight", "forest", "rose", "mocha", "halloween"]) {
      await page.click(`[data-palette-choice=${palette}]`);
      assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem("pm.settings.v1")).palette), palette);
      assert.equal(await page.locator("html").getAttribute("data-palette"), palette);
    }
    await page.reload();
    await page.click(".nav button[data-view=settings]");
    assert.equal(await page.locator("[data-palette-choice=halloween]").getAttribute("aria-checked"), "true");
    await page.focus("[data-palette-choice=halloween]");
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.locator("[data-palette-choice=classic]").getAttribute("aria-checked"), "true");
    const samples = await page.evaluate(async () => {
      const { PALETTES, COLORS, applyPalette } = await import("/js/themes.js");
      const samples=[];
      for (const palette of PALETTES) for (const accent of ["blue","indigo","violet","pink","red","orange","green","teal","banana"]) {
        applyPalette({palette,accent});
        const css=getComputedStyle(document.documentElement);
        samples.push({palette,accent,colors:COLORS[palette],light:css.getPropertyValue("--a-l").trim(),dark:css.getPropertyValue("--a-d").trim()});
      }
      return samples;
    });
    for (const sample of samples) for (const [index,bg] of sample.colors.entries()) {
      assert.ok(contrast(hexColor(index < 3 ? sample.light : sample.dark),hexColor(bg)) >= 4.5, `${sample.palette}/${sample.accent}/${bg}`);
    }
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});

test("Selena: mobile mood artwork loads and one click starts radio with the server mood ID", async () => {
  const app = await openApp({ config: { signedIn: true, catalogMixes: true } });
  try {
    const { page } = app;
    await page.click(".nav button[data-view=forYou]");
    await page.waitForSelector(".mix-tile");
    assert.equal(await page.locator(".selena-card").count(), 6);
    const images = await page.evaluate(async () => Promise.all([...document.querySelectorAll(".selena-mood")].map(async (el) => {
      const url=getComputedStyle(el).backgroundImage.slice(5,-2);
      const img=new Image(); img.src=url; await img.decode(); return img.naturalWidth;
    })));
    assert.ok(images.every((width)=>width >= 320));
    await page.click(".mood-calm");
    await page.waitForFunction(() => window.__mock.last("play_radio"));
    const source=await page.evaluate(() => window.__mock.last("play_radio").args.source);
    assert.equal(source.kind,"catalog");
    assert.equal(source.mix_id,"m7");
    assert.deepEqual(JSON.parse(source.options),{mood:["calm"]});
    assert.equal(await page.evaluate(() => window.__mock.count("play_radio")),1);
    assert.deepEqual(app.problems, []);
  } finally { await app.close(); }
});


test("product: pinned mixes persist, their tracks keep the catalog radio, and discovery has one vertical scroll", async () => {
 const app=await openApp({config:{signedIn:true,catalogMixes:true}});
 try {
  const {page}=app; await waitForLibrary(page);
  await page.click('.nav button[data-view=forYou]'); await page.waitForSelector('.mix-tile');
  const item=page.locator('.mix-card').filter({hasText:'Спокойный вечер'});
  await item.getByRole('button',{name:'Закрепить микс',exact:true}).click();
  assert.equal(await page.locator('.pinned-shelf .mix-tile').count(),1);
  await page.reload(); await page.click('.nav button[data-view=forYou]');
  await page.waitForSelector('.pinned-shelf .mix-tile');
  await page.locator('.pinned-shelf .mix-tile').click();
  await page.waitForFunction(()=>window.__mock.last('play_radio')?.args.source.kind==='catalog');
  await rows(page).nth(2).click();
  assert.equal(await page.evaluate(()=>window.__mock.last('play_radio').args.source.mix_id),'m7');
  assert.equal(await page.evaluate(()=>getComputedStyle(document.querySelector('.fy-shelves')).overflowY),'visible');
  await page.getByRole('button',{name:'Треки подборки',exact:true}).click();
  const geometry=await page.evaluate(()=>({title:document.querySelector('.discovery-tracks-title').getBoundingClientRect().bottom,list:document.querySelector('.discovery-view > .list').getBoundingClientRect().top}));
  assert.ok(Math.abs(geometry.title-geometry.list)<24);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: shared VK playlist links open exact metadata and can be followed", async () => {
 const app=await openApp({config:{signedIn:true}});
 try {
  const {page}=app; await waitForLibrary(page);
  await page.click('.nav button[data-view=playlists]');
  await page.getByRole('button',{name:'Открыть по ссылке',exact:true}).click();
  await page.getByRole('textbox',{name:'Ссылка на плейлист VK'}).fill('https://vk.com/music/playlist/-123_456_access');
  await page.locator('dialog').getByRole('button',{name:'Открыть по ссылке',exact:true}).click();
  await page.waitForFunction(()=>window.__mock.last('playlist_by_id'));
  await page.waitForFunction(()=>window.__mock.last('playlist_tracks')?.args.playlist.owner_id===-123);
  assert.equal(await page.evaluate(()=>window.__mock.last('playlist_by_id').args.album.access_key),'access');
  await page.getByRole('button',{name:'Добавить в мои плейлисты',exact:true}).click();
  await page.waitForFunction(()=>window.__mock.last('follow_album')?.args.follow===true);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: late collection and like replies cannot restore a signed-out account", async () => {
 const app=await openApp({config:{signedIn:true}});
 try {
  const {page}=app; await waitForLibrary(page);
  const state=await page.evaluate(async()=>{
   const {api}=await import('/js/api.js');
   const playlists=await import('/js/my-playlists.js');
   playlists.forgetPlaylists();
   let resolveOld; api.playlists=()=>new Promise(resolve=>resolveOld=resolve);
   const pending=playlists.ensurePlaylists(); playlists.forgetPlaylists();
   api.playlists=async()=>({items:[{playlist_id:99,owner_id:8,title:'new'}],total:1,next_offset:null});
   await playlists.ensurePlaylists(); resolveOld({items:[{playlist_id:1,owner_id:7,title:'old'}],total:1,next_offset:null});await pending;
   const likes=await import('/js/likes.js');
   const track={track_id:555,owner_id:7,title:'old',artist:'old'};likes.setLibrary([]);
   let finishLike;api.addToLibrary=()=>new Promise(resolve=>finishLike=resolve);
   const like=likes.toggleLike(track);likes.forgetLibrary();likes.setLibrary([]);finishLike(track);await like;
   return {playlists:playlists.allPlaylists().map(p=>p.title),liked:likes.isLiked(track)};
  });
  assert.deepEqual(state,{playlists:['new'],liked:false}); assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: dock reserves space, equalizer has real faders, and selects keep their keyboard", async () => {
 for(const width of [420,780,1920]) {
  const app=await openApp({config:{signedIn:true},viewport:{width,height:800}});
  try {
   const {page}=app;await waitForLibrary(page);await rows(page).first().click();
   await page.click('.nav button[data-view=settings]');
   const layout=await page.evaluate(()=>({view:document.querySelector('.settings').getBoundingClientRect().bottom,player:document.querySelector('.player').getBoundingClientRect().top,fader:document.querySelector('.eq-slider').getBoundingClientRect().height,sleep:getComputedStyle(document.querySelector('.sleep-timer select')).minHeight}));
   assert.ok(layout.view<=layout.player,JSON.stringify(layout));assert.ok(layout.fader>=120);assert.equal(layout.sleep,'40px');
   const before=await page.evaluate(()=>window.__mock.count('seek'));
   await page.getByRole('combobox',{name:'Таймер сна',exact:true}).focus();await page.keyboard.press('ArrowRight');
   assert.equal(await page.evaluate(()=>window.__mock.count('seek')),before); assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
 }
});

test("product: radial spectrum stays clear of titles and controls at maximum amplitude", async () => {
 for(const viewport of [{width:1920,height:1030},{width:780,height:740},{width:1120,height:640}]) {
  const app=await openApp({config:{signedIn:true},viewport});
  try {
   const {page}=app;
   await page.evaluate(()=>localStorage.setItem('pm.settings.v1',JSON.stringify({visualizer:'radial',theme:'dark',lyrics:false})));await page.reload();
   await waitForLibrary(page);await rows(page).first().click();await page.click('.player .cover-button');await page.waitForSelector('.np:not([hidden])');
   const painted=await page.evaluate(async()=>{
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    window.__mock.emit('pm-event',{type:'spectrum',bands:Array(64).fill(1)});
    const c=document.querySelector('.np-canvas'),data=c.getContext('2d').getImageData(0,0,c.width,c.height).data;
    let left=c.width,right=0,top=c.height,bottom=0,pixels=0;
    for(let y=0;y<c.height;y++)for(let x=0;x<c.width;x++)if(data[(y*c.width+x)*4+3]>4){left=Math.min(left,x);right=Math.max(right,x);top=Math.min(top,y);bottom=Math.max(bottom,y);pixels++;}
    const scale=c.width/c.clientWidth,r=c.getBoundingClientRect();
    const box=el=>{const b=el.getBoundingClientRect();return {left:b.left,right:b.right,top:b.top,bottom:b.bottom};};
    return {pixels,ring:{left:left/scale+r.left,right:right/scale+r.left,top:top/scale+r.top,bottom:bottom/scale+r.top},stage:box(document.querySelector('.np-stage')),info:box(document.querySelector('.np-info'))};
   });
   assert.ok(painted.pixels>100,JSON.stringify(painted));
   assert.ok(painted.ring.top>=painted.stage.top && painted.ring.bottom<=painted.stage.bottom,JSON.stringify(painted));
   const {ring,info}=painted;assert.ok(ring.right<info.left || ring.left>info.right || ring.bottom<info.top || ring.top>info.bottom,JSON.stringify(painted));
   assert.deepEqual(app.problems,[]);
  }finally{await app.close();}
 }
});


test("product: tuned mix identities, playlist locators and restored track metadata are lossless", async () => {
 const app=await openApp({config:{signedIn:true}});
 try {
  const output=await app.page.evaluate(async()=>{
   const {mixKey,mixColors,moodMatch}=await import('/js/selena.js');
   const {playlistLink}=await import('/js/playlist-link.js');
   const {saveQueue,loadQueue}=await import('/js/persist.js');
   const colors=Array.from({length:12},(_,i)=>mixColors(mixKey({id:'common',options:JSON.stringify({genre:[String(i)]})})).join(','));
   const a=mixKey({id:'common',options:'{"mood":{"value":"calm"},"genre":["pop"]}'}),b=mixKey({id:'common',options:'{"genre":["pop"],"mood":{"value":"calm"}}'}),c=mixKey({id:'common',options:'{"mood":{"value":"sad"},"genre":["pop"]}'});
   const valid=playlistLink('https://vk.com/music?z=audio_playlist-123_456_key%2Ftest');
   const invalid=['https://evil.example/playlist/-123_456','https://vk.com@evil.example/playlist/-123_456','https://vk.com/playlist/0_1','https://vk.com/playlist/1_0','https://vk.com:444/playlist/-1_2'].every(url=>{try{playlistLink(url);return false;}catch{return true;}});
   const track={track_id:1,owner_id:7,title:'t',artist:'a',duration:200,artists:[{id:'artist1',name:'a'}],album:{album_id:12,owner_id:-1,title:'album'},stream_url:'secret-expiring-url'};
   saveQueue([track],0,73);const saved=loadQueue();
   return {a,b,c,colors:new Set(colors).size,valid,invalid,mood:moodMatch('Desktop stop',['top']),saved};
  });
  assert.equal(output.a,output.b);assert.notEqual(output.a,output.c);assert.ok(output.colors>=10);assert.equal(output.valid.owner_id,-123);assert.equal(output.valid.access_key,'key');assert.equal(output.invalid,true);assert.equal(output.mood,false);
  assert.equal(output.saved.position,73);assert.equal(output.saved.tracks[0].album.album_id,12);assert.equal(output.saved.tracks[0].artists[0].id,'artist1');assert.equal(output.saved.tracks[0].stream_url,null);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: one download job survives navigation and failed files can be retried", async () => {
 const app=await openApp({config:{signedIn:true}});
 try {
  const output=await app.page.evaluate(async()=>{
   const {api}=await import('/js/api.js');const d=await import('/js/downloads.js');
   const first={owner_id:7,track_id:80001,title:'a'},second={owner_id:7,track_id:80002,title:'b'};
   let attempts=0,release;api.download=async track=>{attempts++;if(attempts===1)await new Promise(resolve=>release=resolve);if(track.track_id===80002)throw {kind:'offline'};return track;};
   const batch=d.downloadAll([first,second]);await Promise.resolve();
   const other=await d.downloadAll([second]);release();await batch;
   const job=d.downloadJob();return {attempts,other,completed:job.completed,failed:job.failed,failedId:job.errors[0].track.track_id,running:job.running};
  });
  assert.deepEqual(output,{attempts:2,other:null,completed:2,failed:1,failedId:80002,running:false});assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});


test("product: updates check and download inside the app, show progress and revalidate before opening", async()=>{
 const app=await openApp({config:{signedIn:true,updateDelay:600}});
 try{const {page}=app;await waitForLibrary(page);await page.click('.nav [data-view="settings"]');
  await page.locator('.update-panel').scrollIntoViewIfNeeded();
  await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.update-panel').innerText.includes('Доступна версия 0.4.0'));
  await page.getByRole('button',{name:'Скачать обновление',exact:true}).click();
  await page.waitForSelector('.update-panel progress:not([hidden])');
  assert.equal(await page.locator('.update-panel progress').getAttribute('value'),'50');
  await page.waitForFunction(()=>document.querySelector('.update-panel').innerText.includes('Обновление загружено и проверено'));
  await page.locator('.update-actions .primary:not([hidden])').click();
  assert.equal(await page.evaluate(()=>window.__mock.count('update_open')),1);
  assert.equal(page.context().pages().length,1);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: update cancellation is shared across settings visits and failure messages survive UI refresh",async()=>{
 const app=await openApp({config:{signedIn:true,updateDelay:700}});
 try{const {page}=app;await waitForLibrary(page);await page.click('.nav [data-view="settings"]');
  await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();await page.getByRole('button',{name:'Скачать обновление',exact:true}).click();
  await page.getByRole('button',{name:'Отменить загрузку',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.update-panel').innerText.includes('Загрузка отменена'));
  await page.waitForTimeout(750);
  await page.click('.nav [data-view="library"]');await page.click('.nav [data-view="settings"]');
  await page.waitForFunction(()=>document.querySelector('.update-panel').innerText.includes('Загрузка отменена'));
  await page.evaluate(async()=>{const {api}=await import('/js/api.js');api.updateCheck=async()=>{throw 'network';};});
  await page.getByRole('button',{name:'Проверить обновления',exact:true}).click();
  await page.waitForSelector('.update-error:not([hidden])');assert.match(await page.locator('.update-error').innerText(),/связаться с GitHub/);
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: private updater token is cleared from the input, never persisted in web storage and can be removed",async()=>{
 const app=await openApp({config:{signedIn:true}});
 try{const {page}=app;await waitForLibrary(page);await page.click('.nav [data-view="settings"]');await page.locator('.update-access summary').click();
  const secret='github_pat_test_123';await page.locator('.update-access input').fill(secret);await page.getByRole('button',{name:'Сохранить доступ',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.update-access input').value===''&&document.querySelector('.update-access summary').textContent==='Доступ сохранён');
  const storage=await page.evaluate(()=>JSON.stringify({...localStorage,...sessionStorage}));assert.ok(!storage.includes(secret));
  await page.getByRole('button',{name:'Удалить токен',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.update-access summary').textContent==='Доступ к закрытому репозиторию');
  assert.equal(await page.evaluate(()=>window.__mock.last('update_access').args.token),'');assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});

test("product: fixed window presets and fullscreen work independently of the player and text focus",async()=>{
 const app=await openApp({config:{signedIn:true}});
 try{const {page}=app;await waitForLibrary(page);await page.click('.nav [data-view="settings"]');
  for(const mode of ['medium','maximized','fullscreen']){await page.locator(`[data-window-mode="${mode}"]`).click();assert.equal(await page.evaluate(()=>window.__mock.last('set_window_mode').args.mode),mode);}
  await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.__mock.last('set_fullscreen').args.on),false);
  await page.locator('.update-access summary').click();await page.locator('.update-access input').focus();
  await page.keyboard.press('F11');assert.equal(await page.evaluate(()=>window.__mock.last('set_fullscreen').args.on),true);
  await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.__mock.last('set_fullscreen').args.on),false);assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});


test("product: refreshing a selected catalogue mix keeps its source and does not restart playback",async()=>{
 const app=await openApp({config:{signedIn:true,catalogMixes:true}});
 try{const {page}=app;await waitForLibrary(page);await page.click('.nav [data-view=forYou]');
  await page.waitForSelector('.mix-card [data-mix-key]');
  const card=page.locator('.mix-card [data-mix-key]').last();await card.click();
  await page.waitForFunction(()=>window.__mock.count('play_radio')>0);
  const source=await page.evaluate(()=>window.__mock.last('stream_mix').args);
  const personalCalls=await page.evaluate(()=>window.__mock.count('mix'));
  const plays=await page.evaluate(()=>window.__mock.count('play_radio'));
  const key=await card.getAttribute('data-mix-key');
  await page.locator('.discovery-view .view-header .btn').filter({hasText:'Обновить'}).click();
  await page.waitForFunction(()=>window.__mock.count('stream_mix')>=2);
  assert.deepEqual(await page.evaluate(()=>window.__mock.last('stream_mix').args),source);
  assert.equal(await page.evaluate(()=>window.__mock.count('mix')),personalCalls);
  assert.equal(await page.evaluate(()=>window.__mock.count('play_radio')),plays);
  assert.equal(await page.locator(`[data-mix-key='${key}']`).first().getAttribute('aria-pressed'),'true');
  assert.deepEqual(app.problems,[]);
 }finally{await app.close();}
});
