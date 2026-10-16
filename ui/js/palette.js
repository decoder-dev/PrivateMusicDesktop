import { api } from "./api.js";

const cache = new Map();

/** The cover's two colours from the engine, remembered. Rejects when it cannot tell. */
export async function coverPalette(url) {
  if (!cache.has(url)) {
    const palette = await api.artworkPalette(url);
    if (cache.size > 100) cache.clear();
    cache.set(url, palette);
  }
  return cache.get(url);
}
