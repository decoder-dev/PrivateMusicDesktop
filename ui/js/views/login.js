import { h } from "../dom.js";
import { t, errorText } from "../i18n.js";
import { api } from "../api.js";
import { icon } from "../icons.js";
import { isNetworkError, vpnHint } from "../network.js";

/** Экран входа в систему. Собственная страница VK открывается в отдельном окне. */
export function createLoginView({ onError }) {
  const message = h("p", { class: "message", role: "status" });
  const button = h("button", { class: "btn primary", type: "button", onclick: () => start() }, t("login.button"));
  const restart = h("button", { class: "login-restart", type: "button", onclick: () => start(true) }, t("login.restart"));
  const recovery = h("div", { class: "login-recovery", hidden: true }, restart,
    h("p", { text: t("login.restartHint") }));
  const note = h("p", { class: "note", hidden: true, role: "note" }, icon("alert", 16), h("span", { text: t("login.notPersistent") }));
  // Подсказка VPN для страницы входа, которая не загружается, или для невыполненного запроса токена.
  const hint = h("div", { class: "net-hint-slot", hidden: true });
  let waiting = false;

  function showHint(show) {
    hint.hidden = !show;
    hint.replaceChildren(...(show ? [vpnHint({ onRetry: () => { showHint(false); start(); } })] : []));
  }

  async function start(reset = false) {
    if (button.disabled) return;
    button.disabled = true;
    restart.disabled = true;
    if (!waiting || reset) message.textContent = t("login.waiting");
    if (reset) showHint(false);
    try {
      await api.loginStart(reset);
      waiting = true;
      recovery.hidden = false;
      button.textContent = t("login.return");
    } catch (error) {
      button.disabled = false;
      restart.disabled = false;
      message.textContent = "";
      onError(error);
    } finally {
      // Открытие окна завершено; держите способ вернуть его на передний план.
      button.disabled = false;
      restart.disabled = false;
    }
  }

  const element = h("div", { class: "login" },
    h("div", { class: "box" },
      h("div", { class: "login-art", "aria-hidden": "true" },
        h("i", { class: "tile a" }, icon("artist", 26)), h("i", { class: "tile b" }, icon("album", 26)),
        h("div", { class: "app-icon" }, icon("note", 40))),
      h("h1", { text: t("login.title") }),
      h("p", { text: t("login.lead") }),
      button, message, hint, recovery, note));

  return {
    element,
    /** Вызов с помощью "success" | "cancelled" | "error:<kind>" из бэкэнда. */
    loginResult(result) {
      button.disabled = false;
      showHint(result === "loadfailed" || (result.startsWith("error:") && isNetworkError({ kind: result.slice(6) })));
      if (result === "loadfailed") { message.textContent = ""; return; }
      if (result === "loadok") return;
      if (result.startsWith("pending:")) {
        message.textContent = t("login.confirm");
        return;
      }
      waiting = false;
      button.textContent = t("login.button");
      if (result === "cancelled") message.textContent = t("login.cancelled");
      else if (result.startsWith("error:")) {
        const kind = result.slice(6);
        message.textContent = ["offline", "timeout", "network"].includes(kind) ? errorText({ kind }) : t("login.failed");
      }
      else message.textContent = "";
    },
    showPersistenceWarning(show) { note.hidden = !show; },
    reset() { waiting = false; button.disabled = false; restart.disabled = false; recovery.hidden = true; button.textContent = t("login.button"); message.textContent = ""; showHint(false); },
  };
}
