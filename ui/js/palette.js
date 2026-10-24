import { api } from "./api.js";

const cache = new Map();

/** Запомнилось два цвета крышки от двигателя. Отклоняет, когда не может сказать. */
export async function coverPalette(url) {
  if (!cache.has(url)) {
    if (cache.size > 100) cache.clear();
    // The mini player, full player and Home can request the same cover together.
    // Cache the in-flight work too: otherwise each downloads and decodes it again.
    const pending = api.artworkPalette(url).catch(error => {
      if (cache.get(url) === pending) cache.delete(url);
      throw error;
    });
    cache.set(url, pending);
  }
  return cache.get(url);
}
