// Shared navigation through every phone screen, using visible controls.
const visible = (page, selector) => page.locator(selector).locator("visible=true");
const tap = async (page, selector) => {
  if (selector.includes(".sheet")) await page.waitForTimeout(300);
  await visible(page, selector).first().tap();
  await page.waitForTimeout(120);
};
const see = (page, selector) => visible(page, selector).first().waitFor();
const pressBack = page => page.evaluate(() => !window.dispatchEvent(new CustomEvent("pm-back", {cancelable:true})));
const tab = (page, id) => tap(page, `.dock-tab[data-tab="${id}"]`);

export async function everyPhoneScreen(page, check) {
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
  await see(page, ".main > .view:not([hidden]) h1");
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
  await see(page, ".main > .view:not([hidden]) h1");
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
