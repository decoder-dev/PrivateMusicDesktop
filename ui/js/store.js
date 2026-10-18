// Один небольшой заметный магазин. Просмотры подписываются на темы, которые они рисуют.

const listeners = new Map();

export const store = {
  session: { signed_in: false, profile: null, persistent: true },
  player: {
    status: "idle", track: null, position: 0, duration: 0, volume: 1,
    shuffle: false, repeat: "off", queue_index: null, queue_len: 0,
  },
  queue: { tracks: [], current: null },
  view: "library",
};

// Все, что подключается экран входа в систему (подписки, прослушиватели документов), собирается во
// время его создания, поэтому выход из системы может отменить все это сразу.
let scope = null;

/** Регистрирует очистку в создаваемой области, если таковая имеется. */
export function own(dispose) {
  scope?.push(dispose);
  return dispose;
}

/** Запускает `build`, возвращая его значение и `dispose` для того, что он подключил. */
export function withScope(build) {
  const previous = scope;
  const owned = [];
  scope = owned;
  try {
    return { value: build(), dispose: () => owned.splice(0).reverse().forEach((fn) => fn()) };
  } finally {
    scope = previous;
  }
}

export function subscribe(topic, handler) {
  if (!listeners.has(topic)) listeners.set(topic, new Set());
  listeners.get(topic).add(handler);
  return own(() => listeners.get(topic)?.delete(handler));
}

export function publish(topic) {
  for (const handler of listeners.get(topic) ?? []) handler(store[topic]);
}

export function set(topic, value) {
  store[topic] = value;
  publish(topic);
}

/** Для тестов: сколько обработчиков слушают тему. */
export const listenerCount = (topic) => listeners.get(topic)?.size ?? 0;
