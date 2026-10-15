import { h } from "../dom.js";
import { t } from "../i18n.js";
import { api } from "../api.js";

/** The sign-in screen. VK's own page opens in a separate window. */
export function createLoginView({ onError }) {
  const message = h("p", { class: "message", role: "status" });
  const button = h("button", { class: "btn primary", type: "button", onclick: start }, t("login.button"));
  const note = h("p", { class: "note", hidden: true, text: t("login.notPersistent") });

  async function start() {
    button.disabled = true;
    message.textContent = t("login.waiting");
    try {
      await api.loginStart();
    } catch (error) {
      button.disabled = false;
      message.textContent = "";
      onError(error);
    }
  }

  const element = h("div", { class: "login" },
    h("div", { class: "box" },
      h("h1", { text: t("login.title") }),
      h("p", { text: t("login.lead") }),
      button, message, note));

  return {
    element,
    /** Called with "success" | "cancelled" | "error:<kind>" from the backend. */
    loginResult(result) {
      button.disabled = false;
      if (result === "cancelled") message.textContent = t("login.cancelled");
      else if (result.startsWith("error")) message.textContent = t("login.failed");
      else message.textContent = "";
    },
    showPersistenceWarning(show) { note.hidden = !show; },
    reset() { button.disabled = false; message.textContent = ""; },
  };
}
