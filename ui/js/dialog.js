import { h } from "./dom.js";
import { t } from "./i18n.js";
import { afterAnimation } from "./motion.js";

/** Открывает диалоговое окно модально и возвращает `close(value)`, который сначала затухает. */
function present(dialog, resolve) {
  let done = false;
  document.body.classList.add("overlay-open");
  document.body.append(dialog);
  dialog.showModal();
  return function close(value) {
    if (done) return;
    done = true;
    dialog.classList.add("closing");
    afterAnimation(dialog, () => {
      dialog.close();
      dialog.remove();
      if (!document.querySelector("dialog.dialog")) document.body.classList.remove("overlay-open");
      resolve(value);
    }, 300);
  };
}

/** Модальная форма. Возвращает значения полей или нулевое значение при отмене. */
export function formDialog({ title, fields, confirm }) {
  return new Promise((resolve) => {
    const inputs = fields.map((field) => {
      const attrs = { class: "field", name: field.name, "aria-label": field.label, maxlength: String(field.max ?? 128) };
      const input = field.options ? h("select",attrs,field.options.map(([value,label])=>h("option",{value,text:label}))) : field.multiline ? h("textarea", { ...attrs, rows: "3" }) : h("input", { ...attrs, type: field.type ?? "text", ...(field.min != null ? {min:String(field.min)} : {}), ...(field.maxValue != null ? {max:String(field.maxValue)} : {}) });
      input.value = field.value ?? "";
      return { field, input };
    });
    const submit = h("button", { class: "btn primary", type: "submit", text: confirm });
    const form = h("form", { class: "dialog-form" },
      h("h2", { text: title }),
      inputs.map(({ field, input }) => h("label", { class: "dialog-field" }, h("span", { text: field.label }), input)),
      h("div", { class: "dialog-actions" },
        h("button", { class: "btn", type: "button", onclick: () => finish(null), text: t("pl.cancel") }),
        submit));
    const dialog = h("dialog", { class: "dialog", "aria-label": title }, form);

    function valid() {
      return inputs.every(({ field, input }) => (!field.required || input.value.trim()) && input.checkValidity());
    }
    function refresh() { submit.disabled = !valid(); }
    inputs.forEach(({ input }) => input.addEventListener("input", refresh));
    refresh();

    const finish = present(dialog, resolve);
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!valid()) return;
      finish(Object.fromEntries(inputs.map(({ field, input }) => [field.name, input.value.trim()])));
    });
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(null); });
    inputs[0]?.input.focus();
  });
}

/** Yes/no. При подтверждении принимает значение true. */
export function confirmDialog({ text, confirm, danger = false }) {
  return new Promise((resolve) => {
    const dialog = h("dialog", { class: "dialog", "aria-label": text },
      h("p", { class: "dialog-text", text }),
      h("div", { class: "dialog-actions" },
        h("button", { class: "btn", type: "button", onclick: () => finish(false), text: t("pl.cancel") }),
        h("button", { class: `btn ${danger ? "danger" : "primary"}`, type: "button", onclick: () => finish(true), text: confirm })));
    const finish = present(dialog, resolve);
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(false); });
    dialog.querySelector(".btn:not(.primary):not(.danger)")?.focus();
  });
}
