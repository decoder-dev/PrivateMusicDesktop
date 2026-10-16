// Artist and album pages open from anywhere: track rows, the track menu, the
// player bar and the full-screen view. main.js supplies the router; this
// module only knows what a track can lead to.

let router = null;

export function setRouter(next) { router = next; }

/** Artists VK linked on the track (they carry an id). */
export const linkedArtists = (track) => (track?.artists ?? []).filter((artist) => artist.id);

/** Whether the track names an artist: linked, or at least by name. */
export const hasArtist = (track) => linkedArtists(track).length > 0 || !!track?.artist?.trim();

export const hasAlbum = (track) => !!track?.album;

/** `{ id, name }`; a missing id is looked up by name on the artist page. */
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

/** The album a track belongs to, as far as the track knows it. */
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
