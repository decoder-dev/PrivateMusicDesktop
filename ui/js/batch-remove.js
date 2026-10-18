import { ft } from "./feature-i18n.js";
import { confirmDialog } from "./dialog.js";
import { accountId } from "./personal.js";
export async function removeSelected(
  tracks,
  remove,
  label = ft("Удалить выбранные треки?"),
) {
  const id = accountId();
  if (
    !(await confirmDialog({
      text: `${label} (${tracks.length})`,
      confirm: ft("Удалить"),
      danger: true,
    }))
  )
    return;
  for (const track of tracks) {
    if (id !== accountId()) break;
    await remove(track);
  }
}
