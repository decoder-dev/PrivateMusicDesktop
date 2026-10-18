import { api } from "./api.js";

const cache = new Map();

/** Запомнилось два цвета крышки от двигателя. Отклоняет, когда не может сказать. */
export async function coverPalette(url) {
  if (!cache.has(url)) {
    const palette = await api.artworkPalette(url);
    if (cache.size > 100) cache.clear();
    cache.set(url, palette);
  }
  return cache.get(url);
}
