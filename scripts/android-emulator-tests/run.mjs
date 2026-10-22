// End-to-end checks of the real debug app on an Android emulator or a device, through the WebView's
// DevTools socket (Playwright connectOverCDP). No mock backend, no VK account: the app is signed out.
//
// Environment:
//   ANDROID_SERIAL        which device (optional if only one is attached)
//   PM_APK                install this APK first (adb install -r -g); otherwise the app must be installed
//   PM_OUT                output directory (default ./out): results.json, summary.md, screenshots, logcat
//   PM_CLEAR=1            `pm clear` first, so the app starts signed out (the CI sets it)
//   PM_REQUIRE_NETWORK=1  no internet on the device is a failure instead of a skipped step (the CI sets it)
//   PM_TOLERATE_PAGE_ERRORS=1  uncaught JavaScript errors in the page only warn
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  OUT, PACKAGE, adb, allPages, appIsFocused, appPage, connect, crashReport, disconnect, dumpPages, finish, focusedWindow, forceStop,
  invoke, log, logcat, pageErrors, pid, safeUrl, screencap, sh, shSafe, sleep, startApp, step, waitFor, warn,
} from "./lib.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const expectedVersion = JSON.parse(fs.readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8")).version;
const requireNetwork = process.env.PM_REQUIRE_NETWORK === "1";
const LOGIN_BUTTON = ".login .btn.primary";
const isVk = (host) => /(^|\.)vk\.(ru|com)$/.test(host);

/** invoke() that turns the engine's { kind, message } rejections into readable errors. */
async function call(page, command, args) {
  try { return await invoke(page, command, args); } catch (error) {
    const text = String(error?.message ?? error);
    throw new Error(`invoke("${command}") failed: ${text}`);
  }
}

async function networkAvailable() {
  const validated = Number((await shSafe("dumpsys connectivity | grep -c VALIDATED")) || 0);
  if (validated > 0) return true;
  return (await shSafe("ping -c 1 -W 5 1.1.1.1 >/dev/null 2>&1 && echo up")).includes("up");
}

/** Returns true when the network is there; otherwise fails (CI) or warns and tells the step to skip (local). */
async function needNetwork() {
  if (await networkAvailable()) return true;
  const message = "the device has no validated internet connection";
  if (requireNetwork) throw new Error(`${message} (PM_REQUIRE_NETWORK=1)`);
  warn(`${message}: skipped`);
  return false;
}

async function freshPage() {
  await connect();
  const page = await appPage();
  await waitFor("document.readyState === complete", () => page.evaluate(() => document.readyState === "complete"), { timeout: 60_000 });
  return page;
}

async function loginButton(page, timeout = 60_000) {
  const button = page.locator(LOGIN_BUTTON).first();
  await button.waitFor({ state: "visible", timeout });
  return button;
}

async function assertNoCrash(label) {
  const hits = crashReport(await logcat());
  if (hits.length) throw new Error(`${label}: crash evidence in logcat for ${PACKAGE}:\n${hits.slice(0, 3).join("\n---\n")}`);
}

// ------------------------------------------------------------------------------------------ preparation

log(`package ${PACKAGE}, expected version ${expectedVersion}, output ${OUT}`);
await adb(["wait-for-device"], { timeout: 120_000 });
log(`device: ${await sh("getprop ro.product.model")} / Android ${await sh("getprop ro.build.version.release")} (API ${await sh("getprop ro.build.version.sdk")})`);
if (process.env.PM_APK) {
  log(`installing ${process.env.PM_APK}`);
  await adb(["install", "-r", "-g", process.env.PM_APK], { timeout: 300_000 });
}
const installed = await shSafe(`pm path ${PACKAGE}`);
if (!installed) { console.error(`::error::${PACKAGE} is not installed on the device. Install the debug APK first (adb install -r -g <apk>).`); process.exit(2); }
if (process.env.PM_CLEAR === "1") await shSafe(`pm clear ${PACKAGE}`);
await shSafe(`pm grant ${PACKAGE} android.permission.POST_NOTIFICATIONS`);
await sh("logcat -c");
await forceStop();

let page;

// -------------------------------------------------------------------------------------------- the steps

await step("1. the app starts and survives 20 s without a crash", async () => {
  await startApp();
  const began = Date.now();
  while (Date.now() - began < 20_000) {
    if (!(await pid())) throw new Error("the app process disappeared during the first 20 s");
    await sleep(1000);
  }
  if (!(await appIsFocused())) throw new Error(`MainActivity is not in the foreground. Focus: ${await focusedWindow()}`);
  await assertNoCrash("after 20 s");
  return `pid ${await pid()}`;
}, { critical: true });

