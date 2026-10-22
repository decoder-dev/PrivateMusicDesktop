// Плоские значки stroke/fill, 24x24, нарисованные с помощью currentColor, поэтому темы просто работают.
const NS = "http://www.w3.org/2000/svg";

const ICONS = {
  bolt: { fill: true, d: ["M14 2L4 14h7l-1 8 10-13h-7z"] },
  moon: { fill: true, d: ["M20.5 14A9 9 0 0110 3a9 9 0 1010.5 11z"] },
  rain: { d: ["M6 15a4 4 0 010-8 6 6 0 0111-1 4.5 4.5 0 011 9", "M7 18l-1 3", "M12 17l-1 3", "M17 18l-1 3"] },
  smile: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M8 14s1 3 4 3 4-3 4-3", "M8 9h.01", "M16 9h.01"] },
  pin: { d: ["M8 3h8l-1 6 4 4H5l4-4z", "M12 13v8"] },
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
  house: { d: ["M3.5 11.2L12 4l8.5 7.2", "M5.5 10v9.5h4.5v-5.5h4v5.5h4.5V10"] },
  "music-list": { d: ["M3 6h11", "M3 11h11", "M3 16h6", "M18 17V6.5l3-.7", "M18 17a2.2 2.2 0 11-4.4 0 2.2 2.2 0 014.4 0z"] },
  "person-circle": { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 12.5a3 3 0 100-6 3 3 0 000 6z", "M6.2 18.2c1.2-2.2 3.3-3.2 5.8-3.2s4.6 1 5.8 3.2"] },
  sliders: { d: ["M4 7h9", "M17 7h3", "M4 17h3", "M11 17h9", "M15 7a2 2 0 100 .01", "M9 17a2 2 0 100 .01"] },
  info: { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 11v5.5", "M12 7.6v.1"] },
  "plus-circle": { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 8v8", "M8 12h8"] },
  "arrow-down-circle": { d: ["M12 21a9 9 0 100-18 9 9 0 000 18z", "M12 8v8", "M8.5 12.5L12 16l3.5-3.5"] },
  lock: { d: ["M6 11h12v9H6z", "M8.5 11V8a3.5 3.5 0 017 0v3"] },
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
 * Небольшое пустое изображение: мягкий акцент позади двух наклоненных карт с глифом на передней
 * стороне. Нарисовано с использованием классов CSS (il-*), поэтому соответствует теме и акценту;
 * статичный, декоративный (скрытый арией).
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
