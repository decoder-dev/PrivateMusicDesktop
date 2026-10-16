import { api } from "../api.js";
import { errorText, locale, t } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { confirmDialog } from "../dialog.js";
import { artBox, watchSegmented, paintStatus, fillPanel } from "../ui-parts.js";

const SCOPES = ["month", "year", "all"];

/** "1 h 5 min" / "4 min 7 s": hours and minutes from an hour on, else minutes and seconds. */
export function formatListening(seconds) {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const parts = hours > 0
    ? [[hours, "h"], [Math.floor((total % 3600) / 60), "m"]]
    : [[Math.floor(total / 60), "m"], [total % 60, "s"]];
  const text = parts.filter(([value]) => value > 0).map(([value, unit]) => `${value} ${t(`stats.unit.${unit}`)}`).join(" ");
  return text || `0 ${t("stats.unit.s")}`;
}

const monthTitle = (key) => {
  const [year, month] = key.split("-").map(Number);
  return new Date(year, month - 1, 1).toLocaleDateString(locale, { month: "long", year: "numeric" });
};
// 2023-01-01 was a Sunday, and the engine counts weekdays from Sunday.
const weekdayName = (index) => new Date(2023, 0, 1 + index).toLocaleDateString(locale, { weekday: "long" });
const dayTitle = (key) => {
  const [year, month, day] = key.split("-").map(Number);
  return new Date(year, month - 1, day).toLocaleDateString(locale, { day: "numeric", month: "long" });
};
const hourText = (hour) => `${String(hour).padStart(2, "0")}:00`;

/** A thin capsule showing how this entry compares with the most played one. */
function shareBar(share) {
  const bar = h("span", { class: "stat-share", "aria-hidden": "true" }, h("i"));
  bar.firstChild.style.width = `${Math.max(4, Math.round(share * 100))}%`; // CSSOM: allowed under the CSP
  return bar;
}

function topList(title, entries, { artwork }) {
  const most = Math.max(...entries.map((entry) => entry.plays), 1);
  return h("section", { class: "set-section stat-section", "data-section": artwork ? "tracks" : "artists" },
    h("h2", { text: title }),
    h("ol", { class: "stat-list set-card" }, entries.map((entry, index) =>
      h("li", { class: "stat-row" },
        h("span", { class: "stat-rank", text: String(index + 1) }),
        artwork ? artBox(entry.artwork_url) : null,
        h("span", { class: "stat-titles" },
          h("span", { class: "stat-title", text: entry.title }),
          entry.subtitle ? h("span", { class: "stat-sub", text: entry.subtitle }) : null,
          shareBar(entry.plays / most)),
        h("span", { class: "stat-count", text: `×${entry.plays}` })))));
}

/**
 * Listening statistics kept on this computer: totals, top artists and tracks
 * and when the listening happens, for a month, a year or all time.
 */
