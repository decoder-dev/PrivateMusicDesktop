// Keeps the user's scrubber value until the engine confirms it. A resolved invoke
// alone is not confirmation: its state event can still arrive after older packets.
export function createSeekGate(notify, { tolerance = 1.1, timeout = 1200 } = {}) {
  let generation = 0;
  let trackKey = null;
  let pending = null;
  let dragging = false;
  let timer = null;

  function clearTimer() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }

  function settle() {
    generation += 1;
    clearTimer();
    pending = null;
    dragging = false;
    notify();
  }

  function cancel() {
    generation += 1;
    clearTimer();
    pending = null;
    dragging = false;
  }

  return {
    preview() {
      generation += 1;
      clearTimer();
      pending = null;
      dragging = true;
    },
    track(key) {
      if (key === trackKey) return;
      trackKey = key;
      cancel();
    },
    cancel,
    async commit(value, run) {
      const mine = ++generation;
      clearTimer();
      pending = { value, at: performance.now() };
      dragging = true;
      try { return await run(); }
      finally {
        if (mine === generation) {
          pending.at = performance.now();
          timer = setTimeout(settle, timeout);
          notify();
        }
      }
    },
    shouldRender(position, now = performance.now()) {
      if (!pending) return !dragging;
      if (Math.abs(Math.max(0, position || 0) - pending.value) <= tolerance) {
        settle();
        return true;
      }
      if (now - pending.at >= timeout) {
        settle();
        return true;
      }
      return false;
    },
  };
}
