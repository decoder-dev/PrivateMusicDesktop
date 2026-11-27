// VK rejects some undocumented mix_options shapes with error 8. Match the iOS
// client: retry the tile untuned and use the working options for continuation.
export function mixOptionsRejected(error) {
  const message = String(error?.message ?? error ?? "");
  return error?.kind === "invalid" || (error?.kind === "vk" && /error\s+8|bad request/i.test(message));
}

export async function loadMixTracks(mix, stream) {
  let options = mix.options ?? null;
  let tracks;
  try {
    tracks = await stream(mix.id, options);
  } catch (error) {
    if (!options || !mixOptionsRejected(error)) throw error;
    tracks = await stream(mix.id, null);
    options = null;
  }
  if (!tracks.length && options) {
    tracks = await stream(mix.id, null);
    options = null;
  }
  return { tracks, options };
}
