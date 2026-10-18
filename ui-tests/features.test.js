import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";
test("smart playlists resume automatically when a slow account profile arrives",async()=>{
 const app=await openApp({config:{signedIn:true,libraryTotal:5,profileDelay:800}});try{const{page}=app;await page.waitForSelector('.row');await page.click('[data-view="smart"]');await page.waitForFunction(()=>!!document.querySelector('.account .name')?.textContent);await page.waitForFunction(()=>document.querySelector('.smart-view .meta')?.textContent.includes('5'));assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test("a visible smart playlist updates when a favourite is downloaded or removed",async()=>{
 const app=await openApp({config:{signedIn:true,libraryTotal:5}});try{const{page}=app;await ready(page);await page.click('[data-view="smart"]');await active(page).locator('select[aria-label="Умный плейлист"]').selectOption('offline');await page.waitForFunction(()=>document.querySelector('.smart-view h1')?.textContent==='Скачанное избранное');await page.evaluate(async()=>{const{toggleDownload}=await import('/js/downloads.js');const{libraryTracks}=await import('/js/likes.js');await toggleDownload(libraryTracks()[0]);});await page.waitForFunction(()=>document.querySelector('.smart-view .meta')?.textContent.includes('1'));await page.evaluate(async()=>{const{toggleLike,libraryTracks}=await import('/js/likes.js');await toggleLike(libraryTracks()[0]);});await page.waitForFunction(()=>document.querySelector('.smart-view .empty-state')?.hidden===false);assert.equal(await active(page).locator('.row').count(),0);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
test("new playback settings and smart rules are available in English",async()=>{
  const app=await openApp({language:"en-US",config:{signedIn:true,libraryTotal:5}});try{await ready(app.page);await app.page.click('[data-view="settings"]');assert.equal(await active(app.page).getByRole('switch',{name:'Gapless playback'}).count(),1);assert.equal(await active(app.page).getByRole('switch',{name:'Loudness normalization'}).count(),1);await app.page.click('[data-view="smart"]');await app.page.waitForFunction(()=>document.querySelector('.smart-view h1')?.textContent==='Rediscover');assert.equal(await active(app.page).getByText('Configure',{exact:true}).count(),1);assert.deepEqual(app.problems,[]);}finally{await app.close();}
});
const active = (page) => page.locator(".main > .view:not([hidden])");
async function ready(page) {
  await page.waitForFunction(() => document.querySelector(".row"));
  await page.waitForFunction(
    () => !!document.querySelector(".account .name")?.textContent,
  );
}

test("mass selection preserves one-click playback and queues a Shift range", async () => {
  const app = await openApp({ config: { signedIn: true, libraryTotal: 20 } });
  try {
    const { page } = app;
    await ready(page);
    const rows = active(page).locator(".row");
    await rows.nth(1).click({ modifiers: ["Control"] });
    await rows.nth(3).click({ modifiers: ["Shift"] });
    assert.equal(
      await page.evaluate(() => window.__mock.count("play_tracks")),
      0,
    );
    assert.equal(await active(page).locator(".row.multi-selected").count(), 3);
    await active(page).locator('[data-batch="queue"]').click();
    await page.waitForFunction(() => window.__mock.count("play_last") === 3);
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});
test("queue clear can be undone without unexpectedly starting music", async () => {
  const app = await openApp({ config: { signedIn: true, libraryTotal: 8 } });
  try {
    const { page } = app;
    await ready(page);
    await active(page).locator(".row").first().click();
    await page.click('[data-view="queue"]');
    await active(page).getByText("Очистить следующие", { exact: true }).click();
    await page.locator(".toast-action").click();
    await page.waitForFunction(() => window.__mock.count("load_queue") === 1);
    assert.equal(
      await page.evaluate(() => window.__mock.state.queue.length),
      8,
    );
    assert.equal(
      await page.evaluate(() => window.__mock.state.player.status),
      "idle",
    );
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});
test("smart playlist rules update from the library and validate custom limits", async () => {
  const app = await openApp({ config: { signedIn: true, libraryTotal: 25 } });
  try {
    const { page } = app;
    await ready(page);
    await page.click('[data-view="smart"]');
    await active(page)
      .locator('select[aria-label="Умный плейлист"]')
      .selectOption("recent");
    await page.waitForFunction(
      () =>
        document.querySelector(".main > .view:not([hidden]) h1")
          ?.textContent === "Недавно добавлено",
    );
    await active(page).getByText("Настроить", { exact: true }).click();
    await page.fill('dialog [name="limit"]', "7");
    await page.locator('dialog button[type="submit"]').click();
    await page.waitForFunction(() =>
      document
        .querySelector(".main > .view:not([hidden]) .meta")
        ?.textContent.includes("7"),
    );
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});
test("mini player uses the fixed native preset and can restore the normal window", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await ready(page);
    await page.click('[data-view="settings"]');
    await page.click('[data-window-mode="mini"]');
    await page.waitForSelector(".mini-player", { state: "visible" });
    assert.equal(
      await page.evaluate(
        () => window.__mock.last("set_window_mode").args.mode,
      ),
      "mini",
    );
    await page
      .locator('.mini-player [aria-label="Вернуться в приложение"]')
      .click();
    await page.waitForSelector(".mini-player", { state: "hidden" });
    assert.equal(
      await page.evaluate(
        () => window.__mock.last("set_window_mode").args.mode,
      ),
      "medium",
    );
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});
test("personal LRC offsets are used for highlighting and seeking and remain per account", async () => {
  const app = await openApp({ config: { signedIn: true, history: [1] } });
  try {
    const { page } = app;
    await ready(page);
    const result = await page.evaluate(async () => {
      const { parseLocalLrc, savePersonalLyrics, personalLyrics } =
        await import("/js/lyrics-personal.js");
      const { createLyricsPanel } = await import("/js/lyrics-panel.js");
      const track = window.__mock.state.history[0].track;
      const lyrics = parseLocalLrc("[00:01.00]Начало\n[00:02.50]Продолжение");
      savePersonalLyrics(track, { lyrics, offset: 1 });
      const panel = createLyricsPanel();
      document.body.append(panel.element);
      panel.setVisible(true);
      panel.setTrack(track);
      panel.update(2.1);
      panel.element.querySelector(".ly-line").click();
      return {
        time: window.__mock.last("seek").args.seconds,
        saved: personalLyrics(track).offset,
        count: lyrics.lines.length,
      };
    });
    assert.deepEqual(result, { time: 2, saved: 1, count: 2 });
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});
test("smart rules combine downloaded favourites, local month counts, and history", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const result = await app.page.evaluate(async () => {
      const { applySmartRule, DEFAULT_RULES } = await import(
        "/js/smart-rules.js"
      );
      const a = { track_id: 1, owner_id: 7 },
        b = { track_id: 2, owner_id: 7 };
      const data = {
        library: [a, b, a],
        history: [{ track: a, played_at: Date.now() / 1000 }],
        downloaded: [b],
        plays: { "7_2": { "2026-10": 3 } },
        now: new Date(2026, 9, 18).getTime(),
      };
      return DEFAULT_RULES.map((rule) =>
        applySmartRule(rule, data).map((t) => t.track_id),
      );
    });
    assert.deepEqual(result, [[2], [1, 2], [2], [2]]);
  } finally {
    await app.close();
  }
});

test("playlist sync adds new tracks and only removes files it owns", async () => {
  const app = await openApp({
    config: { signedIn: true, libraryTotal: 5, playlistTrackIds: [1, 2, 3] },
  });
  try {
    await ready(app.page);
    const result = await app.page.evaluate(async () => {
      const { setPlaylistSync, syncPlaylists, syncRules } = await import(
        "/js/playlist-sync.js"
      );
      const { writePersonal } = await import("/js/personal.js");
      const { downloadAll, downloads } = await import("/js/downloads.js");
      const playlist = {
        owner_id: 7,
        playlist_id: 55,
        title: "Проверка",
        count: 3,
      };
      setPlaylistSync(playlist, true);
      await syncPlaylists();
      const first = downloads()
        .tracks.map((t) => t.track_id)
        .sort();
      const manual = downloads().tracks.find((t) => t.track_id === 3);
      await downloadAll([manual]);
      writePersonal("syncRemoveObsolete", true);
      window.__mock.config.playlistTrackIds = [2, 4];
      await syncPlaylists();
      return {
        first,
        next: downloads()
          .tracks.map((t) => t.track_id)
          .sort(),
        error: syncRules()[0].error,
      };
    });
    assert.deepEqual(result, {
      first: [1, 2, 3],
      next: [2, 3, 4],
      error: null,
    });
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});

test("taste controls reach the engine and exclusions remain isolated by account", async () => {
  const app = await openApp({ config: { signedIn: true, libraryTotal: 5 } });
  try {
    await ready(app.page);
    const result = await app.page.evaluate(async () => {
      const { dislike, tastes } = await import("/js/tastes.js");
      const { store } = await import("/js/store.js");
      const track = { owner_id: 7, track_id: 1, artist: "Кино" };
      await dislike(track, "artists");
      const first = window.__mock.last("set_taste").args.taste;
      const saved = store.session.profile;
      store.session = { ...store.session, profile: { ...saved, id: 99 } };
      const second = tastes();
      store.session = { ...store.session, profile: saved };
      return {
        first: first.artists,
        second: second.artists,
        restored: tastes().artists,
      };
    });
    assert.deepEqual(result, {
      first: ["кино"],
      second: [],
      restored: ["кино"],
    });
    assert.deepEqual(app.problems, []);
  } finally {
    await app.close();
  }
});
