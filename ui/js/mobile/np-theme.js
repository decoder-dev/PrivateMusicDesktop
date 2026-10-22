// Оформление фона полноэкранного плеера: «cover» (размытая обложка, по умолчанию), «aurora» (плавные
// цветные пятна из цветов обложки) и «plain» (спокойный однотонный фон). Хранится на устройстве и
// ставится на <html data-np-theme>; CSS: css/np-themes.css.
const KEY = "pm-np-theme";
export const NP_THEMES = ["cover", "aurora", "plain"];

export function currentNpTheme() {
  try { const value = localStorage.getItem(KEY); return NP_THEMES.includes(value) ? value : "cover"; } catch { return "cover"; }
}
export function applyNpTheme(value = currentNpTheme()) {
  document.documentElement.dataset.npTheme = value;
}
export function setNpTheme(value) {
  if (!NP_THEMES.includes(value)) return;
  try { localStorage.setItem(KEY, value); } catch { /* память недоступна: действует до перезапуска */ }
  applyNpTheme(value);
}
applyNpTheme();
