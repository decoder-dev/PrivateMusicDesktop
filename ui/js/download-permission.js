import { api } from "./api.js";
import { isAndroid } from "./mobile/layout.js";
import { accountId } from "./personal.js";
import { store } from "./store.js";
import { confirmDialog } from "./dialog.js";
import { t } from "./i18n.js";
import "./mobile/strings.js";

// Consent belongs to this download action only. Recheck cost between tracks so a
// batch started on Wi-Fi cannot silently continue on a newly metered connection.
export function downloadPermission({ interactive = true, isCancelled = () => false } = {}) {
  const account = accountId();
  let consent = false;
  const current = () => account && account === accountId() && store.session.signed_in && !isCancelled();
  return async () => {
    if (!current()) return false;
    if (!isAndroid()) return true;
    let network;
    try {
      network = await api.networkUsage();
      if (typeof network?.connected !== "boolean" || typeof network?.metered !== "boolean") network = null;
    } catch { network = null; }
    if (!current()) return false;
    if (network?.connected === false) throw { kind: "offline", message: "No active network" };
    if (network?.metered === false) return true;
    if (!interactive) return false; // Automatic playlist sync waits for an unmetered network.
    if (consent) return true;
    consent = await confirmDialog({text:t(network ? "download.metered" : "download.unknownNetwork"), confirm:t("download.continue")});
    return consent && current();
  };
}
