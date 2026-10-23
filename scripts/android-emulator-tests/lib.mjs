// Helpers for the Android emulator tests: adb, a tiny step runner and the DevTools (CDP) connection to the
// app's WebView. No test framework: every step logs what it does, and a failure explains itself.
import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { _android as android } from "playwright";

export const PACKAGE = process.env.PM_PACKAGE ?? "com.dec.privatemusic2.android";
export const OUT = path.resolve(process.env.PM_OUT ?? "out");
export const CDP_PORT = Number(process.env.PM_CDP_PORT ?? 9222);
export const SERIAL = process.env.ANDROID_SERIAL ?? "";
fs.mkdirSync(OUT, { recursive: true });

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
export const log = (...parts) => console.error(`[${new Date().toISOString().slice(11, 19)}]`, ...parts); // stderr: stdout is for results

// ---------------------------------------------------------------------------------------------- adb

function run(args, { timeout = 60_000, binary = false } = {}) {
  const full = SERIAL ? ["-s", SERIAL, ...args] : args;
  return new Promise((resolve, reject) => {
    execFile("adb", full, { timeout, maxBuffer: 256 * 1024 * 1024, encoding: binary ? "buffer" : "utf8" }, (error, stdout, stderr) => {
      if (error) {
        const detail = `${binary ? "" : stdout}${stderr}`.trim().slice(-600);
        reject(new Error(`adb ${full.join(" ")} failed: ${error.message}${detail ? `\n${detail}` : ""}`));
      } else resolve(stdout);
    });
  });
}

export const adb = (args, options) => run(args, options);
/** `adb shell <command>`; the whole string is interpreted by the device shell. Returns stdout, trimmed. */
export const sh = async (command, options) => (await run(["shell", command], options)).trim();
/** Like sh, but never throws: returns "" (and logs) on failure. */
export const shSafe = async (command, options) => {
  try { return await sh(command, options); } catch (error) { log(`(ignored) ${error.message.split("\n")[0]}`); return ""; }
};

export async function screencap(name) {
  try {
    const png = await run(["exec-out", "screencap", "-p"], { binary: true, timeout: 30_000 });
    if (png.length < 100) throw new Error("empty screenshot");
    const file = path.join(OUT, `${name}.png`);
    fs.writeFileSync(file, png);
    log(`screenshot: ${file}`);
    return file;
  } catch (error) {
    log(`screenshot ${name} failed: ${error.message.split("\n")[0]}`);
    return null;
  }
}

export const pid = async () => (await shSafe(`pidof ${PACKAGE}`)).split(/\s+/)[0] ?? "";
export const focusedWindow = async () => {
  const text = await shSafe("dumpsys window | grep -E 'mCurrentFocus|mFocusedApp'");
  return text.split("\n").map((line) => line.trim()).filter(Boolean).join(" | ");
};
export const appIsFocused = async (activity = "MainActivity") => {
  const text = await shSafe("dumpsys window | grep mCurrentFocus");
  return text.includes(`${PACKAGE}/`) && text.includes(activity);
};

export async function logcat() {
  return run(["logcat", "-d", "-v", "threadtime"], { timeout: 20_000 });
}

/** Crash evidence for our package in a logcat dump: Java crashes, native crashes, ANRs. */
export function crashReport(text, pkg = PACKAGE) {
  const lines = text.split("\n");
  const hits = [];
  lines.forEach((line, i) => {
    if (/FATAL EXCEPTION/.test(line)) {
      const window = lines.slice(i, i + 6).join("\n");
      if (window.includes(pkg)) hits.push(window);
    } else if (/Fatal signal \d+/.test(line) && (line.includes(pkg) || lines.slice(i, i + 4).some((l) => l.includes(pkg)))) {
      hits.push(lines.slice(i, i + 4).join("\n"));
    } else if (/ANR in /.test(line) && line.includes(pkg)) {
      hits.push(line);
    } else if (/Process: /.test(line) && line.includes(`Process: ${pkg}`) && /AndroidRuntime/.test(line)) {
      hits.push(line);
    }
  });
  return [...new Set(hits)];
}

export async function startApp(extras = "") {
  // -W waits for the launch to finish; --activity-clear-top is not needed: the activity is singleTask.
  const out = await sh(`am start -W -n ${PACKAGE}/.MainActivity ${extras}`, { timeout: 90_000 });
  log(`am start: ${out.replace(/\s+/g, " ").slice(0, 200)}`);
  if (/Error|Exception/.test(out)) throw new Error(`am start failed: ${out}`);
}
export const forceStop = () => shSafe(`am force-stop ${PACKAGE}`);

