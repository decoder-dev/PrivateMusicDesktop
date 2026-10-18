import { ft } from "../feature-i18n.js";
import { h } from "../dom.js";
import { api, loadAllPages } from "../api.js";
import { createTracksScreen } from "../tracks-screen.js";
import { DEFAULT_RULES, validRule, applySmartRule } from "../smart-rules.js";
import { readPersonal, writePersonal, accountId } from "../personal.js";
import { formDialog, confirmDialog } from "../dialog.js";
import { errorText, t, addStrings } from "../i18n.js";
import { subscribe } from "../store.js";

addStrings({
  ru: { "nav.smart": ft("Умные плейлисты") },
  en: { "nav.smart": "Smart playlists" },
});
export function createSmartView({ play, onTrackAction, toast }) {
  let rules = [],
    current = null,
    generation = 0;
  const kinds = [
    ["forgotten", ft("Давно не слушал")],
    ["recent", ft("Недавно добавлено")],
    ["month", ft("Любимые за месяц")],
    ["offline", ft("Скачанное избранное")],
  ];
  const select = h("select", {
    class: "field",
    "aria-label": ft("Умный плейлист"),
    onchange: () => {
      current = rules.find((r) => r.id === select.value);
      load();
    },
  });
  const edit = h("button", {
    class: "btn",
    type: "button",
    text: ft("Настроить"),
    onclick: () => editor(current),
  });
  const create = h("button", {
    class: "btn",
    type: "button",
    text: ft("Создать"),
    onclick: () => editor(null),
  });
  const remove = h("button", {
    class: "btn",
    type: "button",
    text: ft("Удалить правило"),
    onclick: async () => {
      if (
        current &&
        (await confirmDialog({
          text: ft("Удалить правило «{title}»?",{title:current.title}),
          confirm: ft("Удалить"),
        }))
      ) {
        rules = rules.filter((r) => r.id !== current.id);
        writePersonal("smartRules", rules);
        render();
        load();
      }
    },
  });
  const screen = createTracksScreen({
    title: ft("Умные плейлисты"),
    filterable: true,
    hooks: {
      emptyText: ft("Нет треков по этому правилу"),
      art: "playlist",
      onPlay: play,
      onAction: onTrackAction,
      onRetry: load,
      headerExtra: h(
        "div",
        { class: "set-inline smart-tools" },
        select,
        edit,
        create,
        remove,
      ),
    },
  });
  const note = h("div", {
    class: "page-note footer-note",
    text: ft("Правила пересчитываются при открытии. Рейтинг использует локальную статистику прослушиваний, а «Давно не слушал» — сохранённую историю этого компьютера. «Недавно добавлено» использует порядок библиотеки VK."),
  });
  screen.header.after(note);
  screen.element.classList.add("smart-view");
  function render() {
    rules = readPersonal("smartRules", DEFAULT_RULES).filter(validRule);
    current = rules.find((r) => r.id === current?.id) || rules[0] || null;
    select.replaceChildren(
      ...rules.map((rule) => h("option", { value: rule.id, text: rule.title })),
    );
    select.value = current?.id || "";
    edit.disabled = remove.disabled = !current;
  }
  async function load() {
    const mine = ++generation,
      id = accountId();
    screen.setTracks([]);
    if (!current) {
      screen.clearStatus();
      return;
    }
    screen.setStatus(t("loading"));
    const rule = { ...current };
    try {
      const [library, history, downloaded,plays] = await Promise.all([
        loadAllPages((offset, count) => api.library(offset, count), {
          isCancelled: () => mine !== generation || id !== accountId(),
        }),
        api.history(),
        api.downloads(),
        api.monthPlays(),
      ]);
      if (mine !== generation || id !== accountId() || !library) return;
      screen.setTitle(rule.title);
      screen.setTracks(
        applySmartRule(rule, {
          library,
          history,
          downloaded: downloaded.tracks,
          plays,
        }),
      );
      screen.clearStatus();
    } catch (error) {
      if (mine === generation)
        screen.setStatus(errorText(error), { error: true });
    }
  }
  async function editor(rule) {
    const id=accountId();
    const values = await formDialog({
      title: rule ? ft("Настройка правила") : ft("Новый умный плейлист"),
      confirm: ft("Сохранить"),
      fields: [
        {
          name: "title",
          label: ft("Название"),
          required: true,
          value: rule?.title,
          max: 128,
        },
        {
          name: "kind",
          label: ft("Правило"),
          options: kinds,
          value: rule?.kind || "forgotten",
        },
        {
          name: "days",
          label: ft("Дней без прослушиваний"),
          type: "number",
          min: 1,
          maxValue: 3650,
          value: rule?.days || 30,
          required: true,
        },
        {
          name: "limit",
          label: ft("Максимум треков"),
          type: "number",
          min: 1,
          maxValue: 5000,
          value: rule?.limit || 200,
          required: true,
        },
      ],
    });
    if (!values || id!==accountId()) return;
    const next = {
      ...values,
      id: rule?.id || crypto.randomUUID(),
      days: Number(values.days),
      limit: Number(values.limit),
    };
    if (!validRule(next)) return;
    try {
      rules = rule
        ? rules.map((r) => (r.id === rule.id ? next : r))
        : [...rules, next];
      writePersonal("smartRules", rules);
      current = next;
      render();
      load();
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }
  subscribe("session", () => {
    generation++;
  });
  return {
    element: screen.element,
    show() {
      render();
      load();
    },
    reset() {
      generation++;
      current = null;
      screen.setTracks([]);
    },
  };
}
