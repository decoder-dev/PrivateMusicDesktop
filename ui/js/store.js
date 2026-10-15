// One small observable store. Views subscribe to the topics they draw.

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

// Everything a signed-in screen hooks up (subscriptions, document listeners)
// is collected while it is built, so signing out can undo all of it at once.
let scope = null;

/** Registers a clean-up with the scope being built, if any. */
export function own(dispose) {
  scope?.push(dispose);
  return dispose;
}

/** Runs `build`, returning its value and a `dispose` for what it hooked up. */
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

/** For tests: how many handlers listen to a topic. */
export const listenerCount = (topic) => listeners.get(topic)?.size ?? 0;