export async function waitFor(what, check, { timeout = 60_000, interval = 500 } = {}) {
  const start = Date.now();
  let last;
  while (Date.now() - start < timeout) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) { last = error; }
    await sleep(interval);
  }
  throw new Error(`timed out after ${Math.round(timeout / 1000)} s waiting for ${what}${last ? ` (last error: ${String(last.message).split("\n")[0]})` : ""}`);
}

// ---------------------------------------------------------------------------------------------- CDP

let connection = null;
const collectedErrors = []; // survive reconnects

export async function disconnect() {
  if (connection) {
    try { await connection.device.close(); } catch { /* the app may be gone */ }
    connection = null;
  }
  await run(["forward", "--remove", `tcp:${CDP_PORT}`], { timeout: 5_000 }).catch(() => {});
}

async function socketName() {
  const processId = await pid();
  if (!processId) throw new Error(`${PACKAGE} is not running`);
  const unix = await shSafe("cat /proc/net/unix");
  const wanted = `webview_devtools_remote_${processId}`;
  if (!unix.includes(wanted)) {
    const others = unix.split("\n").filter((l) => l.includes("devtools_remote")).map((l) => l.split(/\s+/).pop());
    throw new Error(`no @${wanted} socket yet (devtools sockets present: ${others.join(", ") || "none"})`);
  }
  return wanted;
}

/** Forwards the app's WebView DevTools socket and connects Playwright to it. Reconnects from scratch each time. */
export async function connect({ timeout = 60_000 } = {}) {
  await disconnect();
  const socket = await waitFor("the WebView DevTools socket of the app", socketName, { timeout });
  await run(["forward", `tcp:${CDP_PORT}`, `localabstract:${socket}`]);
  log(`forwarded tcp:${CDP_PORT} -> @${socket}`);
  const base = `http://127.0.0.1:${CDP_PORT}`;
  try {
    const version = await (await fetch(`${base}/json/version`, { signal: AbortSignal.timeout(10_000) })).text();
    log(`DevTools /json/version: ${version.replace(/\s+/g, " ").slice(0, 300)}`);
    fs.writeFileSync(path.join(OUT, "devtools-version.json"), version);
  } catch (error) { log(`/json/version unreachable: ${error.message}`); }
  // Playwright's Android API is made for WebViews: connectOverCDP needs browser-level context management,
  // which a WebView's DevTools does not offer ("Browser context management is not supported").
  const device = await waitFor("Playwright to see the emulator through adb", async () => {
    const found = await android.devices();
    if (!found.length) throw new Error("no adb devices");
    return found[0];
  }, { timeout, interval: 2000 });
  connection = { device, pages: [], known: new Set() };
  await refreshPages();
  return device;
}

const watch = (page) => {
  page.on("pageerror", (error) => collectedErrors.push(`${page.url()}: ${error.message}`));
  page.on("console", (message) => { if (message.type() === "error") log(`page console.error: ${message.text().slice(0, 300)}`); });
};

/** Adopts the app's WebViews (the main window, the VK sign-in Activity) that appeared since the last call. */
export async function refreshPages() {
  if (!connection) return [];
  for (const view of connection.device.webViews()) {
    if (view.pkg() !== PACKAGE) continue;
    const key = `${view.pid}:${view.socketName ?? ""}:${view.pkg()}`;
    if (connection.known.has(key)) continue;
    try {
      const page = await view.page();
      connection.known.add(key);
      connection.pages.push(page);
      watch(page);
    } catch (error) { log(`WebView ${key} is not ready: ${String(error.message).split("\n")[0]}`); }
  }
  connection.pages = connection.pages.filter((page) => !page.isClosed());
  return connection.pages;
}

export const pageErrors = () => collectedErrors;
export const allPages = () => (connection ? connection.pages : []);

