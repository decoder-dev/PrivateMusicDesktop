import { toast } from "./toast.js";
import { errorText } from "./i18n.js";

/** Event handlers cannot await playback failures: report them without an unhandled rejection. */
const pendingButtons = new WeakMap();
export async function playbackAction(run, button) {
  if (button) {
    pendingButtons.set(button, (pendingButtons.get(button) ?? 0) + 1);
    button.classList.add("is-requesting");
    button.setAttribute("aria-busy", "true");
  }
  try { return await run(); }
  catch (error) { toast(errorText(error), { error: true }); }
  finally {
    if (button) {
      const pending = (pendingButtons.get(button) ?? 1) - 1;
      if (pending) pendingButtons.set(button, pending);
      else {
        pendingButtons.delete(button);
        button.classList.remove("is-requesting");
        button.removeAttribute("aria-busy");
      }
    }
  }
}
