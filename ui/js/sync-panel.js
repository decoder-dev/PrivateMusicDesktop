import { ft } from "./feature-i18n.js";
import { h } from "./dom.js";
import {
  syncRules,
  syncRunning,
  syncPlaylists,
  setPlaylistSync,
} from "./playlist-sync.js";
import { readPersonal, writePersonal } from "./personal.js";
import { subscribe } from "./store.js";
import { toast } from "./toast.js";
import { errorText } from "./i18n.js";
export function createSyncPanel() {
  const rows = h("div", { class: "sync-rows" });
  const run = h("button", {
    class: "btn",
    type: "button",
    text: ft("Синхронизировать"),
    onclick: () =>
      syncPlaylists().catch((error) =>
        toast(errorText(error), { error: true }),
      ),
  });
  const remove = h("input", {
    type: "checkbox",
    "aria-label": ft("Удалять устаревшие файлы синхронизации"),
    onchange: () => {
      try {
        writePersonal("syncRemoveObsolete", remove.checked);
      } catch (error) {
        toast(errorText(error), { error: true });
      }
    },
  });
  const element = h(
    "section",
    { class: "sync-panel" },
    h(
      "div",
      { class: "set-inline" },
      h("strong", { text: ft("Плейлисты офлайн") }),
      run,
    ),
    h(
      "label",
      { class: "set-inline" },
      remove,
      ft("Удалять ушедшие из подборки файлы, скачанные синхронизацией"),
    ),
    h("p", {
      class: "set-note",
      text: ft("Проверка каждые 15 минут, пока приложение работает, и после восстановления сети. Скачанные вручную треки сохраняются."),
    }),
    rows,
  );
  function render() {
    const rules = syncRules();
    run.disabled = syncRunning() || !rules.length;
    remove.checked = readPersonal("syncRemoveObsolete", false);
    rows.replaceChildren(
      ...rules.map((rule) =>
        h(
          "div",
          { class: "sync-row" },
          h(
            "div",
            {},
            h("strong", { text: rule.playlist.title }),
            h("div", {
              class: "set-note",
              text:
                rule.error ||
                (rule.last
                  ? ft("Проверено {date}",{date:new Date(rule.last).toLocaleString()})
                  : ft("Ожидает синхронизации")),
            }),
          ),
          h("button", {
            class: "btn",
            type: "button",
            text: ft("Отключить"),
            onclick: () => {
              try {
                setPlaylistSync(rule.playlist, false);
                render();
              } catch (error) {
                toast(errorText(error), { error: true });
              }
            },
          }),
        ),
      ),
    );
  }
  subscribe("playlistSync", render);
  subscribe("session", render);
  render();
  return element;
}
