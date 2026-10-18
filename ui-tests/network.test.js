// Подсказка VPN / проблемы с подключением: отображается только при сбоях в сети, с успешной повторной
// попыткой, автоматической повторной попыткой, когда движок сообщает, что сеть восстановлена, и
// инструкциями на обоих языках.

import assert from "node:assert/strict";
import test from "node:test";
import { openApp } from "./harness.js";

const HINT_RU = "Если используете VPN, попробуйте отключить его (или включить, если VK недоступен) и повторить.";
const HINT_EN = "If you use a VPN, try turning it off (or on, if VK is unavailable) and try again.";

const libraryStatus = (page) => page.locator(".main > .view:not([hidden]) .status.error");

async function waitForLibrary(page) {
  await page.waitForFunction(() => document.querySelectorAll(".row").length > 0);
  await page.waitForFunction(() => !document.querySelector(".main > .view:not([hidden]) .status:not([hidden])"));
}

test("a failed library load by a network error shows the VPN card, Retry and the how-to link", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["library"], failKind: "offline" } });
  try {
    const { page } = app;
    await page.waitForSelector(".main > .view:not([hidden]) .status.error .net-hint");
    const card = libraryStatus(page).locator(".net-hint");
    assert.equal(await card.locator(".net-hint-title").innerText(), "Не удаётся подключиться к VK");
    assert.equal(await card.locator(".net-hint-text").innerText(), HINT_RU);
    assert.equal(await card.getByRole("button", { name: "Как проверить VPN" }).count(), 1);
    // На панели сохраняется собственная кнопка «Повторить», поэтому карта не добавляет вторую.
    assert.equal(await libraryStatus(page).getByRole("button", { name: "Повторить" }).count(), 1);
    assert.equal(await libraryStatus(page).locator(".retry").isVisible(), true);
    assert.deepEqual(app.problems.filter((p) => !p.includes("Failed to load resource")), []);
  } finally { await app.close(); }
});

test("timeouts get the card too, but sign-in, refused and playback errors do not", async () => {
  for (const [failKind, expected] of [["timeout", true], ["network", true], ["http", false], ["vk", false], ["invalid", false]]) {
    const app = await openApp({ config: { signedIn: true, failing: ["library"], failKind } });
    try {
      const { page } = app;
      await page.waitForSelector(".main > .view:not([hidden]) .status.error");
      assert.equal(await page.locator(".net-hint").count(), expected ? 1 : 0, `kind ${failKind}`);
      // Отклоненный запрос сохраняет обычную повторную попытку.
      if (!expected) assert.equal(await libraryStatus(page).locator(".retry").isVisible(), true);
    } finally { await app.close(); }
  }
});

test("Retry drops connections and loads again", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = app;
    await page.waitForSelector(".net-hint");
    const before = await page.evaluate(() => window.__mock.count("library"));
    await page.evaluate(() => { window.__mock.config.failing = []; });
    await page.getByRole("button", { name: "Повторить" }).click();
    await waitForLibrary(page);
    assert.equal(await page.evaluate(() => window.__mock.count("network_check")), 1);
    assert.ok((await page.evaluate(() => window.__mock.count("library"))) > before);
    assert.equal(await page.locator(".net-hint").count(), 0);
  } finally { await app.close(); }
});

test("when the engine reports the network back, the failed load repeats by itself and the user is told", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = app;
    await page.waitForSelector(".net-hint");
    // Все еще терпит неудачу: сам по себе "changed" ничего не делает.
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "network", state: "changed" }));
    assert.equal(await page.locator(".net-hint").count(), 1);
    await page.evaluate(() => {
      window.__mock.config.failing = [];
      window.__mock.emit("pm-event", { type: "network", state: "ok" });
    });
    await waitForLibrary(page);
    await page.waitForFunction(() => document.querySelector(".toast")?.textContent.includes("Соединение восстановлено"));
    assert.equal(await page.evaluate(() => window.__mock.count("network_check")), 0, "automatic: no button was pressed");
  } finally { await app.close(); }
});

