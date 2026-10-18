import { api } from "./api.js";
let mode = "medium";
export const isWindowFullscreen = () => mode === "fullscreen";
export async function setWindowMode(next) { await api.setWindowMode(next); mode=next; document.documentElement.dataset.windowMode=mode; window.dispatchEvent(new Event("pm-window-mode")); }
export async function toggleWindowFullscreen(force) { const on=force ?? !isWindowFullscreen(); await api.setFullscreen(on); mode=on?"fullscreen":"medium";document.documentElement.dataset.windowMode=mode;window.dispatchEvent(new Event("pm-window-mode"));return on; }
