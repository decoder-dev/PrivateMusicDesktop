// The Android Back button. The shell (Kotlin) turns it into a cancelable
// `pm-back` CustomEvent on `window`; whoever handles it calls preventDefault().
// Unhandled, the app moves to the background, as Android expects at the root.
//
// Layers close from the top down: menus and sheets, dialogs, the full-screen
// player, then the navigation stack. Modules add theirs with `onBack(order, fn)`;
// a handler returns true when it used the press.

const handlers = [];

/** Lower `order` runs first. Returns a function that removes the handler. */
export function onBack(order, fn) {
  const entry = { order, fn };
  handlers.push(entry);
  handlers.sort((a, b) => a.order - b.order);
  return () => { const i = handlers.indexOf(entry); if (i >= 0) handlers.splice(i, 1); };
}

/** Runs the handlers; true when one of them used the press. */
export function handleBack() {
  for (const { fn } of [...handlers]) {
    try {
      if (fn()) return true;
    } catch {
      // A broken handler must not trap the user in the app.
    }
  }
  return false;
}

window.addEventListener("pm-back", (event) => {
  if (handleBack()) event.preventDefault();
});

// Open dialogs are everyone's business (the sign-out confirmation, the VPN
// how-to, forms), so they are closed here without each of them registering.
onBack(10, () => {
  const dialogs = [...document.querySelectorAll("dialog[open]")];
  const top = dialogs[dialogs.length - 1];
  if (!top) return false;
  top.dispatchEvent(new Event("cancel", { cancelable: true }));
  return true;
});
