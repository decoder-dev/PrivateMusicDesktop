import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

test("an empty playlist explains itself, in English and Russian", async () => {
  const app = await openApp({ config: { signedIn: true }, language: "en-US" });
  try {
    const { page } = app;
    await page.evaluate(() => localStorage.setItem("pm.settings.v1", JSON.stringify({ language: "en" })));
    await page.reload();
    await page.getByRole("heading", { name: "My music" }).waitFor();
    await page.locator('.nav [data-view="playlists"]').click();
    await page.getByRole("button", { name: "New playlist", exact: true }).click();
    await page.locator("dialog[open] input").first().fill("Empty hours");
    await page.locator("dialog[open] .btn.primary").click();
    await page.getByRole("button", { name: /Empty hours/ }).click();
    await page.getByText("This playlist has no tracks yet.").waitFor();
    assert.equal(await page.getByText("Nothing in your music yet.").count(), 0);
    assert.equal(await page.getByText("Your music is empty.").count(), 0);
  } finally { await app.close(); }
  const ru = await openApp({ config: { signedIn: true }, language: "ru-RU" });
  try {
    const { page } = ru;
    await page.waitForSelector("h1");
    await page.locator('.nav [data-view="playlists"]').click();
    await page.getByRole("button", { name: "Новый плейлист", exact: true }).click();
    await page.locator("dialog[open] input").first().fill("Пустой");
    await page.locator("dialog[open] .btn.primary").click();
    await page.getByRole("button", { name: /Пустой/ }).click();
    await page.getByText("В этом плейлисте пока нет треков.").waitFor();
    assert.equal(await page.getByText("В вашей музыке пока ничего нет.").count(), 0);
  } finally { await ru.close(); }
});

test("the English queue offers to clear upcoming tracks", async () => {
  const app = await openApp({ config: { signedIn: true }, language: "en-US" });
  try {
    const { page } = app;
    await page.evaluate(() => localStorage.setItem("pm.settings.v1", JSON.stringify({ language: "en" })));
    await page.reload();
    await page.locator(".row").first().click();
    await page.locator('.nav [data-view="queue"]').click();
    await page.getByRole("button", { name: "Clear upcoming", exact: true }).waitFor();
    assert.equal(await page.getByRole("button", { name: "Clear up next" }).count(), 0);
  } finally { await app.close(); }
});

test("small phone lyric tools stay inside the screen at 1.3 text scale", async () => {
  const app = await openApp({ config: { signedIn: true, catalogMixes: true, platform: "android" }, phone: "small" });
  try {
    const { page } = app;
    await page.locator(".stage-cta").click();
    await page.locator(".player .now").click();
    await page.locator(".np:not([hidden])").waitFor();
    await page.evaluate(() => document.documentElement.style.setProperty("--text-scale", "1.3"));
    await page.getByRole("button", { name: "Текст", exact: true }).click();
    await page.locator(".ly-edit-controls button", { hasText: "Сбросить" }).waitFor();
    const clipped = await page.evaluate(() => [...document.querySelectorAll(".ly-edit-controls button")].filter((el) => {
      const box = el.getBoundingClientRect();
      return box.width > 0 && (box.right > innerWidth + 1 || box.left < -1);
    }).map((el) => el.textContent));
    assert.deepEqual(clipped, []);
  } finally { await app.close(); }
});

test("starting the home mix is not held behind slow catalogue shelves", async () => {
  const app = await openApp({
    config: { signedIn: true, platform: "android", catalogMixes: true, recsDelay: 4000, catalogDelay: 4000 },
    phone: true,
  });
  try {
    const { page } = app;
    await page.locator(".stage-cta").click();
    const started = Date.now();
    await page.waitForFunction(() => window.__mock.calls.some((call) => call.command === "play_radio" || call.command === "play_tracks"));
    assert.ok(Date.now() - started < 2000, "play waited for the slow shelves");
  } finally { await app.close(); }
});
