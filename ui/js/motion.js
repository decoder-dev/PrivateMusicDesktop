// Движение считывается в реальном времени: человек может включить «Уменьшение движения» во время работы
// приложения, поэтому при загрузке ничего не фиксируется.

const query = matchMedia("(prefers-reduced-motion: no-preference)");

/** Верно, если система прямо сейчас разрешает анимацию. */
export const motionOk = () => query.matches;

/** Вызывает `fn(motionOk)` всякий раз, когда изменяются настройки системы. */
export function onMotionChange(fn) {
  const handler = () => fn(query.matches);
  query.addEventListener("change", handler);
  return () => query.removeEventListener("change", handler);
}

/**
 * Запускается `done` один раз, когда заканчивается анимация, которая только что была запущена на
 * `element`. Если движение отключено или анимация не запущена, она запускается сразу, поэтому логика
 * закрытия никогда не ждет того, чего не произойдет.
 */
export function afterAnimation(element, done, fallback = 450) {
  if (!motionOk() || !element.getAnimations?.().length) {
    done();
    return;
  }
  let finished = false;
  const finish = () => {
    if (finished) return;
    finished = true;
    element.removeEventListener("animationend", onEnd);
    clearTimeout(timer);
    done();
  };
  const onEnd = (event) => { if (event.target === element) finish(); };
  element.addEventListener("animationend", onEnd);
  const timer = setTimeout(finish, fallback);
}

/** Воспроизводит одноразовый класс анимации ввода для только что показанного элемента. */
export function enter(element) {
  if (!motionOk()) return;
  element.classList.remove("enter");
  void element.offsetWidth;
  element.classList.add("enter");
  element.addEventListener("animationend", () => element.classList.remove("enter"), { once: true });
}

/**
 * Однократная замена глифа (play/pause, сердце): появляется новый глиф. Только тогда, когда глиф
 * действительно изменился.
 */
export function morph(element) {
  if (!motionOk() || !element) return;
  element.classList.add("morph");
  element.addEventListener("animationend", () => element.classList.remove("morph"), { once: true });
}
