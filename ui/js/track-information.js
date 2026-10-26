import {addStrings} from './i18n.js';
import {copyLyricsText} from './lyrics-copy.js';
addStrings({ru:{'track.copyInfo':'Скопировать название и исполнителя','track.infoCopied':'Название и исполнитель скопированы','track.infoCopyFailed':'Не удалось скопировать название и исполнителя'},en:{'track.copyInfo':'Copy track info','track.infoCopied':'Track info copied','track.infoCopyFailed':'Could not copy track info'}});
export function trackInformation(track) {
  return [track?.artist,track?.title].filter(value=>typeof value==='string').map(value=>value.trim()).filter(Boolean).join(' — ');
}
export function copyTrackInformation(track) {
  return copyLyricsText(trackInformation(track),()=>true,'track.infoCopied','track.infoCopyFailed');
}
