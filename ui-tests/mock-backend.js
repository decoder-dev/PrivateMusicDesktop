// Injected into the page before the UI loads. It stands in for the Tauri
// backend: a library of 1234 tracks, playlists, search, and a player that
// behaves like the real controller closely enough to drive the UI.
(() => {
  const calls = [];
  const handlers = new Map();
  const delay = (ms) => new Promise((r) => setTimeout(r, ms));
  const config = window.__mockConfig || {};

  const makeTrack = (n, extra = {}) => ({
    track_id: n, owner_id: 7,
    title: n % 97 === 0 ? `Ёлка ${n}` : `Трек ${n}`,
    artist: `Исполнитель ${n % 13}`,
    album_title: null, duration: 120 + (n % 200), stream_url: null,
    artwork_url: n % 3 === 0 ? null : `https://cdn.example/${n}.jpg`,
    access_key: null, lyrics_id: null, is_hq: false, track_code: null, ...extra,
  });

  const total = config.libraryTotal ?? 1234;
  const library = Array.from({ length: total }, (_, i) => makeTrack(i + 1));
  if (config.hostileTitle) library[0] = makeTrack(1, { title: config.hostileTitle, artist: config.hostileTitle });
  const playlists = [
    { playlist_id: 1, owner_id: 7, title: "Дорога", description: null, count: 3, artwork_url: null, access_key: null },
    { playlist_id: 2, owner_id: 7, title: "Работа", description: null, count: 2, artwork_url: "https://cdn.example/p.jpg", access_key: "k" },
  ];

  const state = {
    session: { signed_in: !!config.signedIn, profile: null, persistent: config.persistent !== false },
    player: { status: "idle", track: null, position: 0, duration: 0, volume: 1, shuffle: false, repeat: "off", queue_index: null, queue_len: 0 },
    queue: [],
  };

  const emit = (event, payload) => (handlers.get(event) ?? []).forEach((fn) => fn({ payload }));
  const emitState = () => emit("pm-event", { type: "state", ...state.player });
  const emitQueue = () => emit("pm-event", { type: "queue", tracks: state.queue, current: state.player.queue_index });
  const startCurrent = () => {
    const track = state.queue[state.player.queue_index] ?? null;
    state.player.track = track;
    state.player.status = track ? "playing" : "idle";
    state.player.position = 0;
    state.player.duration = track ? track.duration : 0;
    state.player.queue_len = state.queue.length;
  };

  const page = (items, offset, count) => {
    const slice = items.slice(offset, offset + count);
    const next = offset + slice.length < items.length && slice.length ? offset + slice.length : null;
    return { items: slice, total: items.length, next_offset: next };
  };

  const commands = {
    session_state: () => state.session,
    profile: () => ({ id: 7, first_name: "Иван", last_name: "Петров", photo_url: null }),
    login_start: async () => {
      setTimeout(() => {
        state.session = { signed_in: true, profile: null, persistent: true };
        emit("pm-login", "success");
      }, 30);
    },
    logout: () => { state.session = { signed_in: false, profile: null, persistent: true }; state.queue = []; },
    library: async ({ offset, count }) => { await delay(config.pageDelay ?? 5); return page(library, offset, count); },
    playlists: async ({ offset, count }) => page(playlists, offset, count),
    playlist_tracks: async ({ playlist, offset, count }) =>
      page(library.slice(0, playlist.count).map((t) => ({ ...t, title: `${playlist.title}: ${t.title}` })), offset, count),
    search: async ({ query, offset, count }) => {
      await delay(query === "slow" ? 300 : 10);
      const all = query === "empty" ? [] : Array.from({ length: 120 }, (_, i) => makeTrack(5000 + i, { title: `${query} ${i + 1}` }));
      return page(all, offset, count);
    },
    player_state: () => state.player,
    play_tracks: ({ tracks, index }) => {
      state.queue = tracks; state.player.queue_index = index; startCurrent(); emitQueue(); emitState();
    },
    load_queue: ({ tracks, index, position }) => {
      state.queue = tracks; state.player.queue_index = index; startCurrent();
      state.player.status = "idle"; state.player.position = position; emitQueue(); emitState();
    },
    toggle: () => {
      state.player.status = state.player.status === "playing" ? "paused" : "playing"; emitState();
    },
    next: () => {
      if (state.player.queue_index + 1 < state.queue.length) { state.player.queue_index += 1; startCurrent(); emitQueue(); emitState(); }
    },
    previous: () => {
      if (state.player.queue_index > 0) { state.player.queue_index -= 1; startCurrent(); emitQueue(); emitState(); }
    },
    seek: ({ seconds }) => { state.player.position = seconds; emitState(); },
    set_volume: ({ volume }) => { state.player.volume = volume; emitState(); },
    set_shuffle: ({ on }) => { state.player.shuffle = on; emitState(); },
    set_repeat: ({ repeat }) => { state.player.repeat = repeat; emitState(); },
    jump: ({ index }) => { state.player.queue_index = index; startCurrent(); emitQueue(); emitState(); },
    play_next: ({ track }) => { state.queue.splice((state.player.queue_index ?? -1) + 1, 0, track); state.player.queue_len = state.queue.length; emitQueue(); emitState(); },
    play_last: ({ track }) => { state.queue.push(track); state.player.queue_len = state.queue.length; emitQueue(); emitState(); },
    remove_from_queue: ({ index }) => {
      state.queue.splice(index, 1);
      if (state.queue.length === 0) { state.player.queue_index = null; state.player.track = null; state.player.status = "idle"; }
      else if (index < state.player.queue_index) state.player.queue_index -= 1;
      state.player.queue_len = state.queue.length; emitQueue(); emitState();
    },
    stop: () => { state.player.status = "idle"; emitState(); },
    add_to_library: async ({ track }) => {
      await delay(10);
      if (config.libraryWriteFails) throw { kind: "vk", message: "VK error 15: Access denied" };
      const copy = { ...track, track_id: 900000 + track.track_id, owner_id: 7, access_key: null };
      library.unshift(copy);
      return copy;
    },
    remove_from_library: async ({ track }) => {
      await delay(10);
      if (config.libraryWriteFails) throw { kind: "vk", message: "VK error 15: Access denied" };
      const index = library.findIndex((t) => t.owner_id === track.owner_id && t.track_id === track.track_id);
      if (index < 0) throw { kind: "vk", message: "not in library" };
      library.splice(index, 1);
    },
    lyrics: async ({ track }) => {
      if (config.lyricsFail) throw { kind: "offline", message: "No internet connection." };
      await delay(15);
      const n = track.track_id;
      if (n % 5 === 0) return null;
      if (n % 2 === 0) return { source: "VK", text: "первая строка\nвторая <b>строка</b>", lines: [] };
      const lines = Array.from({ length: 40 }, (_, i) => ({ time: 5 + i * 5, text: i === 3 ? "" : i === 0 ? config.hostileLyric ?? "Строка 0" : `Строка ${i}` }));
      return { source: "LRCLIB", text: lines.map((l) => l.text).join("\n"), lines };
    },
    set_visualizer: ({ on }) => { state.visualizer = on; },
    artwork_palette: ({ url }) => {
      if (config.paletteFails) throw { kind: "format", message: "no palette" };
      return url.includes("/6.") ? { primary: "#e02838", secondary: "#2838e0" } : { primary: "#28a050", secondary: "#a05028" };
    },
    set_fullscreen: ({ on }) => { state.fullscreen = on; },
  };

  window.__TAURI__ = {
    core: {
      invoke: async (command, args) => {
        calls.push({ command, args });
        if (!(command in commands)) throw { kind: "invalid", message: `unknown command ${command}` };
        if (config.failing && config.failing.includes(command)) throw { kind: "offline", message: "No internet connection." };
        return commands[command](args ?? {});
      },
    },
    event: {
      listen: async (event, handler) => {
        if (!handlers.has(event)) handlers.set(event, []);
        handlers.get(event).push(handler);
        return () => {};
      },
    },
  };
  window.__mock = {
    calls, state, emit, emitState, emitQueue,
    last: (command) => [...calls].reverse().find((c) => c.command === command),
    count: (command) => calls.filter((c) => c.command === command).length,
    sessionExpired: () => { state.session = { signed_in: false, profile: null, persistent: true }; emit("pm-event", { type: "session_expired" }); },
  };
})();