export async function dumpPages(name) {
  try {
    const list = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`, { signal: AbortSignal.timeout(10_000) })).json();
    const redacted = list.map(({ title, type, url }) => ({ title, type, url: safeUrl(url) }));
    fs.writeFileSync(path.join(OUT, `${name}.json`), JSON.stringify(redacted, null, 2));
    return redacted;
  } catch (error) { log(`/json/list failed: ${error.message}`); return []; }
}

/** scheme://host only: URLs of the VK page can carry tokens in the query. */
export function safeUrl(url) {
  try { const u = new URL(url); return `${u.protocol}//${u.host}`; } catch { return String(url).slice(0, 40); }
}

const isApp = (url) => /^https?:\/\/tauri\.localhost(\/|$)/.test(url) || /^tauri:\/\/localhost/.test(url);

/** The app's own page (the Tauri origin), waiting for it to exist. */
export async function appPage({ timeout = 60_000 } = {}) {
  return waitFor("the app's page (origin tauri.localhost) in the WebView", async () => {
    await refreshPages();
    const found = allPages().find((page) => isApp(page.url()));
    if (found) return found;
    const seen = allPages().map((page) => safeUrl(page.url())).join(", ") || "no pages";
    throw new Error(`pages seen: ${seen}`);
  }, { timeout, interval: 1000 });
}

/** Tauri IPC from the page, with a clear error when the global is missing. */
export const invoke = (page, command, args = {}) =>
  page.evaluate(async ({ command, args }) => {
    const core = window.__TAURI__?.core;
    if (!core) throw new Error("window.__TAURI__.core is missing (withGlobalTauri off, or the page is not the app)");
    try { return await core.invoke(command, args); } catch (error) {
      throw new Error(typeof error === "string" ? error : JSON.stringify(error));
    }
  }, { command, args });

// ---------------------------------------------------------------------------------------------- steps

const results = [];
export const warnings = [];
export const warn = (text) => { warnings.push(text); log(`WARNING: ${text}`); if (process.env.GITHUB_ACTIONS) console.log(`::warning::${text}`); };

/**
 * Runs one named step. `critical` steps stop the run when they fail (the rest could not mean anything).
 * On failure: a screenshot, the focused window and the last app log lines are saved/printed.
 */
export async function step(name, fn, { critical = false } = {}) {
  if (results.some((r) => r.critical && r.status === "fail")) {
    results.push({ name, status: "skip", detail: "skipped: an earlier critical step failed" });
    log(`SKIP  ${name}`);
    return;
  }
  const index = String(results.length + 1).padStart(2, "0");
  const started = Date.now();
  log(`---- ${name}`);
  const warnBefore = warnings.length;
  try {
    const detail = await fn();
    const extra = warnings.slice(warnBefore);
    results.push({ name, status: extra.length ? "warn" : "pass", detail: [detail, ...extra].filter(Boolean).join("; "), ms: Date.now() - started });
    log(`${extra.length ? "WARN" : "PASS"}  ${name}`);
  } catch (error) {
    let deviceAvailable = false;
    try { deviceAvailable = (await adb(["get-state"], { timeout: 5_000 })).trim() === "device"; } catch { /* infrastructure failure */ }
    results.push({ name, status: "fail", critical: critical || !deviceAvailable,
      detail: `${error.message}${deviceAvailable ? "" : "; adb device unavailable; remaining device checks skipped"}`, ms: Date.now() - started });
    log(`FAIL  ${name}\n${error.stack ?? error.message}`);
    if (!deviceAvailable) return;
    log(`focused window: ${await focusedWindow()}`);
    log(`process: ${(await pid()) || "NOT RUNNING"}`);
    await screencap(`fail-${index}`);
    const tail = (await shSafe(`logcat -d -v threadtime | grep -E "${PACKAGE}|AndroidRuntime|RustStdoutStderr|PrivateMusic|PmSmokeLogin|chromium" | tail -60`, { timeout: 60_000 }));
    if (tail) log(`--- recent app log\n${tail}\n---`);
  }
}

export function finish() {
  fs.writeFileSync(path.join(OUT, "results.json"), JSON.stringify(results, null, 2));
  const icon = { pass: "PASS", warn: "WARN", fail: "FAIL", skip: "SKIP" };
  const rows = results.map((r) => `| ${icon[r.status]} | ${r.name} | ${String(r.detail ?? "").replace(/\|/g, "\\|").replace(/\n/g, " ").slice(0, 300)} |`);
  const table = ["| Result | Step | Detail |", "| --- | --- | --- |", ...rows].join("\n");
  fs.writeFileSync(path.join(OUT, "summary.md"), `${table}\n`);
  console.log(`\n${table}\n`);
  if (process.env.GITHUB_STEP_SUMMARY) {
    try { fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### Android emulator tests\n\n${table}\n\n`); } catch { /* not fatal */ }
  }
  return results.every((r) => r.status !== "fail");
}
