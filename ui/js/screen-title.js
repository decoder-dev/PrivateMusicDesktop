import { api } from "./api.js";
import { t } from "./i18n.js";
import { isAndroid } from "./mobile/layout.js";

const FALLBACK = {home:"tab.home", libraryHub:"tab.library", profile:"tab.profile"};
export function screenTitle(element, id) {
  const heading = [...(element?.querySelectorAll("h1") ?? [])].find(node => node.offsetParent && node.textContent.trim());
  const raw = heading?.textContent || (FALLBACK[id] ? t(FALLBACK[id]) : "");
  return raw.replace(/[\u0000-\u001f\u007f]/g, " ").replace(/\s+/gu," ").trim();
}

let wanted = "";
let pending = Promise.resolve();
function publishTitle(label) {
  const brand = t("app.name");
  const title = label ? `${Array.from(label).slice(0,160).join("")} — ${brand}` : brand;
  if (wanted === title) return;
  wanted = title;
  document.title = title;
  // Native requests stay ordered, including a sign-out during an earlier request.
  if (!isAndroid()) pending = pending.then(() => wanted === title ? api.setWindowTitle(title) : undefined).catch(() => {});
}

/** Observe heading changes, not playback clocks or the contents of every row. */
export function installScreenTitles(main) {
  let frame = 0;
  let disposed = false;
  let active = null;
  const refresh = () => {
    frame = 0;
    if (disposed) return;
    const label = screenTitle(active, main.dataset.view);
    publishTitle(label);
    main.dispatchEvent(new Event("pm-screen-title"));
  };
  const schedule = () => { if (!disposed && !frame) frame = requestAnimationFrame(refresh); };
  const affectsTitle = record => {
    if (record.target.nodeType === Node.TEXT_NODE) return record.target.parentElement?.closest("h1");
    if (record.target.closest?.("h1")) return true;
    if (record.type === "attributes") return record.target.matches("h1") || record.target.querySelector("h1");
    return [...record.addedNodes,...record.removedNodes].some(node => node.nodeType === Node.ELEMENT_NODE && (node.matches("h1") || node.querySelector("h1")));
  };
  const headings = new MutationObserver(records => { if (records.some(affectsTitle)) schedule(); });
  const select = () => {
    headings.disconnect();
    active = [...main.children].find(node => node.classList.contains("view") && !node.hidden);
    if (active) headings.observe(active,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:["hidden"]});
    schedule();
  };
  const route = new MutationObserver(select);
  route.observe(main,{attributes:true,attributeFilter:["data-view"]});
  select();
  return () => {
    disposed = true;
    cancelAnimationFrame(frame);
    headings.disconnect();route.disconnect();
    publishTitle("");
  };
}
