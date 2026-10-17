// Flat stroke/fill icons, 24x24, drawn with currentColor so themes just work.
const NS = "http://www.w3.org/2000/svg";

const ICONS = {
  play: { fill: true, d: ["M8 5v14l11-7z"] },
  pause: { fill: true, d: ["M6 5h4v14H6z", "M14 5h4v14h-4z"] },
  prev: { fill: true, d: ["M6 6h2v12H6z", "M19 6v12L9.5 12z"] },
  next: { fill: true, d: ["M16 6h2v12h-2z", "M5 6v12l9.5-6z"] },
  shuffle: { d: ["M3 6h3c3 0 5 2 7 6s4 6 7 6h1", "M3 18h3c1.5 0 2.8-.7 3.8-1.8", "M13.5 9.5C14.6 7.4 16.2 6 19 6h2", "M19 3l3 3-3 3", "M19 15l3 3-3 3"] },
  repeat: { d: ["M17 2l4 4-4 4", "M3 11V9a3 3 0 013-3h15", "M7 22l-4-4 4-4", "M21 13v2a3 3 0 01-3 3H3"] },
  volume: { d: ["M4 9v6h4l5 4V5L8 9z", "M16.5 9a4 4 0 010 6", "M19 6.5a8 8 0 010 11"] },
  mute: { d: ["M4 9v6h4l5 4V5L8 9z", "M17 9l5 6", "M22 9l-5 6"] },
  note: { d: ["M9 18V5l11-2v13", "M9 18a3 3 0 11-6 0 3 3 0 016 0z", "M20 16a3 3 0 11-6 0 3 3 0 016 0z"] },
  plus: { d: ["M12 5v14", "M5 12h14"] },
  "queue-add": { d: ["M3 6h12", "M3 12h12", "M3 18h8", "M18 15v6", "M15 18h6"] },
  "play-next": { d: ["M3 6h10", "M3 12h7", "M3 18h7", "M13 11l8 4.5-8 4.5z"] },
  "move-up": { d: ["M6 15l6-6 6 6"] },
  "move-down": { d: ["M6 9l6 6 6-6"] },
  close: { d: ["M6 6l12 12", "M18 6L6 18"] },
  search: { d: ["M11 4a7 7 0 100 14 7 7 0 000-14z", "M21 21l-5-5"] },
  playlist: { d: ["M3 6h13", "M3 12h13", "M3 18h7", "M19 8v9", "M19 17a2 2 0 11-4 0 2 2 0 014 0z"] },
  library: { d: ["M3 5h11", "M3 10h11", "M3 15h6", "M18 18V7l4-1v10", "M18 18a2 2 0 11-4 0 2 2 0 014 0z", "M22 16a2 2 0 11-4 0 2 2 0 014 0z"] },
  home: { fill: true, d: ["M2 11L12 2l10 9-2 2-1-1v10h-5v-7h-4v7H5V12l-1 1z"] },
  queue: { d: ["M3 6h18", "M3 12h18", "M3 18h12"] },
  more: { fill: true, d: ["M5 10.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3z", "M12 10.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3z", "M19 10.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3z"] },
  edit: { d: ["M4 20h4L19 9l-4-4L4 16z", "M13 7l4 4"] },
  trash: { d: ["M4 7h16", "M9 7V4h6v3", "M6 7l1 13h10l1-13"] },
  chevronRight: { d: ["M9 6l6 6-6 6"] },
  download: { d: ["M12 4v11", "M7 10l5 5 5-5", "M5 20h14"] },
  downloaded: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M8 12l3 3 5-6"] },
  radio: { d: ["M4.9 19.1a10 10 0 010-14.2", "M19.1 4.9a10 10 0 010 14.2", "M7.8 16.2a6 6 0 010-8.4", "M16.2 7.8a6 6 0 010 8.4", "M12 13a1 1 0 100-2 1 1 0 000 2z"] },
  sparkles: { d: ["M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z", "M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z"] },
  settings: { d: ["M12 15a3 3 0 100-6 3 3 0 000 6z", "M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"] },
  heart: { d: ["M12 20s-7-4.4-9.2-8.6C1.3 8.5 3 5 6.4 5c2 0 3.3 1.1 4 2.3.7-1.2 2-2.3 4-2.3C18 5 19.7 8.5 18.2 11.4 16 15.6 12 20 12 20z"] },
  "heart-on": { fill: true, d: ["M12 20s-7-4.4-9.2-8.6C1.3 8.5 3 5 6.4 5c2 0 3.3 1.1 4 2.3.7-1.2 2-2.3 4-2.3C18 5 19.7 8.5 18.2 11.4 16 15.6 12 20 12 20z"] },
  lyrics: { d: ["M4 6h16", "M4 11h10", "M4 16h16", "M4 21h8"] },
  expand: { d: ["M4 9V4h5", "M20 9V4h-5", "M4 15v5h5", "M20 15v5h-5"] },
  collapse: { d: ["M9 4v5H4", "M15 4v5h5", "M9 20v-5H4", "M15 20v-5h5"] },
  chevron: { d: ["M6 9l6 6 6-6"] },
  visualizer: { d: ["M5 20V12", "M9 20V6", "M13 20V10", "M17 20V4", "M21 20V14"] },
  back: { d: ["M15 5l-7 7 7 7"] },
  artist: { d: ["M12 12a4 4 0 100-8 4 4 0 000 8z", "M4 21c0-4 3.6-6.5 8-6.5s8 2.5 8 6.5"] },
  friends: { d: ["M9 11a3.5 3.5 0 100-7 3.5 3.5 0 000 7z", "M2.5 20c0-3.6 2.9-5.5 6.5-5.5s6.5 1.9 6.5 5.5", "M16 4.4a3.5 3.5 0 010 6.2", "M18 14.8c2 .6 3.5 2.2 3.5 5.2"] },
  history: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 7v5l3 2"] },
  chart: { d: ["M4 20h16", "M7 20v-7", "M12 20V5", "M17 20v-10"] },
  album: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 14.5a2.5 2.5 0 100-5 2.5 2.5 0 000 5z"] },
  refresh: { d: ["M20 11a8 8 0 00-14.5-4", "M5 4v3.5h3.5", "M4 13a8 8 0 0014.5 4", "M19 20v-3.5h-3.5"] },
  alert: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 7.5v5.5", "M12 16.4v.1"] },
  check: { d: ["M5 12.5l4.5 4.5L19 7.5"] },
  logout: { d: ["M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4", "M16 17l5-5-5-5", "M21 12H9"] },
  clear: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M9 9l6 6", "M15 9l-6 6"] },
};

