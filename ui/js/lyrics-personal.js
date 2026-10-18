import { ft } from "./feature-i18n.js";
import { trackKey } from "./api.js";
import { readPersonal, writePersonal } from "./personal.js";
export function parseLocalLrc(raw) {
  if (typeof raw !== "string" || raw.length > 256 * 1024)
    throw new Error(ft("LRC должен быть не больше 256 КБ"));
  let offset = 0;
  const lines = [];
  const match = raw.match(/\[offset:([+-]?\d+)\]/i);
  if (match) offset = Number(match[1]) / 1000;
  for (const source of raw.split(/\r?\n/)) {
    const stamps = [
      ...source.matchAll(/\[(\d{1,3}):([0-5]\d)(?:[.:](\d{1,3}))?\]/g),
    ];
    const text = source.replace(/\[[^\]]*\]/g, "").trim();
    for (const stamp of stamps) {
      const time =
        Number(stamp[1]) * 60 +
        Number(stamp[2]) +
        Number(`0.${stamp[3] || 0}`) +
        offset;
      if (Number.isFinite(time) && time >= 0) lines.push({ time, text });
    }
    if (lines.length > 10000) throw new Error(ft("Слишком много строк LRC"));
  }
  if (!lines.length)
    throw new Error(ft("В файле нет строк с временными отметками LRC"));
  lines.sort((a, b) => a.time - b.time);
  return {
    source: ft("Личный LRC"),
    text: lines.map((line) => line.text).join("\n"),
    lines,
  };
}
export const personalLyrics = (track) =>
  readPersonal(`lyrics.${trackKey(track)}`, {});
export function savePersonalLyrics(track, value) {
  writePersonal(`lyrics.${trackKey(track)}`, value);
}
