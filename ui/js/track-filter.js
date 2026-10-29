const fold = (text) => String(text ?? "").toLowerCase().replaceAll("ё", "е");

/** Splits a typed filter into folded words; empty input means "show everything". */
export function filterWords(input) {
  return fold(input).split(/\s+/u).filter(Boolean);
}

/** Every word must appear somewhere in "title artist", in any order: "кино группа" finds Кино — Группа крови. */
export function matchesWords(words, ...fields) {
  if (!words.length) return true;
  const haystack = fold(fields.filter(Boolean).join(" "));
  return words.every((word) => haystack.includes(word));
}

/**
 * Exact phrase first ("Исполнитель 5" finds that artist, not "Трек 15" by someone else);
 * when nothing contains the phrase, fall back to words in any order.
 */
export function filterItems(items, words, fieldsOf) {
  if (!words.length) return items;
  const phrase = words.join(" ");
  const exact = items.filter((item) => fieldsOf(item).some((field) => fold(field).includes(phrase)));
  return exact.length ? exact : items.filter((item) => matchesWords(words, ...fieldsOf(item)));
}
