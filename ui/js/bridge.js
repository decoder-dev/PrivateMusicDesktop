// The only file that talks to Tauri. Everything else uses these functions,
// which keeps the UI testable against a fake backend.

const tauri = () => {
  const t = window.__TAURI__;
  if (!t) throw new Error("Tauri API is not available");
  return t;
};

export const invoke = (command, args) => tauri().core.invoke(command, args);

/** Subscribes to a backend event; resolves to an unsubscribe function. */
export const listen = (event, handler) => tauri().event.listen(event, (e) => handler(e.payload));
