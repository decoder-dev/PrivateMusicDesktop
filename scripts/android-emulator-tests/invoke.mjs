// Calls one Tauri command in the running app and prints the JSON result. For shell scripts.
//   node invoke.mjs <command> [json-args]       e.g.  node invoke.mjs debug_secure_selftest '{"mode":"keep"}'
//   node invoke.mjs --eval '<javascript expression>'   evaluates it in the app's page, e.g. localStorage access
// Starts the app first when it is not running. Exit code 1 prints the error on stderr.
import { PACKAGE, appPage, connect, disconnect, invoke, pid, startApp, waitFor } from "./lib.mjs";

const [command, rawArgs] = process.argv.slice(2);
if (!command) { console.error("usage: node invoke.mjs <command> [json-args] | --eval <js>"); process.exit(2); }
let code = 0;
try {
  if (!(await pid())) await startApp();
  await connect();
  const page = await appPage();
  await waitFor("document.readyState === complete", () => page.evaluate(() => document.readyState === "complete"), { timeout: 60_000 });
  const result = command === "--eval"
    ? await page.evaluate(rawArgs ?? "null") // a string: runs through CDP, which the page CSP does not restrict
    : await invoke(page, command, rawArgs ? JSON.parse(rawArgs) : {});
  console.log(JSON.stringify(result));
} catch (error) {
  console.error(`invoke ${command} on ${PACKAGE} failed: ${error.message}`);
  code = 1;
} finally {
  await disconnect();
}
process.exit(code);
