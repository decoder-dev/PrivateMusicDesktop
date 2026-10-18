// Страницы исполнителя и альбома открываются откуда угодно: строки треков, меню треков, панель
// проигрывателя и полноэкранный режим. main.js поставляет маршрутизатор; этот модуль знает только то, к
// чему может привести трек.

let router = null;

export function setRouter(next) { router = next; }

/** Исполнители VK, связанные с треком (у них есть идентификатор). */
export const linkedArtists = (track) => (track?.artists ?? []).filter((artist) => artist.id);

/** Имя ли в треке исполнителя: по ссылке или хотя бы по имени. */
export const hasArtist = (track) => linkedArtists(track).length > 0 || !!track?.artist?.trim();

export const hasAlbum = (track) => !!track?.album;

/** `{ id, name }`; отсутствующий идентификатор ищется по имени на странице исполнителя. */
export function openArtist(artist) {
  if (artist?.id || artist?.name?.trim()) router?.openArtist({ id: artist.id ?? null, name: artist.name ?? "" });
}

export function openArtistOf(track) {
  const [first] = linkedArtists(track);
  openArtist(first ?? { id: null, name: track?.artist ?? "" });
}

export function openAlbum(album) {
  if (album) router?.openAlbum(album);
}

/** Альбом, которому принадлежит трек, насколько он известен. */
export function albumOf(track) {
  const ref = track.album;
  return {
    album_id: ref.album_id, owner_id: ref.owner_id,
    title: ref.title ?? track.album_title ?? "", description: null, count: 0,
    artwork_url: track.artwork_url ?? null, access_key: ref.access_key ?? null,
    artists: linkedArtists(track).length ? linkedArtists(track) : [{ id: null, name: track.artist }],
    year: null, is_followed: false, follow_hash: null,
  };
}

export function openAlbumOf(track) {
  if (hasAlbum(track)) openAlbum(albumOf(track));
}
