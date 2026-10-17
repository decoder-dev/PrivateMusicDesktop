// Product naming and generated mix colours ported from the mobile
// Models/MusicMix.swift and Models/MixBackdrop.swift.
export function mixName(raw = "") {
  if (["мой микс","my mix","vk микс","вк микс","vk mix","микс vk","микс вк","mix vk"].includes(raw.trim().toLowerCase())) return "Selena";
  return raw.replace(/(^|[^\p{L}\d])(?:VK|ВК)(?=$|[^\p{L}\d])/giu, "$1Selena");
}
function hsb(hue, brightness) {
  const sector = ((hue%1)+1)%1*6, i=Math.floor(sector)%6, f=sector-Math.floor(sector), low=brightness*0.28, fall=brightness*(1-0.72*f), rise=brightness*(1-0.72*(1-f));
  const rgb=[[brightness,rise,low],[fall,brightness,low],[low,brightness,rise],[low,fall,brightness],[rise,low,brightness],[brightness,low,fall]][i];
  return "#"+rgb.map((v)=>Math.round(v*255).toString(16).padStart(2,"0")).join("");
}
export function mixColors(id) {
  let seed=5381;
  for (const char of id) seed=(Math.imul(seed,33)+char.codePointAt(0))>>>0;
  const base=(seed%360)/360;
  return [[0,0.98],[0.14,0.78],[0.5,0.92],[0.64,0.62]].map(([shift,brightness])=>hsb(base+shift,brightness));
}
