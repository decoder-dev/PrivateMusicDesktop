import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = path.dirname(fileURLToPath(import.meta.url));
const uiRoot = path.resolve(here, "../ui");
const mockSource = fs.readFileSync(path.join(here, "mock-backend.js"), "utf8");
const csp = JSON.parse(fs.readFileSync(path.resolve(here, "../src-tauri/tauri.conf.json"), "utf8")).app.security.csp;

const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".woff2": "font/woff2" };

export function startServer() {
  const server = http.createServer((request, response) => {
    const url = new URL(request.url, "http://localhost");
    const relative = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
    const file = path.resolve(uiRoot, relative);
    if (!file.startsWith(uiRoot) || !fs.existsSync(file)) {
      response.writeHead(404).end();
      return;
    }
    // Та же политика, по которой обслуживается реальное приложение: встроенный script/style здесь тоже
    // не сработает.
    response.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream", "content-security-policy": csp });
    response.end(fs.readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve({ server, url: `http://127.0.0.1:${server.address().port}/` })));
}

/** Phone profiles (Chromium emulation): the Pixel 7 the Android app is designed on, a small phone and a large one. */
export const PHONES = {
  pixel7: { viewport: { width: 412, height: 915 }, deviceScaleFactor: 2.625 },
  small: { viewport: { width: 360, height: 640 }, deviceScaleFactor: 3 },
  large: { viewport: { width: 430, height: 932 }, deviceScaleFactor: 3 },
  landscape: { viewport: { width: 915, height: 412 }, deviceScaleFactor: 2.625 },
};
const ANDROID_UA = "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36";

/**
 * `phone: true` (or a profile name from PHONES, or `{ viewport }`) opens the app the way the Android
 * WebView does: touch, a mobile viewport and an Android user agent. `query` is appended to the URL.
 */
export async function openApp({ config = {}, viewport = { width: 1120, height: 740 }, colorScheme = "light", language = "ru-RU", reducedMotion = "reduce", phone = false, query = "", fancyArt = false } = {}) {
  const { server, url: base } = await startServer();
  const profile = phone ? (typeof phone === "string" ? PHONES[phone] : phone === true ? PHONES.pixel7 : { ...PHONES.pixel7, ...phone }) : null;
  if (profile) viewport = profile.viewport;
  const url = base + query;
  const executablePath = process.env.PM_CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
  const browser = await chromium.launch({ executablePath: fs.existsSync(executablePath) ? executablePath : undefined, args: ["--no-sandbox"] });
  const context = await browser.newContext({
    viewport, colorScheme, locale: language, reducedMotion,
    ...(profile ? { deviceScaleFactor: profile.deviceScaleFactor, isMobile: true, hasTouch: true, userAgent: ANDROID_UA } : {}),
  });
  const page = await context.newPage();
  const problems = [];
  page.on("pageerror", (error) => problems.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console: ${message.text()}`);
  });
  // Иллюстрации URLs являются фейковыми: отвечайте им изображением 1x1, а не сетью.
  const pixel = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=", "base64");
  // Screenshot fixtures: true uses synthetic gradients; "photos" uses bundled mood images.
  // These are test covers, never replacements for real catalogue artwork in the app.
  await context.route(/^https?:\/\/(?!127\.0\.0\.1)/, (route) => {
    if (!fancyArt) return route.fulfill({ status: 200, contentType: "image/png", body: pixel });
    let hash = 0;
    for (const ch of route.request().url()) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
    if (fancyArt === "photos") {
      const names = ["love", "energy", "rain", "calm", "sun"];
      return route.fulfill({ status: 200, contentType: "image/webp", body: fs.readFileSync(path.join(uiRoot, `assets/discovery-${names[hash % names.length]}.webp`)) });
    }
    const hue = hash % 360;
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue} 70% 58%)"/><stop offset="1" stop-color="hsl(${(hue + 55) % 360} 65% 38%)"/></linearGradient></defs><rect width="300" height="300" fill="url(#g)"/><circle cx="${80 + hash % 140}" cy="${90 + hash % 120}" r="${50 + hash % 40}" fill="hsl(${(hue + 180) % 360} 60% 70%)" opacity=".35"/></svg>`;
    return route.fulfill({ status: 200, contentType: "image/svg+xml", body: svg });
  });
  await page.addInitScript(`window.__mockConfig = ${JSON.stringify(config)};`);
  await page.addInitScript(mockSource);
  try { await page.goto(url); }
  catch (error) {
    // Не оставляем браузер и сервер жить после ошибки открытия тестовой страницы.
    await browser.close();
    server.close();
    throw error;
  }
  return {
    page, problems, url, context,
    async close() {
      await browser.close();
      server.close();
    },
  };
}
