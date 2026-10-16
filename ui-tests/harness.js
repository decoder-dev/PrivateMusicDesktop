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
    // Same policy the real app is served with: inline script/style would fail here too.
    response.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream", "content-security-policy": csp });
    response.end(fs.readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve({ server, url: `http://127.0.0.1:${server.address().port}/` })));
}

export async function openApp({ config = {}, viewport = { width: 1120, height: 740 }, colorScheme = "light", language = "ru-RU", reducedMotion = "reduce" } = {}) {
  const { server, url } = await startServer();
  const executablePath = process.env.PM_CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome";
  const browser = await chromium.launch({ executablePath: fs.existsSync(executablePath) ? executablePath : undefined, args: ["--no-sandbox"] });
  const context = await browser.newContext({ viewport, colorScheme, locale: language, reducedMotion });
  const page = await context.newPage();
  const problems = [];
  page.on("pageerror", (error) => problems.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console: ${message.text()}`);
  });
  // Artwork URLs are fake: answer them with a 1x1 image, never the network.
  const pixel = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=", "base64");
  await context.route(/^https?:\/\/(?!127\.0\.0\.1)/, (route) => route.fulfill({ status: 200, contentType: "image/png", body: pixel }));
  await page.addInitScript(`window.__mockConfig = ${JSON.stringify(config)};`);
  await page.addInitScript(mockSource);
  await page.goto(url);
  return {
    page, problems,
    async close() {
      await browser.close();
      server.close();
    },
  };
}
