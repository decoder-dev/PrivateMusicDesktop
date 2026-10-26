/** Render device-controlled text as a single inert Markdown table cell. */
export function markdownCell(value) {
  const text = String(value ?? "").replace(/[\r\n]/g, " ");
  return Array.from(text).slice(0, 300).join("")
    .replace(/[&<>\\|[\]`!*_~]/g, character => `&#${character.codePointAt(0)};`);
}
