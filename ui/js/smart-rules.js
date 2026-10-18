import { ft } from "./feature-i18n.js";
import { trackKey } from "./api.js";

export const DEFAULT_RULES = [
  {
    id: "forgotten",
    title: ft("Давно не слушал"),
    kind: "forgotten",
    days: 30,
    limit: 200,
  },
  { id: "recent", title: ft("Недавно добавлено"), kind: "recent", limit: 100 },
  { id: "month", title: ft("Любимые за месяц"), kind: "month", limit: 100 },
  { id: "offline", title: ft("Скачанное избранное"), kind: "offline", limit: 1000 },
];
export function validRule(rule) {
  return (
    rule &&
    typeof rule.id === "string" &&
    typeof rule.title === "string" &&
    rule.title.trim().length > 0 &&
    rule.title.length <= 128 &&
    ["forgotten", "recent", "month", "offline"].includes(rule.kind) &&
    Number.isInteger(rule.limit) &&
    rule.limit >= 1 &&
    rule.limit <= 5000 &&
    (rule.kind !== "forgotten" ||
      (Number.isInteger(rule.days) && rule.days >= 1 && rule.days <= 3650))
  );
}
export function applySmartRule(
  rule,
  { library, history, downloaded, plays = {}, now = Date.now() },
) {
  if (!validRule(rule)) return [];
  const last = new Map(
    history.map((entry) => [trackKey(entry.track), entry.played_at * 1000]),
  );
  const offline = new Set(downloaded.map(trackKey));
  const date = new Date(now);
  const month = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
  const count = (track) => Number.isFinite(plays[trackKey(track)])?plays[trackKey(track)]:plays[trackKey(track)]?.[month] || 0;
  const unique = [
    ...new Map(library.map((track) => [trackKey(track), track])).values(),
  ];
  const items =
    rule.kind === "offline"
      ? unique.filter((track) => offline.has(trackKey(track)))
      : rule.kind === "month"
        ? unique
            .filter((track) => count(track) > 0)
            .sort((a, b) => count(b) - count(a))
        : rule.kind === "forgotten"
          ? unique
              .filter(
                (track) =>
                  (last.get(trackKey(track)) || 0) < now - rule.days * 86400000,
              )
              .sort(
                (a, b) =>
                  (last.get(trackKey(a)) || 0) - (last.get(trackKey(b)) || 0),
              )
          : unique;
  return items.slice(0, rule.limit);
}
