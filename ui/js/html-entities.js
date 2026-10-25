const NAMED = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", hellip: "…", ndash: "–", mdash: "—",
  lsquo: "‘", rsquo: "’", ldquo: "“", rdquo: "”" };

/** Decodes HTML character references once (`&amp;lt;` → `&lt;`). Unknown references stay as written. */
export function decodeEntities(text) {
  return String(text ?? "").replace(/&(#[xX][0-9a-fA-F]{1,8}|#[0-9]{1,8}|[a-z]{2,6});/g, (whole, name) => {
    if (name[0] !== "#") return NAMED[name] ?? whole;
    const code = name[1] === "x" || name[1] === "X" ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
    if (!Number.isInteger(code) || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff) || (code < 32 && code !== 10 && code !== 9) || code === 127) return whole;
    return String.fromCodePoint(code);
  });
}
