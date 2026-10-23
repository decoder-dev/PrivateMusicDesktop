// Only public VK locators are shared. Playback URLs and session fields are never serialised.
const validOwner = value => Number.isSafeInteger(value) && value !== 0;
const validId = value => Number.isSafeInteger(value) && value > 0;
export function trackShareLink(track) {
  return track && validOwner(track.owner_id) && validId(track.track_id)
    ? `https://vk.com/audio${track.owner_id}_${track.track_id}` : null;
}
export function collectionShareLink(kind, item) {
  if (!["playlist", "album"].includes(kind) || !item || !validOwner(item.owner_id)) return null;
  const id = kind === "playlist" ? item.playlist_id : item.album_id;
  if (!validId(id)) return null;
  const url = new URL(`https://vk.com/music/${kind}/${item.owner_id}_${id}`);
  if (item.access_key) {
    if (typeof item.access_key !== "string" || !/^[a-z\d]{1,256}$/i.test(item.access_key)) return null;
    url.searchParams.set("access_key", item.access_key);
  }
  return url.href;
}