export function createStatsView({ toast }) {
  let scope = "month";
  const chosen = { month: null, year: null };
  let generation = 0;

  const tabs = h("div", { class: "tabs", role: "tablist", "aria-label": t("stats.period") },
    SCOPES.map((id) => h("button", { type: "button", role: "tab", "data-scope": id, onclick: () => select(id), text: t(`stats.${id}`) })));
  watchSegmented(tabs);
  const chooser = h("select", { class: "field", "aria-label": t("stats.period"), onchange: () => { chosen[scope] = chooser.value; load(); } });
  const periodTitle = h("div", { class: "stat-period" });
  const body = h("div", { class: "stat-body" });
  const note = h("div", { class: "page-note", text: t("stats.privacy") });
  const resetButton = h("button", { class: "btn danger-text", type: "button", onclick: () => reset(), text: t("stats.reset"), hidden: true });
  const element = h("section", { class: "view stats-view" },
    h("header", { class: "view-header" }, h("div", { class: "vh-text" }, h("h1", { text: t("stats.title") }))),
    h("div", { class: "page-scroll" },
      h("div", { class: "stat-controls" }, tabs, chooser, periodTitle),
      body,
      h("div", { class: "stat-foot" }, note, resetButton)));

  function select(id) {
    scope = id;
    for (const button of tabs.querySelectorAll("button")) button.setAttribute("aria-selected", String(button.dataset.scope === id));
    load();
  }

  function renderBody(view) {
    const { summary } = view;
    if (summary.is_empty) {
      const empty = h("div", { class: "empty-state stat-empty" });
      fillPanel(empty, "", "chart");
      empty.append(h("strong", { text: t("stats.emptyTitle") }), h("div", { text: t("stats.emptyDetail") }));
      return [empty];
    }
    const peak = Math.max(...summary.hour_seconds, 1);
    const habits = [
      summary.peak_hour !== null ? h("div", { class: "stat-fact", "data-fact": "hour", text: t("stats.peakHour", { hour: hourText(summary.peak_hour) }) }) : null,
      summary.favourite_weekday !== null ? h("div", { class: "stat-fact", "data-fact": "weekday", text: t("stats.favoriteWeekday", { day: weekdayName(summary.favourite_weekday) }) }) : null,
      summary.busiest_day ? h("div", { class: "stat-fact", "data-fact": "day", text: t("stats.busiestDay", { day: dayTitle(summary.busiest_day.key) }) }) : null,
    ];
    return [
      h("div", { class: "stat-totals" },
        h("div", { class: "stat-hero" },
          h("div", { class: "stat-label", text: t("stats.totalTime") }),
          h("div", { class: "stat-big", "data-stat": "time", text: formatListening(summary.total_seconds) })),
        h("div", { class: "stat-figure" },
          h("div", { class: "stat-medium", "data-stat": "plays", text: summary.plays.toLocaleString(locale) }),
          h("div", { class: "stat-label", text: t("stats.plays") })),
        h("div", { class: "stat-figure" },
          h("div", { class: "stat-medium", "data-stat": "days", text: summary.active_days.toLocaleString(locale) }),
          h("div", { class: "stat-label", text: t("stats.activeDays") }))),
      summary.top_artists.length ? topList(t("stats.topArtists"), summary.top_artists, { artwork: false }) : null,
      summary.top_tracks.length ? topList(t("stats.topTracks"), summary.top_tracks, { artwork: true }) : null,
      h("section", { class: "set-section stat-section", "data-section": "when" },
        h("h2", { text: t("stats.when") }),
        h("div", { class: "set-card stat-when" },
        h("div", { class: "hour-chart", role: "img", "aria-label": t("stats.chartLabel") },
          summary.hour_seconds.map((seconds, hour) => {
            const bar = h("span", { class: `hour-bar${hour === summary.peak_hour ? " peak" : ""}`, "data-hour": String(hour),
              title: `${hourText(hour)} · ${formatListening(seconds)}` });
            bar.style.height = `${Math.max(3, Math.round(64 * seconds / peak))}px`; // CSSOM: the CSP forbids style attributes
            return bar;
          })),
        h("div", { class: "hour-axis", "aria-hidden": "true" }, [0, 6, 12, 18].map((hour) => h("span", { text: String(hour).padStart(2, "0") }))),
        ...habits)),
    ];
  }

  function renderControls(view) {
    for (const button of tabs.querySelectorAll("button")) button.setAttribute("aria-selected", String(button.dataset.scope === scope));
    const period = view.summary.period;
    const options = scope === "month" ? view.months : scope === "year" ? view.years.map(String) : [];
    const current = scope === "month" ? period.key : scope === "year" ? String(period.year) : "";
    // Several periods with data: pick one. One or none: just say which.
    const pickable = options.length > 1;
    chooser.hidden = !pickable;
    periodTitle.hidden = pickable || scope === "all";
    if (pickable) {
      chooser.replaceChildren(...options.map((value) => h("option", { value, text: scope === "month" ? monthTitle(value) : value })));
      chooser.value = current;
    }
    periodTitle.textContent = scope === "month" ? monthTitle(current) : scope === "year" ? current : "";
  }

  async function load() {
    const mine = ++generation;
    try {
      const view = await api.listeningStats(scope, chosen[scope]);
      if (mine !== generation) return;
      renderControls(view);
      body.replaceChildren(...renderBody(view).filter(Boolean));
      resetButton.hidden = view.months.length === 0;
    } catch (error) {
      if (mine !== generation) return;
      // A remembered month that no longer exists falls back to the newest.
      chosen[scope] = null;
      body.replaceChildren(h("div", { class: "status error", role: "status", text: errorText(error) }));
    }
  }

  async function reset() {
    if (!(await confirmDialog({ text: t("stats.resetConfirm"), confirm: t("stats.reset"), danger: true }))) return;
    try {
      await api.resetListeningStats();
      chosen.month = chosen.year = null;
      toast(t("stats.resetDone"));
      load();
    } catch (error) {
      toast(errorText(error), { error: true });
    }
  }

  return {
    element,
    show() { load(); },
    reset() {
      generation++;
      chosen.month = chosen.year = null;
      body.replaceChildren();
      resetButton.hidden = true;
    },
  };
}