await step("2. the web UI loads from the app origin; phone layout flags", async () => {
  page = await freshPage();
  const info = await page.evaluate(() => ({
    origin: location.origin, title: document.title, agent: navigator.userAgent,
    platform: document.documentElement.dataset.platform, layout: document.documentElement.dataset.layout,
    width: innerWidth, height: innerHeight, ratio: devicePixelRatio, tauri: typeof window.__TAURI__?.core?.invoke,
  }));
  log(`page: ${JSON.stringify(info)}`);
  if (!/^https?:\/\/tauri\.localhost$/.test(info.origin) && info.origin !== "tauri://localhost") throw new Error(`unexpected page origin ${info.origin}`);
  if (info.title !== "Private Music") throw new Error(`unexpected page title "${info.title}"`);
  if (!/Android/.test(info.agent)) throw new Error(`the WebView user agent is not Android: ${info.agent}`);
  if (info.tauri !== "function") throw new Error("window.__TAURI__.core.invoke is not available in the page");
  // The phone layout comes from a sibling branch: absent flags only warn, wrong flags fail.
  if (info.platform === undefined && info.layout === undefined) {
    warn("the page sets neither data-platform nor data-layout on <html>: the phone layout is not in this build yet");
  } else {
    if (info.platform !== "android") throw new Error(`document.documentElement.dataset.platform is "${info.platform}", expected "android"`);
    if (info.layout !== "phone") throw new Error(`document.documentElement.dataset.layout is "${info.layout}", expected "phone" (viewport ${info.width}x${info.height})`);
  }
  await dumpPages("devtools-pages-main");
  return `origin ${info.origin}, viewport ${info.width}x${info.height}@${info.ratio}, platform=${info.platform ?? "-"} layout=${info.layout ?? "-"}`;
}, { critical: true });

await step("3. the sign-in screen is shown with a working primary button", async () => {
  const button = await loginButton(page);
  const box = await button.boundingBox();
  const text = (await button.innerText()).trim();
  if (!(await button.isEnabled())) throw new Error("the sign-in button is disabled");
  if (!text) throw new Error("the sign-in button has no label");
  const width = await page.evaluate(() => innerWidth);
  if (!box || box.width < 40 || box.height < 24) throw new Error(`the sign-in button is too small to tap: ${JSON.stringify(box)}`);
  if (box.x < 0 || box.x + box.width > width + 1) throw new Error(`the sign-in button sticks out of the ${width}px viewport: ${JSON.stringify(box)}`);
  await screencap("03-sign-in");
  return `button "${text}" ${Math.round(box.width)}x${Math.round(box.height)}`;
});

await step("4. the engine is initialised: app_info", async () => {
  const info = await call(page, "app_info");
  log(`app_info: ${JSON.stringify(info)}`);
  if (info.platform !== "android") throw new Error(`app_info.platform is "${info.platform}", expected "android"`);
  if (info.version !== expectedVersion) throw new Error(`app_info.version is "${info.version}", expected the workspace version "${expectedVersion}"`);
  if (typeof info.has_tray !== "boolean") throw new Error(`app_info.has_tray is not a boolean: ${JSON.stringify(info)}`);
  return `version ${info.version}, platform ${info.platform}`;
});

await step("5. the Keystore secure store works", async () => {
  const session = await call(page, "session_state");
  log(`session_state: ${JSON.stringify({ signed_in: session.signed_in, persistent: session.persistent })}`);
  if (session.signed_in) throw new Error("the app reports a signed-in session: start from a clean state (PM_CLEAR=1 / run-local.sh --clear)");
  if (session.persistent !== true) throw new Error("session_state.persistent is not true: the engine could not use the Keystore-backed secret store");
  // Direct round trip (debug builds only): write, read back, delete through SecureStorePlugin.
  const selftest = await call(page, "debug_secure_selftest");
  if (selftest !== "ok" && !String(selftest).startsWith("skipped")) throw new Error(`debug_secure_selftest returned ${JSON.stringify(selftest)}`);
  return `signed out, persistent store, round trip: ${selftest}`;
});

