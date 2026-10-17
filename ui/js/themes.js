// Colours and defaults ported from PrivateMusic2/App/AppPalette.swift.
import { addStrings } from "./i18n.js";
export const PALETTES = ["classic", "banana", "midnight", "forest", "rose", "mocha", "halloween"];
export const PALETTE_ACCENT = { classic: "blue", banana: "banana", midnight: "indigo", forest: "green", rose: "pink", mocha: "orange", halloween: "orange" };
export const COLORS = {
  classic: ["#ffffff", "#f6f6f6", "#f4f4f4", "#000000", "#0e0e0e", "#131313"],
  banana: ["#F6F3EB", "#F1EDE2", "#FFFCF5", "#181B20", "#14171B", "#23272E"],
  midnight: ["#EEF2FA", "#E6ECF8", "#FFFFFF", "#0B1020", "#090D1A", "#141B33"],
  forest: ["#EEF4EE", "#E5EFE6", "#FBFFFB", "#0D1511", "#0A110D", "#16221B"],
  rose: ["#FBF1F3", "#F7E8EC", "#FFFAFB", "#1A1115", "#140C10", "#261A20"],
  mocha: ["#F4ECE4", "#EDE2D7", "#FFF9F2", "#19130F", "#130E0B", "#251C16"],
  halloween: ["#FFF4E6", "#F3EAF8", "#FFFAF4", "#100E17", "#1B1222", "#292033"],
};
const ACCENT = {
  blue: ["#1a6be0", "#3385fa"], indigo: ["#4f52db", "#6e73fa"], violet: ["#874ccc", "#b275f7"], pink: ["#bf3670", "#fa66a3"],
  red: ["#c73338", "#fa6161"], orange: ["#a6570f", "#ff9933"], green: ["#1c7a45", "#4ccc7a"], teal: ["#0f787a", "#3dcccc"], banana: ["#805600", "#f4d66f"],
};
addStrings({ ru: { "palette.title": "Палитра", "palette.classic": "Классика", "palette.banana": "BananaGram", "palette.midnight": "Полночь", "palette.forest": "Лес", "palette.rose": "Роза", "palette.mocha": "Мокко", "palette.halloween": "Хэллоуин" }, en: { "palette.title": "Palette", "palette.classic": "Classic", "palette.banana": "BananaGram", "palette.midnight": "Midnight", "palette.forest": "Forest", "palette.rose": "Rose", "palette.mocha": "Mocha", "palette.halloween": "Halloween" } });
const rgb = (hex) => [1,3,5].map((i) => parseInt(hex.slice(i,i+2),16)/255);
const luminance = (color) => rgb(color).map((v) => v <= 0.04045 ? v/12.92 : ((v+0.055)/1.055)**2.4).reduce((a,v,i) => a+v*[0.2126,0.7152,0.0722][i],0);
const ratio = (a,b) => (Math.max(a,b)+0.05)/(Math.min(a,b)+0.05);
function readable(hex, backgrounds, dark) {
  let color = rgb(hex);
  const encode = () => "#"+color.map((v) => Math.round(v*255).toString(16).padStart(2,"0")).join("");
  for (let i=0;i<120;i++) {
    const value = encode();
    if (backgrounds.every((bg) => ratio(luminance(value),luminance(bg)) >= 4.5)) return value;
    color = color.map((v) => v+((dark?1:0)-v)*0.02);
  }
  return encode();
}
export function applyPalette(settings) {
  const root = document.documentElement;
  const colors = COLORS[settings.palette] ?? COLORS.classic;
  root.dataset.palette = settings.palette ?? "classic";
  ["--pal-l-top","--pal-l-bottom","--pal-l-surface","--pal-d-top","--pal-d-bottom","--pal-d-surface"].forEach((key,i) => root.style.setProperty(key,colors[i]));
  const [light,dark] = ACCENT[settings.accent] ?? ACCENT.blue;
  root.style.setProperty("--a-l",readable(light,colors.slice(0,3),false));
  root.style.setProperty("--a-d",readable(dark,colors.slice(3),true));
}
