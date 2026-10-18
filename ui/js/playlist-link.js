/** Разобрать локатор плейлиста VK; никогда не извлекайте произвольный вставленный URL. */
export function playlistLink(raw) {
  const text=String(raw).trim();
  if (!text || text.length > 2048) throw new Error("invalid playlist link");
  const url=new URL(/^https?:\/\//i.test(text) ? text : `https://${text}`);
  if (!["vk.com","m.vk.com","vk.ru","m.vk.ru","music.vk.com","music.vk.ru"].includes(url.hostname.toLowerCase()) || url.username || url.password || url.port) throw new Error("invalid playlist host");
  const path=url.searchParams.get("z") ?? url.pathname;
  const match=path.match(/(?:audio_playlist|\/playlist\/)(-?\d+)_(\d+)(?:_([a-z\d]+))?(?:$|[/?&#])/i);
  if (!match) throw new Error("invalid playlist locator");
  const owner=Number(match[1]),id=Number(match[2]);
  if (!Number.isSafeInteger(owner) || owner === 0 || !Number.isSafeInteger(id) || id <= 0 || (match[3]?.length ?? 0) > 256) throw new Error("invalid playlist id");
  return { owner_id:owner,album_id:id,access_key:match[3] ?? null,title:"",artists:[],count:0,is_followed:false };
}
