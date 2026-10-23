
// An explicit source schema keeps playback addresses and unrelated state out of bookmarks.
export function radioSnapshot(value) {
  if (!value || !Number.isInteger(value.next_offset) || value.next_offset < 0 || value.next_offset > 1_000_000) return null;
  const source = value.source;
  if (!source) return null;
  let saved;
  if (source.kind === "personal" || source.kind === "catalog") {
    let options = null;
    if (source.options != null) {
      if (typeof source.options !== "string" || source.options.length > 8192) return null;
      try { const parsed=JSON.parse(source.options);if(!parsed || typeof parsed!=="object" || Array.isArray(parsed))return null;options=JSON.stringify(parsed); } catch {return null;}
    }
    saved = {kind:source.kind, options};
    if(source.kind === "catalog") {
      if(typeof source.mix_id!=="string" || !source.mix_id.trim() || source.mix_id.length>256 || /[\x00-\x1f\x7f]/.test(source.mix_id))return null;
      saved.mix_id=source.mix_id;
    }
  } else if(source.kind === "similar") {
    const seed=source.seed;
    if(!seed || !Number.isSafeInteger(seed.owner_id) || seed.owner_id===0 || !Number.isSafeInteger(seed.track_id) || seed.track_id<=0)return null;
    if(typeof seed.title!=="string" || typeof seed.artist!=="string" || !Number.isInteger(seed.duration) || seed.duration<0)return null;
    const fields=["owner_id","track_id","title","artist","album_title","duration","artwork_url","access_key","lyrics_id","is_hq","track_code","artists","album"];
    saved={kind:"similar",seed:{...Object.fromEntries(fields.filter(key=>key in seed).map(key=>[key,seed[key]])),stream_url:null}};
  } else return null;
  return {source:saved,next_offset:value.next_offset};
}