await step("6a. VK sign-in page loads in the native Activity (smoke hook)", async () => {
  if (!(await needNetwork())) return "skipped: no network";
  await disconnect();
  await forceStop();
  await shSafe(`run-as ${PACKAGE} rm -f files/pm_smoke_login.txt`);
  await startApp("--ez pm_smoke_login true");
  const report = await waitFor("files/pm_smoke_login.txt from the smoke mode", async () => {
    const text = await sh(`run-as ${PACKAGE} cat files/pm_smoke_login.txt`);
    return /^(loaded|error)\b/m.test(text) ? text : "";
  }, { timeout: 120_000, interval: 2000 });
  const first = report.split("\n")[0].trim();
  log(`smoke result: ${report.replace(/\n/g, " | ")}`);
  const trace = await shSafe("logcat -d -s PmSmokeLogin:V");
  if (trace) log(`PmSmokeLogin log:\n${trace}`);
  const match = first.match(/^loaded (\S+)/);
  if (!match) throw new Error(`the smoke mode did not load a VK page. Result file: ${report}`);
  if (!isVk(match[1])) throw new Error(`the smoke mode loaded a non-VK host: ${match[1]}`);
  await waitFor("the app to return to MainActivity after the smoke run", () => appIsFocused(), { timeout: 30_000 });
  return `loaded ${match[1]}`;
});

await step("6b. sign-in from the UI button opens VK; a non-VK URL is blocked; Back returns", async () => {
  if (!(await needNetwork())) return "skipped: no network";
  page = await freshPage();
  const button = await loginButton(page);
  await page.evaluate(async () => {
    window.__pmLogin = [];
    await window.__TAURI__.event.listen("pm-login", (event) => window.__pmLogin.push(String(event.payload).slice(0, 60)));
  });
  await button.click();
  await waitFor("LoginActivity in the foreground", () => appIsFocused("LoginActivity"), { timeout: 30_000 });
  log("LoginActivity is in the foreground");
  const events = await waitFor("the pm-login loadok / loadfailed event", async () => {
    const seen = await page.evaluate(() => window.__pmLogin);
    return seen.some((e) => e === "loadok" || e === "loadfailed") ? seen : null;
  }, { timeout: 90_000, interval: 1000 });
  log(`pm-login events: ${events.join(", ")}`);
  if (!events.includes("loadok")) throw new Error(`VK did not load in the sign-in Activity; events: ${events.join(", ")}`);

  // The Activity's WebView lives in the same process, so its page shows up on the same DevTools socket.
  const listing = await dumpPages("devtools-pages-login");
  const vkPage = await waitFor("the VK page among the DevTools pages", () => allPages().find((p) => { try { return isVk(new URL(p.url()).host); } catch { return false; } }), { timeout: 20_000 })
    .catch(() => null);
  let detail = `events ${events.join(",")}`;
  if (!vkPage) {
    warn(`the VK page is not listed by DevTools (pages: ${listing.map((p) => p.url).join(", ")}); relying on the loadok event`);
  } else {
    detail += `, page ${safeUrl(vkPage.url())}`;
    await vkPage.screenshot({ path: path.join(OUT, "06-vk-login-page.png"), timeout: 15_000 }).catch((e) => log(`VK page screenshot failed: ${e.message.split("\n")[0]}`));
    // A page-initiated jump to a non-VK site must not happen (shouldOverrideUrlLoading / shouldInterceptRequest).
    await vkPage.evaluate(() => { setTimeout(() => location.assign("https://example.org/pm-ci-blocked"), 0); }).catch(() => {});
    await sleep(8000);
    const hosts = allPages().map((p) => { try { return new URL(p.url()).host; } catch { return "?"; } });
    if (hosts.includes("example.org")) warn("a non-VK URL (example.org) was NOT blocked in the sign-in Activity");
    else detail += ", example.org blocked";
  }

  // Back: WebView history first, then the Activity closes (cancelled).
  let back = 0;
  for (; back < 10; back++) {
    if (await appIsFocused("MainActivity")) break;
    await sh("input keyevent KEYCODE_BACK");
    await sleep(2000);
  }
  if (!(await appIsFocused("MainActivity"))) throw new Error(`Back (${back} presses) did not return to MainActivity. Focus: ${await focusedWindow()}`);
  await waitFor("the sign-in button to be enabled again", async () => (await page.locator(LOGIN_BUTTON).first().isVisible()) && (await page.locator(LOGIN_BUTTON).first().isEnabled()), { timeout: 20_000 });
  const after = await page.evaluate(() => window.__pmLogin);
  log(`pm-login events after Back: ${after.join(", ")}`);
  if (!after.includes("cancelled")) warn(`no "cancelled" event after Back (events: ${after.join(", ")})`);
  await assertNoCrash("after the sign-in Activity");
  return `${detail}, back to the sign-in screen after ${back} press(es)`;
});

