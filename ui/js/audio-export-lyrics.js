export function exportLyrics(lyrics) {
  if(!lyrics?.text)return null;
  const lines=Array.isArray(lyrics.lines)?lyrics.lines:[];
  const lrc=lines.filter(line=>Number.isFinite(line.time) && line.time>=0 && line.time<=86400).map(line=>{
    const centiseconds=Math.round(line.time*100),minutes=Math.floor(centiseconds/6000),seconds=Math.floor(centiseconds/100)%60;
    return `[${String(minutes).padStart(2,"0")}:${String(seconds).padStart(2,"0")}.${String(centiseconds%100).padStart(2,"0")}]${String(line.text).replace(/[\r\n]+/g," ")}`;
  }).join("\n");
  return {text:lyrics.text,lrc:new TextEncoder().encode(lrc).length<=200000?lrc:""};
}