test("the browser's own online event does the same", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = app;
    await page.waitForSelector(".net-hint");
    await page.evaluate(() => {
      window.__mock.config.failing = [];
      window.dispatchEvent(new Event("online"));
    });
    await waitForLibrary(page);
  } finally { await app.close(); }
});

test("nothing is said when the network is fine", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "network", state: "ok" }));
    await page.waitForTimeout(100);
    assert.equal(await page.locator(".toast").count(), 0);
    assert.equal(await page.locator(".net-hint, .net-chip, .net-banner").count(), 0);
    assert.ok((await page.evaluate(() => window.__mock.count("network_focus"))) >= 1, "the engine is told about the window's focus");
  } finally { await app.close(); }
});

test("the how-to dialog explains Windows, macOS and Linux and says a VPN is only one possible cause (ru)", async () => {
  const app = await openApp({ config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = app;
    await page.waitForSelector(".net-hint");
    await page.getByRole("button", { name: "Как проверить VPN" }).click();
    const dialog = page.locator("dialog.net-dialog");
    await dialog.waitFor();
    assert.equal(await dialog.locator("h2").innerText(), "Как проверить VPN");
    const text = await dialog.innerText();
    for (const part of ["Windows:", "macOS:", "Linux:", "Отключить", "Системные настройки", "nmcli", "VPN — только одна из возможных причин сбоя", "Скачанная музыка доступна без подключения к VK"]) {
      assert.ok(text.includes(part), `missing "${part}" in:\n${text}`);
    }
    await dialog.getByRole("button", { name: "Закрыть" }).click();
    assert.equal(await page.locator("dialog.net-dialog").count(), 0);
    assert.equal(await page.evaluate(() => document.body.classList.contains("overlay-open")), false);
  } finally { await app.close(); }
});

test("the same card and dialog in English", async () => {
  const app = await openApp({ language: "en-US", config: { signedIn: true, failing: ["library"] } });
  try {
    const { page } = app;
    await page.waitForSelector(".net-hint");
    assert.equal(await page.locator(".net-hint-title").innerText(), "Can't connect to VK");
    assert.equal(await page.locator(".net-hint-text").innerText(), HINT_EN);
    assert.equal(await page.getByRole("button", { name: "Try again" }).count(), 1);
    await page.getByRole("button", { name: "How to check your VPN" }).click();
    const text = await page.locator("dialog.net-dialog").innerText();
    for (const part of ["Windows:", "macOS:", "Linux:", "Disconnect", "System Settings", "A VPN is only one possible cause", "Downloaded music plays without a connection to VK"]) {
      assert.ok(text.includes(part), `missing "${part}" in:\n${text}`);
    }
  } finally { await app.close(); }
});

test("playback that gave up on the network shows the card above the player; Retry resumes it", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => {
      window.__mock.emit("pm-event", { type: "error", kind: "offline", message: "No internet connection." });
      window.__mock.emit("pm-event", { type: "recovery", phase: "failed", attempt: 6, max: 6, kind: "network" });
    });
    const banner = page.locator(".net-banner");
    await banner.waitFor();
    assert.equal(await page.locator(".toast.error").count(), 0, "the card replaces the error toast");
    assert.equal(await banner.locator(".net-hint-text").innerText(), HINT_RU);
    await banner.getByRole("button", { name: "Повторить" }).click();
    assert.equal(await page.evaluate(() => window.__mock.count("retry_playback")), 1);
    assert.equal(await page.evaluate(() => window.__mock.count("network_check")), 1);
    assert.equal(await page.locator(".net-banner").count(), 0);
  } finally { await app.close(); }
});

test("a playback error that is not about the network keeps its toast and gets no card", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => {
      window.__mock.emit("pm-event", { type: "error", kind: "stream", message: "Stream error" });
      window.__mock.emit("pm-event", { type: "recovery", phase: "failed", attempt: 6, max: 6, kind: "service" });
    });
    await page.waitForSelector(".toast.error");
    assert.equal(await page.locator(".net-banner, .net-hint").count(), 0);
  } finally { await app.close(); }
});

