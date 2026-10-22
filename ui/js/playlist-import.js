// Import text is data. Parse exports locally, then search VK before any playlist is created.
export const IMPORT_LIMIT = 500;
const clean = value => String(value ?? "").replace(/\s+/g, " ").trim().replace(/^["'“«](.*)["'”»]$/u, "$1").trim();
const key = value => clean(value).toLocaleLowerCase().replace(/ё/g, "е").normalize("NFKD").replace(/\p{M}/gu, "").replace(/[^\p{L}\p{N}]+/gu, " ").trim();
const titleHeaders = new Set(["track name", "trackname", "track", "title", "song", "song name", "name", "название", "трек", "песня", "название трека"]);
const artistHeaders = new Set(["artist name(s)", "artist names", "artist name", "artist", "artists", "performer", "исполнитель", "исполнители", "артист", "артисты"]);

function csv(text, delimiter) {
  const rows = []; let row = [], field = "", quoted = false;
  const finish = () => { row.push(field); field = ""; rows.push(row); row = []; };
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"' && text[i + 1] === '"') { field += '"'; i++; }
      else if (ch === '"') quoted = false;
      else field += ch;
    } else if (ch === '"' && !field) quoted = true;
    else if (ch === delimiter) { row.push(field); field = ""; }
    else if (ch === "\r" || ch === "\n") { finish(); if (ch === "\r" && text[i + 1] === "\n") i++; }
    else field += ch;
  }
  if (field || row.length) finish();
  return rows;
}

export function parseImport(text) {
  text = text.replace(/^\uFEFF/, "");
  let items = null;
  for (const delimiter of [",", ";", "\t"]) {
    const rows = csv(text, delimiter), header = (rows[0] ?? []).map(value => value.trim().toLocaleLowerCase());
    const title = header.findIndex(value => titleHeaders.has(value)), artist = header.findIndex(value => artistHeaders.has(value));
    if (title < 0 || artist < 0 || title === artist) continue;
    items = rows.slice(1).filter(row => row[title] !== undefined && row[artist] !== undefined)
      .map(row => ({title: clean(row[title]), artist: clean(row[artist].replace(/;/g, ", "))}));
    break;
  }
  if (!items) items = text.split(/\r\n|\n|\r/).map(line => {
    line = line.trim().replace(/^\d{1,4}[.)]\s+/, "");
    const split = /\s[—–-]\s|\t/.exec(line);
    return split ? {artist: clean(line.slice(0, split.index)), title: clean(line.slice(split.index + split[0].length))} : {artist: "", title: clean(line)};
  });
  const seen = new Set();
  return items.filter(item => {
    if (!item.title) return false;
    const id = `${key(item.artist)}\u001f${key(item.title)}`;
    if (seen.has(id)) return false;
    seen.add(id); return true;
  }).slice(0, IMPORT_LIMIT);
}

function similarity(a, b) {
  if (a === b) return 1;
  if (a.length < 2 || b.length < 2) return 0;
  const pairs = value => Array.from({length: value.length - 1}, (_, i) => value.slice(i, i + 2));
  const first = pairs(a), second = pairs(b); let shared = 0;
  for (const pair of first) { const index = second.indexOf(pair); if (index >= 0) { shared++; second.splice(index, 1); } }
  return 2 * shared / (a.length + b.length - 2);
}
export function bestImportMatch(item, candidates) {
  let best = null;
  for (const track of candidates) {
    const title = similarity(key(item.title), key(track.title));
    const artist = item.artist ? similarity(key(item.artist), key(track.artist)) : 0.85;
    if (title < (item.artist ? 0.8 : 0.95) || artist < 0.75) continue;
    let confidence = 0.6 * title + 0.4 * artist;
    // A live/remix/instrumental variant must be explicitly reviewed.
    const variants = /\b(remix|live|instrumental|acoustic|karaoke|cover|sped|slowed)\b/gi;
    if ((item.title.match(variants) ?? []).join().toLowerCase() !== (track.title.match(variants) ?? []).join().toLowerCase()) confidence *= 0.7;
    if (!best || confidence > best.confidence) best = {track, confidence};
  }
  return best;
}
