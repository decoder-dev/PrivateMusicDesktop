// Injected into the page before the UI loads. It stands in for the Tauri
// backend: a library of 1234 tracks, playlists, search, and a player that
// behaves like the real controller closely enough to drive the UI.
(() => {
  const calls = [];
  const handlers = new Map();
  const delay = (ms) => new Promise((r) => setTimeout(r, ms));
  const config = window.__mockConfig || {};

  // Artists are linked by id, except every 13th track (named only, found by
  // name); every 10th+4 has a guest too. Even tracks name their album.
  const artistsOf = (n) => {
    if (n % 13 === 0) return [];
    const main = { id: `art${n % 13}`, name: `Исполнитель ${n % 13}` };
    return n % 10 === 4 ? [main, { id: "art99", name: "Гость" }] : [main];
  };
  const makeTrack = (n, extra = {}) => ({
    track_id: n, owner_id: 7,
    title: n % 97 === 0 ? `Ёлка ${n}` : `Трек ${n}`,
    artist: `Исполнитель ${n % 13}`,
    album_title: n % 2 === 0 ? `Альбом ${n % 5 + 1}` : null, duration: 120 + (n % 200), stream_url: null,
    artwork_url: n % 3 === 0 ? null : `https://cdn.example/${n}.jpg`,
    access_key: null, lyrics_id: null, is_hq: false, track_code: null,
    artists: artistsOf(n),
    album: n % 2 === 0 ? { album_id: n % 5 + 1, owner_id: -2000, access_key: null, title: `Альбом ${n % 5 + 1}` } : null,
    ...extra,
  });
  const artistName = (id) => (id === "art99" ? "Гость" : `Исполнитель ${String(id).replace("art", "")}`);
  const makeAlbum = (n, extra = {}) => ({
    album_id: n, owner_id: -2000, title: `Альбом ${n}`, description: null, count: 12,
    artwork_url: n % 2 ? `https://cdn.example/al${n}.jpg` : null, access_key: n % 2 ? "k" : null,
    artists: [{ id: "art5", name: "Исполнитель 5" }], year: 2010 + n, is_followed: false, follow_hash: null, ...extra,
  });
  const sameAlbum = (a, b) => a.owner_id === b.owner_id && a.album_id === b.album_id;

  const total = config.libraryTotal ?? 1234;
  const library = Array.from({ length: total }, (_, i) => makeTrack(i + 1));
  if (config.hostileTitle) library[0] = makeTrack(1, { title: config.hostileTitle, artist: config.hostileTitle });
  const playlists = [
    { playlist_id: 1, owner_id: 7, title: "Дорога", description: null, count: 3, artwork_url: null, access_key: null },
    { playlist_id: 2, owner_id: 7, title: "Работа", description: null, count: 2, artwork_url: "https://cdn.example/p.jpg", access_key: "k" },
    { playlist_id: 9, owner_id: -50, title: "Чужой", description: null, count: 2, artwork_url: null, access_key: null },
  ];
  const playlistTracks = new Map();

  // Friends: Анна has music and playlists, Борис keeps his audio private (VK
  // answers with an error), Ёлка has nothing. `friendsTotal` makes a long list.
  const friends = config.friendsTotal
    ? Array.from({ length: config.friendsTotal }, (_, i) => ({ id: 100 + i, first_name: `Друг${i + 1}`, last_name: "Тестов", photo_url: null }))
    : [
      { id: 11, first_name: "Анна", last_name: "Смирнова", photo_url: "https://cdn.example/anna.jpg" },
      { id: 12, first_name: "Борис", last_name: "Закрытый", photo_url: null },
      { id: 13, first_name: "Ёлка", last_name: "Пустая", photo_url: null },
    ];
  if (config.hostileFriend) friends[0].first_name = config.hostileFriend;
  const friendTrackList = (id) => Array.from({ length: 5 }, (_, i) => makeTrack(30000 + id * 10 + i, { title: `Друг ${id}: трек ${i + 1}`, owner_id: id }));
  const friendPlaylistList = (id) => [
    { playlist_id: 1, owner_id: id, title: "Плейлист друга", description: null, count: 3, artwork_url: null, access_key: "fk" },
    { playlist_id: 2, owner_id: id, title: "Ещё один", description: null, count: 2, artwork_url: "https://cdn.example/fp.jpg", access_key: null },
  ];
  const privateAudio = () => ({ kind: "vk", message: "VK error 201: Access denied: user hid his audio" });

  // Listening history and statistics, as the engine would answer them.
  const history = (config.history ?? []).map((n) => ({ track: makeTrack(n, { title: `Из истории ${n}` }), played_at: 1_772_620_200 - n }));
  const statsMonths = { "2026-03": { plays: 40, seconds: 5400 + 25, peak: 19, weekday: 3, days: 6 }, "2026-02": { plays: 25, seconds: 1800, peak: 8, weekday: 1, days: 3 } };
  const statsFor = (scope, choice) => {
    const months = state.statsEmpty ? [] : Object.keys(statsMonths);
    const years = state.statsEmpty ? [] : [2026];
    const key = scope === "month" ? choice ?? months[0] ?? "2026-04" : null;
    const picked = scope === "month" ? (state.statsEmpty ? [] : [statsMonths[key]].filter(Boolean)) : (state.statsEmpty ? [] : Object.values(statsMonths));
    const plays = picked.reduce((sum, m) => sum + m.plays, 0);
    const seconds = picked.reduce((sum, m) => sum + m.seconds, 0);
    const hour_seconds = Array.from({ length: 24 }, (_, i) => (picked.length && i === picked[0].peak ? seconds / 2 : picked.length ? 10 : 0));
    const period = scope === "month" ? { kind: "month", key } : scope === "year" ? { kind: "year", year: Number(choice ?? 2026) } : { kind: "all" };
    const entry = (id, title, subtitle, n) => ({ id, title, subtitle, artwork_url: subtitle ? `https://cdn.example/${id}.jpg` : null, plays: n, seconds: n * 200 });
    return {
      summary: {
        period, total_seconds: seconds, plays, active_days: picked.reduce((sum, m) => sum + m.days, 0),
        top_artists: plays ? [entry("a1", "Кино", null, 12), entry("a2", "Исполнитель 2", null, 7)] : [],
        top_tracks: plays ? [entry("7_1", "Группа крови", "Кино", 5), entry("7_2", "Трек 2", "Исполнитель 2", 3)] : [],
        hour_seconds, peak_hour: plays ? picked[0].peak : null, favourite_weekday: plays ? picked[0].weekday : null,
        busiest_day: plays ? { key: "2026-03-04", seconds: 3600 } : null, is_empty: plays === 0 && seconds === 0,
      },
      months, years,
    };
  };

  const state = {
    session: { signed_in: !!config.signedIn, profile: null, persistent: config.persistent !== false },
    player: { status: "idle", track: null, position: 0, duration: 0, volume: 1, shuffle: false, repeat: "off", queue_index: null, queue_len: 0, radio: null },
    queue: [],
    followedAlbums: [],
    history,
    statsEmpty: !!config.statsEmpty,
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
      if (config.loginPending) return;
      setTimeout(() => {
        state.session = { signed_in: true, profile: null, persistent: true };
        emit("pm-login", "success");
      }, 30);
    },
    logout: () => { state.session = { signed_in: false, profile: null, persistent: true }; state.queue = []; },
    library: async ({ offset, count }) => { await delay(config.pageDelay ?? 5); return page(library, offset, count); },
    playlists: async ({ offset, count }) => page(playlists, offset, count),
    playlist_tracks: async ({ playlist, offset, count }) => {
      const key = `${playlist.owner_id}_${playlist.playlist_id}`;
      if (!playlistTracks.has(key)) playlistTracks.set(key, library.slice(0, playlist.count).map((t) => ({ ...t, title: `${playlist.title}: ${t.title}` })));
      return page(playlistTracks.get(key), offset, count);
    },
    create_playlist: async ({ title, description }) => {
      await delay(10);
      const created = { playlist_id: 100 + playlists.length, owner_id: 7, title, description: description || null, count: 0, artwork_url: null, access_key: null };
      playlists.unshift(created);
      return created;
    },
    edit_playlist: async ({ playlist, title, description }) => {
      if (playlist.owner_id !== 7) throw { kind: "invalid", message: "Invalid request." };
      const found = playlists.find((p) => p.playlist_id === playlist.playlist_id && p.owner_id === playlist.owner_id);
      Object.assign(found, { title, description: description || null });
      return { ...found };
    },
    delete_playlist: async ({ playlist }) => {
      const index = playlists.findIndex((p) => p.playlist_id === playlist.playlist_id && p.owner_id === playlist.owner_id);
      playlists.splice(index, 1);
    },
    add_to_playlist: async ({ playlist, track }) => {
      if (playlist.owner_id !== 7) throw { kind: "invalid", message: "Invalid request." };
      const key = `${playlist.owner_id}_${playlist.playlist_id}`;
      playlistTracks.set(key, [...(playlistTracks.get(key) ?? []), track]);
      state.lastAdded = { playlist: playlist.title, track: track.title };
    },
    remove_from_playlist: async ({ playlist, track }) => {
      const key = `${playlist.owner_id}_${playlist.playlist_id}`;
      playlistTracks.set(key, (playlistTracks.get(key) ?? []).filter((t) => t.track_id !== track.track_id || t.owner_id !== track.owner_id));
    },
    search: async ({ query, offset, count }) => {
      await delay(query === "slow" ? 300 : 10);
      const all = query === "empty" ? [] : Array.from({ length: 120 }, (_, i) => makeTrack(5000 + i, { title: `${query} ${i + 1}` }));
      return page(all, offset, count);
    },
    player_state: () => state.player,
    play_tracks: ({ tracks, index }) => {
      state.queue = tracks; state.player.queue_index = index; state.player.radio = null; startCurrent(); emitQueue(); emitState();
    },
    // A queue that refills itself: the engine's own rules are tested in Rust,
    // here the window only has to ask for it and show that it is a mix.
    play_radio: ({ source, tracks, index }) => {
      if (config.radioFails) throw { kind: "invalid", message: "Invalid request." };
      state.queue = tracks; state.player.queue_index = index; state.player.radio = source.kind; startCurrent(); emitQueue(); emitState();
    },
    load_queue: ({ tracks, index, position }) => {
      state.queue = tracks; state.player.queue_index = index; state.player.radio = null; startCurrent();
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
    downloads: () => ({ tracks: state.downloads ?? [], bytes: (state.downloads ?? []).length * 4_200_000 }),
    download: async ({ track }) => {
      await delay(config.downloadDelay ?? 30);
      if (config.downloadFails) throw { kind: "stream", message: "Stream error: 403" };
      state.downloads = [{ ...track, stream_url: null }, ...(state.downloads ?? []).filter((t) => t.track_id !== track.track_id || t.owner_id !== track.owner_id)];
      return track;
    },
    remove_download: ({ trackId }) => {
      const before = (state.downloads ?? []).length;
      state.downloads = (state.downloads ?? []).filter((t) => `${t.owner_id}_${t.track_id}` !== trackId);
      return state.downloads.length !== before;
    },
    mix: async () => {
      await delay(20);
      if (config.forYouFails) throw { kind: "offline", message: "No internet connection." };
      return Array.from({ length: 100 }, (_, i) => makeTrack(7000 + i, { title: `Микс ${i + 1}` }));
    },
    catalog_mixes: async () => config.catalogMixes
      ? [{ id: "m7", title: "Спокойный вечер", subtitle: "", artwork_url: null, section_title: "Настроение", options: "{\"mood\":[\"calm\"]}" }]
      : [],
    stream_mix: async ({ mixId }) => {
      await delay(10);
      return Array.from({ length: 40 }, (_, i) => makeTrack(6000 + i, { title: `Мини ${mixId} ${i + 1}` }));
    },
    new_releases: async () => {
      await delay(10);
      if (config.releasesFail) throw { kind: "offline", message: "No internet connection." };
      if (config.noReleases) return [];
      return [
        ...Array.from({ length: 5 }, (_, i) => makeAlbum(30 + i, { title: `Релиз ${i + 1}`, year: 2025 })),
        makeAlbum(35, { title: "  " }),
      ];
    },
    recommendations: async () => { await delay(20); return Array.from({ length: 30 }, (_, i) => makeTrack(8000 + i, { title: `Рек ${i + 1}` })); },
    similar: async ({ track }) => {
      await delay(10);
      if (config.noSimilar) return [];
      return Array.from({ length: 20 }, (_, i) => makeTrack(9000 + i, { title: `Похожий ${i + 1}` }));
    },
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
      if (config.lyricsFail === "http") throw { kind: "http", message: "HTTP status 503." };
      if (config.lyricsFail) throw { kind: config.lyricsFail === "timeout" ? "timeout" : "offline", message: "No internet connection." };
      await delay(15);
      const n = track.track_id;
      if (n % 5 === 0) return null;
      if (n % 2 === 0) return { source: "VK", text: "первая строка\nвторая <b>строка</b>", lines: [] };
      const lines = Array.from({ length: 40 }, (_, i) => ({ time: 5 + i * 5, text: i === 3 ? "" : i === 0 ? config.hostileLyric ?? "Строка 0" : `Строка ${i}` }));
      return { source: "LRCLIB", text: lines.map((l) => l.text).join("\n"), lines };
    },
    output_devices: async () => { await delay(5); return config.devices ?? ["Динамики", "Наушники"]; },
    set_output_device: ({ name }) => { state.outputDevice = name; },
    set_close_to_tray: ({ on }) => { state.closeToTray = on; },
    app_info: () => ({ version: "0.1.0", has_tray: config.noTray ? false : true }),
    set_equalizer: ({ gains }) => { state.eq = gains; },
    set_visualizer: ({ on }) => { state.visualizer = on; },
    artwork_palette: ({ url }) => {
      if (config.paletteFails) throw { kind: "format", message: "no palette" };
      return url.includes("/6.") ? { primary: "#e02838", secondary: "#2838e0" } : { primary: "#28a050", secondary: "#a05028" };
    },
    set_fullscreen: ({ on }) => { state.fullscreen = on; },
    search_artists: async ({ query }) => {
      await delay(10);
      if (query === "empty") return [];
      return [
        { id: "art1", name: `${query} артист`, photo_url: "https://cdn.example/artist.jpg" },
        { id: "art2", name: "Исполнитель 2", photo_url: null },
      ];
    },
    find_artist: async ({ name }) => {
      await delay(5);
      const match = /^Исполнитель (\d+)$/.exec(name);
      return match && match[1] !== "0" ? { id: `art${match[1]}`, name, photo_url: null } : null;
    },
    artist_tracks: async ({ artistId, offset, count }) => {
      await delay(10);
      if (config.artistFails) throw { kind: "offline", message: "No internet connection." };
      const name = artistName(artistId);
      return page(Array.from({ length: 30 }, (_, i) => makeTrack(10000 + i, { title: `Хит ${i + 1}`, artist: name, artists: [{ id: artistId, name }] })), offset, count);
    },
    artist_albums: async ({ artistId, offset, count }) => {
      await delay(10);
      const name = artistName(artistId);
      const albums = Array.from({ length: 4 }, (_, i) => makeAlbum(i + 1, { artists: [{ id: artistId, name }] }));
      return { artist: { id: artistId, name, photo_url: "https://cdn.example/artist.jpg" }, albums: page(albums, offset, count) };
    },
    search_albums: async ({ query, offset, count }) => {
      await delay(10);
      return page(query === "empty" ? [] : Array.from({ length: 5 }, (_, i) => makeAlbum(20 + i, { title: `${query} альбом ${i + 1}` })), offset, count);
    },
    followed_albums: async ({ offset, count }) => page(state.followedAlbums, offset, count),
    album_tracks: async ({ album, offset, count }) => {
      await delay(10);
      if (config.albumDenied) throw { kind: "vk", message: "VK error 15: Could not load this album's tracks." };
      // Like the engine: a missing key is found, and VK describes the album.
      const described = { ...album, access_key: album.access_key ?? "resolved", count: 12, year: album.year ?? 2020,
        is_followed: state.followedAlbums.some((a) => sameAlbum(a, album)) };
      const tracks = Array.from({ length: 12 }, (_, i) => makeTrack(20000 + album.album_id * 100 + i, { title: `${album.title}: песня ${i + 1}` }));
      return { album: described, tracks: page(tracks, offset, count) };
    },
    follow_album: async ({ album, follow }) => {
      await delay(10);
      if (config.followFails) throw { kind: "vk", message: "VK error 15: Access denied" };
      state.followedAlbums = state.followedAlbums.filter((a) => !sameAlbum(a, album));
      if (follow) state.followedAlbums.unshift({ ...album, is_followed: true });
    },
  };

  Object.assign(commands, {
    friends: async ({ offset, count }) => { await delay(config.pageDelay ?? 5); return page(friends, offset, count); },
    friend_tracks: async ({ ownerId, offset, count }) => {
      await delay(10);
      if (ownerId === 12) throw privateAudio();
      return page(ownerId === 13 ? [] : friendTrackList(ownerId), offset, count);
    },
    friend_playlists: async ({ ownerId, offset, count }) => {
      await delay(10);
      if (ownerId === 12) throw privateAudio();
      return page(ownerId === 13 ? [] : friendPlaylistList(ownerId), offset, count);
    },
    history: () => state.history.map((entry) => ({ ...entry })),
    history_remove: ({ trackId }) => {
      const before = state.history.length;
      state.history = state.history.filter((entry) => `${entry.track.owner_id}_${entry.track.track_id}` !== trackId);
      return state.history.length !== before;
    },
    history_clear: () => { state.history = []; },
    listening_stats: async ({ scope, choice }) => {
      await delay(5);
      if (config.statsFail) throw { kind: "invalid", message: "Invalid request." };
      return statsFor(scope, choice);
    },
    reset_listening_stats: () => { state.statsEmpty = true; },
    set_utc_offset: ({ minutes }) => { state.utcOffset = minutes; },
  });

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
    calls, state, config, emit, emitState, emitQueue,
    last: (command) => [...calls].reverse().find((c) => c.command === command),
    count: (command) => calls.filter((c) => c.command === command).length,
    sessionExpired: () => { state.session = { signed_in: false, profile: null, persistent: true }; emit("pm-event", { type: "session_expired" }); },
  };
})();