test("a calm chip while playback is being healed, and a note when it is", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "recovery", phase: "retrying", attempt: 1, max: 6, kind: "network" }));
    assert.equal(await page.locator(".net-chip").count(), 0, "the first, instant attempt is silent");
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "recovery", phase: "retrying", attempt: 2, max: 6, kind: "network" }));
    await page.waitForFunction(() => document.querySelector(".net-chip")?.textContent === "Восстанавливаем соединение… (2/6)");
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "recovery", phase: "recovered", attempt: 2, max: 6, kind: "network" }));
    await page.waitForFunction(() => !document.querySelector(".net-chip") && document.querySelector(".toast")?.textContent.includes("Соединение восстановлено"));
  } finally { await app.close(); }
});

test("an unreachable network is a chip that clears when it is back", async () => {
  const app = await openApp({ config: { signedIn: true } });
  try {
    const { page } = app;
    await waitForLibrary(page);
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "network", state: "unreachable" }));
    await page.waitForFunction(() => document.querySelector(".net-chip")?.textContent === "Нет подключения к сети");
    // Игра из загрузок этого не скрывает.
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "state", status: "playing" }));
    assert.equal(await page.locator(".net-chip").count(), 1);
    await page.evaluate(() => window.__mock.emit("pm-event", { type: "network", state: "ok" }));
    await page.waitForFunction(() => !document.querySelector(".net-chip"));
  } finally { await app.close(); }
});

test("sign-in: a login page that does not load shows the card; Retry asks for the window again", async () => {
  const app = await openApp({ config: { signedIn: false, loginPending: true } });
  try {
    const { page } = app;
    await page.click(".login .btn");
    await page.evaluate(() => window.__mock.emit("pm-login", "loadfailed"));
    await page.waitForSelector(".login .net-hint");
    assert.equal(await page.locator(".login .net-hint-text").innerText(), HINT_RU);
    await page.locator(".login .net-hint").getByRole("button", { name: "Повторить" }).click();
    assert.equal(await page.evaluate(() => window.__mock.count("login_start")), 2);
    assert.equal(await page.locator(".login .net-hint").count(), 0);
    // Ведь оно загрузилось.
    await page.evaluate(() => window.__mock.emit("pm-login", "loadfailed"));
    await page.waitForSelector(".login .net-hint");
    await page.evaluate(() => window.__mock.emit("pm-login", "loadok"));
    await page.waitForFunction(() => !document.querySelector(".login .net-hint"));
  } finally { await app.close(); }
});

test("sign-in: a network failure of the token request shows the card, an auth failure does not", async () => {
  const app = await openApp({ config: { signedIn: false, loginPending: true } });
  try {
    const { page } = app;
    await page.click(".login .btn");
    await page.evaluate(() => window.__mock.emit("pm-login", "error:timeout"));
    await page.waitForSelector(".login .net-hint");
    assert.equal(await page.locator(".login .message").innerText(), "Сервер не отвечает.");
    await page.evaluate(() => window.__mock.emit("pm-login", "cancelled"));
    await page.waitForFunction(() => !document.querySelector(".login .net-hint"));
    await page.evaluate(() => window.__mock.emit("pm-login", "error:unauthorized"));
    await page.waitForFunction(() => document.querySelector(".login .message").textContent === "Не удалось войти. Попробуйте ещё раз.");
    assert.equal(await page.locator(".login .net-hint").count(), 0);
  } finally { await app.close(); }
});

test("a one-line shelf error stays one line: a link to the instructions, the hint as its tooltip", async () => {
  const app = await openApp({ config: { signedIn: true, releasesFail: true } });
  try {
    const { page } = app;
    await page.click(".nav button[data-view=forYou]");
    await page.waitForSelector(".new-releases .status.error .net-how");
    const link = page.locator(".new-releases .status.error .net-how");
    assert.equal(await link.getAttribute("title"), HINT_RU);
    assert.equal(await page.locator(".new-releases .net-hint").count(), 0, "no card inside a one-line note");
    const height = await page.evaluate(() => document.querySelector(".new-releases").getBoundingClientRect().height);
    assert.ok(height > 0 && height < 80, `still one line: ${height}px`);
    await link.click();
    await page.waitForSelector("dialog.net-dialog");
  } finally { await app.close(); }
});
