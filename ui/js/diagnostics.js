// «Журнал для отчета об ошибке»: устройство, версия WebView, причины завершения процесса и хвосты журнала.
// Ничего не отправляется: текст показан в окне, его можно скопировать и прислать разработчику.
import { h } from "./dom.js";
import { t, addStrings } from "./i18n.js";
import { api } from "./api.js";

addStrings({
  ru: { "diag.title": "Журнал для отчета", "diag.note": "Если приложение закрывается само, откройте это окно после следующего запуска, скопируйте текст и пришлите его.", "diag.open": "Показать журнал", "diag.copy": "Скопировать", "diag.copied": "Скопировано", "diag.close": "Закрыть", "diag.loading": "Собираю сведения…", "diag.failed": "Не удалось собрать журнал." },
  en: { "diag.title": "Report log", "diag.note": "If the app closes by itself, open this window after the next start, copy the text and send it.", "diag.open": "Show log", "diag.copy": "Copy", "diag.copied": "Copied", "diag.close": "Close", "diag.loading": "Collecting…", "diag.failed": "Could not collect the log." },
});

export function openDiagnostics() {
  const text = h("textarea", { class: "diag-text field", readonly: "", spellcheck: "false", "aria-label": t("diag.title"), value: t("diag.loading") });
  const copy = h("button", { class: "btn primary", type: "button", text: t("diag.copy") });
  const close = h("button", { class: "btn", type: "button", text: t("diag.close") });
  const dialog = h("dialog", { class: "dialog diag-dialog", "aria-label": t("diag.title") },
    h("h2", { text: t("diag.title") }), h("p", { class: "dialog-text", text: t("diag.note") }), text,
    h("div", { class: "dialog-actions" }, close, copy));
  const finish = () => { dialog.close(); dialog.remove(); document.body.classList.remove("overlay-open"); };
  close.addEventListener("click", finish);
  dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(); });
  copy.addEventListener("click", () => {
    text.focus(); text.select();
    let ok = false;
    try { ok = document.execCommand("copy"); } catch { /* ниже запасной путь */ }
    if (!ok && navigator.clipboard) navigator.clipboard.writeText(text.value).catch(() => {});
    copy.textContent = t("diag.copied");
    setTimeout(() => { copy.textContent = t("diag.copy"); }, 1600);
  });
  document.body.classList.add("overlay-open");
  document.body.append(dialog);
  dialog.showModal();
  api.crashReport().then((report) => { text.value = report || t("diag.failed"); }).catch(() => { text.value = t("diag.failed"); });
}
