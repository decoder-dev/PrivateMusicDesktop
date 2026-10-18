import { ft } from "./feature-i18n.js";
import { store, publish } from "./store.js";

// Только пользовательские настройки и метаданные; адреса потоков сюда не попадают.
export const accountId = () =>
  store.session.profile?.id ?? store.session.profile?.user_id ?? null;
export function readPersonal(name, fallback) {
  const id = accountId();
  if (!id) return fallback;
  try {
    const value = JSON.parse(localStorage.getItem(`pm.personal.${id}.${name}`));
    return value != null &&
      typeof value === typeof fallback &&
      Array.isArray(value) === Array.isArray(fallback)
      ? value
      : fallback;
  } catch {
    return fallback;
  }
}
export function writePersonal(name, value) {
  const id = accountId();
  if (!id)
    throw new Error(
      ft("Профиль ещё загружается. Повторите после загрузки аккаунта."),
    );
  // Ошибку переполненного хранилища показываем вызывающему: сохранение нельзя обещать молча.
  localStorage.setItem(`pm.personal.${id}.${name}`, JSON.stringify(value));
  publish(`personal.${name}`);
}
export function cleanTrack(track) {
  return { ...track, stream_url: null };
}
