// Что есть в «Моя музыка», для сердец повсюду. Трек, добавленный из поиска, получает новый идентификатор
// в библиотеке, поэтому копия запоминается под ключом трека, из которого она взята, и любой из них
// отображается как понравившийся.

import { api, trackKey } from "./api.js";
import { publish, own } from "./store.js";

const inLibrary = new Map(); // клавиша библиотечного трека -> трек
const copies = new Map(); // тональность исходного трека -> его библиотечная копия
const pending = new Map();
let epoch = 0;
const listeners = new Set();
/** Пока библиотека не загружена, сердце не может знать, добавляет ли щелчок дубликат. */
let known = false;

/** Представление библиотеки сообщает, что оно загрузило. */
export function setLibrary(tracks) {
  inLibrary.clear();
  for (const track of tracks) inLibrary.set(trackKey(track), track);
  known = true;
  for (const [source, copy] of copies) if (!inLibrary.has(trackKey(copy))) copies.delete(source);
  publish("likes");
}

export function forgetLibrary() {
  epoch++;
  pending.clear();
  known = false;
  inLibrary.clear();
  copies.clear();
  publish("likes");
}

function libraryCopy(track) {
  const key = trackKey(track);
  return inLibrary.get(key) ?? copies.get(key) ?? null;
}

export const isLiked = (track) => !!track && libraryCopy(track) !== null;
export const isBusy = (track) => !known || (!!track && pending.has(trackKey(track)));

/** `listener({ added } | { removed })` — представление библиотеки сохраняет свой список в порядке. */
export function onLibraryChange(listener) {
  listeners.add(listener);
  return own(() => listeners.delete(listener));
}

/** Добавляет или удаляет; разрешается в "added"/"removed", отклоняется с ошибкой API. */
export async function toggleLike(track) {
  const key = trackKey(track);
  if (!known || pending.has(key)) return null;
  const mine = epoch;
  pending.set(key,mine);
  publish("likes");
  try {
    const copy = libraryCopy(track);
    if (copy) {
      await api.removeFromLibrary(copy);
      if (mine !== epoch) return null;
      inLibrary.delete(trackKey(copy));
      for (const [source, value] of copies) if (trackKey(value) === trackKey(copy)) copies.delete(source);
      listeners.forEach((fn) => fn({ removed: copy }));
      return "removed";
    }
    const added = await api.addToLibrary(track);
    if (mine !== epoch) return null;
    inLibrary.set(trackKey(added), added);
    if (trackKey(added) !== key) copies.set(key, added);
    listeners.forEach((fn) => fn({ added }));
    return "added";
  } finally {
    if (pending.get(key) === mine) pending.delete(key);
    if (mine === epoch) publish("likes");
  }
}
