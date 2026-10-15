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

export function subscribe(topic, handler) {
  if (!listeners.has(topic)) listeners.set(topic, new Set());
  listeners.get(topic).add(handler);
  return () => listeners.get(topic).delete(handler);
}

export function publish(topic) {
  for (const handler of listeners.get(topic) ?? []) handler(store[topic]);
}

export function set(topic, value) {
  store[topic] = value;
  publish(topic);
}