await step("7. hardware Back on the main screen sends the app to the background; relaunch works", async () => {
  page = await freshPage();
  await page.evaluate(() => { window.__pmMarker = "alive"; });
  await sh("input keyevent KEYCODE_BACK");
  await waitFor("MainActivity to leave the foreground", async () => !(await appIsFocused()), { timeout: 15_000 });
  if (!(await pid())) throw new Error("Back killed the app process instead of moving it to the background");
  log(`after Back: ${await focusedWindow()}`);
  await sleep(2000);
  await assertNoCrash("after Back");
  await startApp();
  await waitFor("MainActivity back in the foreground", () => appIsFocused(), { timeout: 30_000 });
  page = await freshPage();
  const marker = await page.evaluate(() => window.__pmMarker ?? null);
  if (marker !== "alive") warn("the page was reloaded while the app was in the background (marker lost)");
  await loginButton(page, 30_000);
  return marker === "alive" ? "same page after relaunch" : "page reloaded after relaunch";
});

await step("8. rotation keeps the page alive", async () => {
  page = await freshPage();
  await page.evaluate(() => { window.__pmMarker = "rotation"; });
  const size = () => page.evaluate(() => ({ w: innerWidth, h: innerHeight }));
  const before = await size();
  try {
    await sh("settings put system accelerometer_rotation 0");
    await sh("settings put system user_rotation 1");
    const landscape = await waitFor("landscape viewport", async () => { const s = await size(); return s.w > s.h ? s : null; }, { timeout: 20_000 }).catch(() => null);
    if (!landscape) warn(`the viewport did not become landscape (still ${JSON.stringify(await size())})`);
    await sleep(1500);
    if (!(await pid())) throw new Error("the app died on rotation");
    const marker = await page.evaluate(() => window.__pmMarker);
    if (marker !== "rotation") throw new Error("the page was reloaded by the rotation (configChanges should keep it)");
    await loginButton(page, 20_000);
    await screencap("08-landscape");
    await sh("settings put system user_rotation 0");
    await waitFor("portrait viewport", async () => { const s = await size(); return s.h >= s.w; }, { timeout: 20_000 }).catch(() => warn("the viewport did not return to portrait"));
    await assertNoCrash("after rotation");
    return `${before.w}x${before.h} -> ${landscape ? `${landscape.w}x${landscape.h}` : "no change"} -> back`;
  } finally {
    await shSafe("settings put system user_rotation 0");
    await shSafe("settings put system accelerometer_rotation 1");
  }
});

await step("9. notification permission path and the media plugin status", async () => {
  const status = async () => {
    const p = await freshPage();
    return call(p, "plugin:media-session|status");
  };
  // Revoking a runtime permission restarts the app process.
  await shSafe(`pm revoke ${PACKAGE} android.permission.POST_NOTIFICATIONS`);
  await sleep(2000);
  await startApp();
  const denied = await status();
  log(`media status, notifications revoked: ${JSON.stringify(denied)}`);
  if (typeof denied !== "object" || denied === null) throw new Error(`media-session status is not an object: ${JSON.stringify(denied)}`);
  await sh(`pm grant ${PACKAGE} android.permission.POST_NOTIFICATIONS`);
  await sleep(1000);
  const granted = await status();
  log(`media status, notifications granted: ${JSON.stringify(granted)}`);
  const enabled = granted.notificationsEnabled ?? granted.notifications_enabled;
  if (enabled !== true) warn(`media-session status does not report notifications enabled after the grant: ${JSON.stringify(granted)}`);
  if (!(await pid())) throw new Error("the app is not running after the permission change");
  await assertNoCrash("after the permission change");
  return `status ${JSON.stringify(granted)}`;
});

await step("10. no crash in the whole log, no uncaught page errors", async () => {
  await assertNoCrash("whole run");
  const errors = pageErrors();
  if (errors.length) {
    const text = `uncaught JavaScript errors in the page:\n${errors.slice(0, 5).join("\n")}`;
    if (process.env.PM_TOLERATE_PAGE_ERRORS === "1") warn(text); else throw new Error(text);
  }
  return "clean";
});

// ------------------------------------------------------------------------------------------- the end

await disconnect();
fs.writeFileSync(path.join(OUT, "logcat-after-tests.txt"), await logcat());
await screencap("99-final");
const ok = finish();
process.exit(ok ? 0 : 1);