export function icon(name, size = 18) {
  const spec = ICONS[name];
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  if (spec.fill) {
    svg.setAttribute("fill", "currentColor");
  } else {
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "2");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
  }
  for (const d of spec.d) {
    const path = document.createElementNS(NS, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
}

const svgEl = (tag, attrs) => {
  const node = document.createElementNS(NS, tag);
  for (const [name, value] of Object.entries(attrs)) node.setAttribute(name, String(value));
  return node;
};

const ILLUSTRATIONS = { music: "note", error: "alert", download: "download", search: "search", friends: "friends", history: "history", playlist: "playlist", queue: "queue", chart: "chart", album: "album", artist: "artist" };

/**
 * A small empty-state picture: a soft accent wash behind two tilted cards with a
 * glyph on the front one. Drawn with CSS classes (il-*), so it follows the theme
 * and the accent; static, decorative (aria-hidden).
 */
export function illustration(kind = "music") {
  const svg = svgEl("svg", { class: "il", viewBox: "0 0 120 88", width: 120, height: 88, "aria-hidden": "true", focusable: "false" });
  svg.append(
    svgEl("ellipse", { class: "il-wash", cx: 60, cy: 48, rx: 54, ry: 34 }),
    svgEl("rect", { class: "il-card back", x: 24, y: 16, width: 52, height: 52, rx: 14, transform: "rotate(-9 50 42)" }),
    svgEl("rect", { class: "il-card front", x: 44, y: 18, width: 52, height: 52, rx: 14, transform: "rotate(6 70 44)" }),
    svgEl("circle", { class: "il-dot a", cx: 22, cy: 66, r: 3 }),
    svgEl("circle", { class: "il-dot b", cx: 104, cy: 22, r: 2.5 }),
  );
  const glyph = icon(ILLUSTRATIONS[kind] ?? "note", 26);
  glyph.setAttribute("class", "il-glyph");
  glyph.setAttribute("x", "57");
  glyph.setAttribute("y", "31");
  svg.append(glyph);
  return svg;
}
