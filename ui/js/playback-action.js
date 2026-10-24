import { toast } from "./toast.js";
import { errorText } from "./i18n.js";

/** Event handlers cannot await playback failures: report them without an unhandled rejection. */
export async function playbackAction(run) {
  try { return await run(); }
  catch (error) { toast(errorText(error), { error: true }); }
}
