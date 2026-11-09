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
  OUT, PACKAGE, adb, allPages, refreshPages, appIsFocused, appPage, connect, crashReport, disconnect, dumpPages, finish, focusedWindow, forceStop,
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
  // `am start -W` can return before the process has its package name (LaunchState UNKNOWN on a cold first
  // start), so pidof would miss a healthy app: first wait for the process to appear, then watch it.
  await waitFor("the app process", async () => Boolean(await pid()), { timeout: 30_000 });
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

await step("5b. native cover decoding preserves orientation and strips source metadata",async()=>{
  const result=await call(page,"debug_cover_selftest");
  if(result!=="ok")throw new Error(`debug_cover_selftest returned ${JSON.stringify(result)}`);
  return "generated JPEG: mirrored/rotated square crops and metadata removal";
});

await step("5c. native clipboard accepts explicit generated lyric text",async()=>{
  await call(page,"copy_text",{text:"Private Music generated clipboard check\nТестовая строка"});
  let rejected=false;
  try{await invoke(page,"copy_text",{text:" \n\t"});}catch{rejected=true;}
  if(!rejected)throw new Error("empty clipboard text was accepted");
  return "generated text copied through Android IPC; empty text rejected";
});

await step("5d. real playback: a bundled tone plays through the audio output and the position moves",async()=>{
  // Passed on API 34 and 36 emulators in CI: a failure here is a real playback regression.
  let result;
  try{result=await call(page,"debug_playback_selftest");}
  catch(error){
    await assertNoCrash("after the playback self-test");
    throw new Error(`debug_playback_selftest failed: ${error.message}`);
  }
  await assertNoCrash("after the playback self-test");
  return String(result);
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

await step("6b. sign-in from the UI button opens VK; Back returns", async () => {
  if (!(await needNetwork())) return "skipped: no network";
  page = await freshPage();
  const button = await loginButton(page);
  await page.evaluate(async () => {
    window.__pmLogin = [];
    await window.__TAURI__.event.listen("pm-login", (event) => window.__pmLogin.push(String(event.payload).slice(0, 60)));
  });
  await sh("logcat -c").catch(() => {});
  await button.click();
  // DevTools stays detached while VK's page is open: with it attached the software-rendered emulator
  // repeatedly crashed (qemu SIGSEGV) about 10 s after the second WebView appeared. The URL rules of the
  // sign-in screen are covered by the Kotlin unit tests (VkUrlPolicyTest).
  await disconnect();
  await waitFor("LoginActivity in the foreground", () => appIsFocused("LoginActivity"), { timeout: 30_000 });
  log("LoginActivity is in the foreground");
  const loaded = await waitFor("VK's page in the sign-in Activity (logcat)", async () => {
    const text = await shSafe("logcat -d -s VkLogin:I", { timeout: 20_000 });
    const match = text.match(/page loaded: https page on ([a-z0-9.-]+)/);
    return match ? match[1] : null;
  }, { timeout: 90_000, interval: 2000 });
  log(`VK page loaded: ${loaded}`);
  if (!isVk(loaded)) throw new Error(`the sign-in Activity loaded a non-VK host: ${loaded}`);

  // Back: WebView history first, then the Activity closes (cancelled).
  let back = 0;
  for (; back < 10; back++) {
    if (await appIsFocused("MainActivity")) break;
    await sh("input keyevent KEYCODE_BACK");
    await sleep(2000);
  }
  if (!(await appIsFocused("MainActivity"))) throw new Error(`Back (${back} presses) did not return to MainActivity. Focus: ${await focusedWindow()}`);
  page = await freshPage();
  await waitFor("the sign-in button to be enabled again", async () => (await page.locator(LOGIN_BUTTON).first().isVisible()) && (await page.locator(LOGIN_BUTTON).first().isEnabled()), { timeout: 20_000 });
  const after = await page.evaluate(() => window.__pmLogin ?? []);
  log(`pm-login events: ${after.join(", ")}`);
  if (!after.includes("loadok") && !after.includes("cancelled")) warn(`no "loadok"/"cancelled" event reached the page (events: ${after.join(", ")})`);
  await assertNoCrash("after the sign-in Activity");
  return `loaded ${loaded}, events ${after.join(",") || "none"}, back to the sign-in screen after ${back} press(es)`;
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

// The status bar icons follow the app theme (set_system_bars → media-session plugin), not only the system night
// mode: a dark app theme on a light system must not leave dark clock and battery icons on a dark page.
await step("9b. system bar icons follow the app theme", async () => {
  const page = await freshPage();
  await sleep(1500); // let the UI send its own value first
  const lightIcons = async () => {
    const dump = await sh("dumpsys window windows");
    const start = dump.search(new RegExp(`Window\\{[^}]*${PACKAGE.replace(/\./g, "\\.")}/[^}]*MainActivity\\}`));
    if (start < 0) return null;
    const next = dump.indexOf("Window #", start + 10);
    const block = dump.slice(start, next < 0 ? undefined : next);
    return /LIGHT_STATUS_BARS/.test(block);
  };
  await call(page, "set_system_bars", { dark: false });
  await sleep(800);
  const light = await lightIcons();
  await screencap("09b-system-bars-light-theme");
  if (light !== true) {
    warn(`dumpsys does not show LIGHT_STATUS_BARS for a light theme (${light}); the appearance check is skipped`);
    return "skipped: appearance not visible in dumpsys";
  }
  await call(page, "set_system_bars", { dark: true });
  await sleep(800);
  const afterDark = await lightIcons();
  await screencap("09b-system-bars-dark-theme");
  // Put back what the UI itself chose (no reload: a reload while native answers are in flight makes Tauri's IPC
  // report "runCallback" of undefined in the new page).
  const effectiveDark = await page.evaluate(() => {
    let theme = "system";
    try { theme = JSON.parse(localStorage.getItem("pm.settings.v1") || "{}").theme || "system"; } catch {}
    return theme === "dark" || (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  });
  await call(page, "set_system_bars", { dark: effectiveDark });
  if (afterDark !== false) throw new Error(`the status bar keeps dark icons after set_system_bars(dark) (LIGHT_STATUS_BARS: ${afterDark})`);
  await assertNoCrash("after switching the system bar appearance");
  return "light theme: dark icons; dark theme: light icons";
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

// --------------------------------------------------------------------- visual tour (mock data)
// After every real check: the same UI with the test backend from ui-tests, so each screen can be reviewed
// as the real Android WebView renders it (fonts, insets, scrolling). Screenshots land in $PM_OUT/tour-*.png.
// Navigation timing differs from Chromium, so a missed screen only warns; a crash still fails step 10's rules.
await step("11. visual tour: every phone screen with test data, captured from the device", async () => {
  const { everyPhoneScreen, press } = await import(path.join(root, "ui-tests/phone-screen-walk.js"));
  const mock = fs.readFileSync(path.join(root, "ui-tests/mock-backend.js"), "utf8");
  // The test backend must win over Tauri's own global, whichever script runs first.
  const pin = `(()=>{const m=window.__TAURI__;Object.defineProperty(window,"__TAURI__",{configurable:false,get:()=>m,set:()=>{}});})();`;
  const captured = [];
  // light and dark: the default design; then Material 3 and the classic look.
  for (const scheme of ["light", "dark", "material", "classic"]) try {
    const tour = await freshPage();
    await tour.addInitScript(`window.__mockConfig=${JSON.stringify({ signedIn: true, catalogMixes: true, history: [1, 2, 3], platform: "android" })};`);
    await tour.addInitScript(mock + "\n" + pin);
    // The walk finds controls by their Russian labels; the emulator's system language is English.
    await tour.addInitScript(`try{Object.defineProperty(Navigator.prototype,"language",{configurable:true,get:()=>"ru-RU"});Object.defineProperty(Navigator.prototype,"languages",{configurable:true,get:()=>["ru-RU","ru"]});}catch{}`);
    const theme = scheme === "dark" ? "dark" : "light";
    // The WebView keeps localStorage from the real checks above (the playback self-test leaves a "now playing"
    // snapshot), which the UI would restore over the test data: start each pass from an empty store, once.
    await tour.addInitScript(`try{if(sessionStorage.getItem("pm-tour")!=="${scheme}"){localStorage.clear();sessionStorage.setItem("pm-tour","${scheme}");}localStorage.setItem("pm.settings.v1",JSON.stringify({theme:"${theme}"}));localStorage.setItem("pm-style","${scheme === "material" ? "material" : scheme === "classic" ? "ios" : "premium"}")}catch{}`);
    await tour.reload();
    await tour.locator(".home-view .stage").first().waitFor({ timeout: 30_000 });
    let index = 0;
    const shoot = async (screen) => {
      await sleep(250);
      const file = await screencap(`tour-${scheme}-${String(++index).padStart(2, "0")}-${screen}`);
      if (file) captured.push(path.basename(file));
    };
    // Light and Material get the whole walk (Material is the style most worth checking on a real WebView).
    if (scheme === "light" || scheme === "material") {
      try { await everyPhoneScreen(tour, shoot); }
      catch (error) { warn(`${scheme} tour stopped after ${index} screens: ${error.message.split("\n")[0]}`); }
    } else {
      await shoot("home");
      for (const [tabId, name] of [["library", "library"], ["search", "search"], ["profile", "profile"]]) {
        try { await press(tour.locator(`.dock-tab[data-tab="${tabId}"]`).first()); await shoot(name); }
        catch (error) { warn(`dark tour: ${name}: ${error.message.split("\n")[0]}`); }
      }
    }
  } catch (error) { warn(`visual tour (${scheme}) unavailable: ${error.message.split("\n")[0]}`); }
  if (!(await pid())) throw new Error("the app is not running after the visual tour");
  await assertNoCrash("after the visual tour");
  return `${captured.length} screenshots`;
});

// A real finger on the real WebView: `input tap` and `input swipe` on the full-screen player's progress lane must
// seek (a phone showed taps and drags there doing nothing). Runs on the tour page, so the test backend records it.
await step("12. touch seeking in the full-screen player (adb input on the device)", async () => {
  const { press } = await import(path.join(root, "ui-tests/phone-screen-walk.js"));
  const p = await freshPage();
  await p.reload();
  // The tour ends on another tab, and the app reopens the last one: go Home first.
  await p.locator(".dock-tab").first().waitFor({ timeout: 30_000 });
  await press(p.locator('.dock-tab[data-tab="home"]').first());
  await p.locator(".home-view .stage").first().waitFor({ timeout: 30_000 });
  await press(p.locator(".stage-cta").first());
  await press(p.locator(".player .now").first());
  const lane = p.locator(".np .np-seek input");
  await lane.waitFor({ timeout: 15_000 });
  await sleep(800);
  const geo = await lane.evaluate((el) => { const r = el.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height, dpr: window.devicePixelRatio, max: Number(el.max) }; });
  const seeks = () => p.evaluate(() => window.__mock.calls.filter((c) => c.command === "seek").map((c) => c.args.seconds));
  const px = (v) => Math.round(v * geo.dpr);
  const y = px(geo.y + geo.h / 2);
  const before = (await seeks()).length;
  await sh(`input tap ${px(geo.x + geo.w * 0.75)} ${y}`);
  await waitFor("a seek after a tap", async () => (await seeks()).length > before, { timeout: 5_000 });
  const tapped = (await seeks()).at(-1);
  await sh(`input swipe ${px(geo.x + geo.w * 0.2)} ${y} ${px(geo.x + geo.w * 0.5)} ${y} 400`);
  await waitFor("a seek after a drag", async () => (await seeks()).length > before + 1, { timeout: 5_000 });
  const dragged = (await seeks()).at(-1);
  const near = (value, fraction) => Math.abs(value - geo.max * fraction) <= geo.max * 0.06;
  if (!near(tapped, 0.75) || !near(dragged, 0.5)) throw new Error(`seeks landed at ${tapped} and ${dragged} of ${geo.max} (expected about 75% and 50%)`);
  if (!(await pid())) throw new Error("the app is not running after the touch seek check");
  return `tap → ${tapped}s, drag → ${dragged}s of ${geo.max}s`;
});

// ------------------------------------------------------------------------------------------- the end

await disconnect();
try { fs.writeFileSync(path.join(OUT, "logcat-after-tests.txt"), await logcat()); }
catch (error) { fs.writeFileSync(path.join(OUT, "logcat-unavailable.txt"), String(error)); }
await screencap("99-final");
const ok = finish();
process.exit(ok ? 0 : 1);
