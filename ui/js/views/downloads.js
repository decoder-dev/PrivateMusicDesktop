import { createTracksScreen } from "../tracks-screen.js";
import { t, tn, addStrings, errorText } from "../i18n.js";
import { h } from "../dom.js";
import { icon } from "../icons.js";
import { confirmDialog } from "../dialog.js";
import { api } from "../api.js";
import { subscribe } from "../store.js";
import { downloads, refreshDownloads, formatSize, downloadJob, cancelDownloads, downloadAll } from "../downloads.js";

addStrings({ ru:{ "dl.clear":"Очистить загрузки", "dl.confirm":"Удалить все скачанные аудиофайлы с этого компьютера? Музыка в VK сохранится.", "dl.stop":"Остановить после текущего", "dl.retry":"Повторить неудачные", "dl.progress":"Скачано: {done}/{total}. Ошибок: {failed}", "dl.stopping":"Остановка после текущего трека…" }, en:{ "dl.clear":"Clear downloads", "dl.confirm":"Remove all downloaded audio from this computer? Your VK library will be kept.", "dl.stop":"Stop after current", "dl.retry":"Retry failed", "dl.progress":"Downloaded: {done}/{total}. Failed: {failed}", "dl.stopping":"Stopping after the current track…" } });

/** Everything on this computer, newest first. */
export function createDownloadsView({ play, onTrackAction, toast }) {
  const screen = createTracksScreen({
    title: t("dl.title"),
    filterable: true,
    hooks: { emptyText: t("dl.empty"), art: "download", onPlay: play, onAction: onTrackAction },
  });

  // A summary card above the list: how much room the offline music takes.
  const sizeText = h("div", { class: "dl-size tnum" });
  const countText = h("div", { class: "dl-count" });
  const summary = h("div", { class: "dl-summary", hidden: true },
    h("span", { class: "dl-tile", "aria-hidden": "true" }, icon("download", 22)),
    h("div", { class: "dl-text" }, sizeText, countText));
  screen.element.insertBefore(summary, screen.element.querySelector(".status"));

  const jobText=h("div",{class:"set-note",role:"status","aria-live":"polite"});
  const stop=h("button",{class:"btn",type:"button",text:t("dl.stop"),onclick:cancelDownloads});
  const retry=h("button",{class:"btn",type:"button",text:t("dl.retry"),onclick:async()=>{try {await downloadAll(downloadJob()?.errors.map(item=>item.track) ?? []);}catch(error){toast(errorText(error),{error:true});}}});
  const clear=h("button",{class:"btn danger-text",type:"button",text:t("dl.clear"),onclick:async()=>{
    if (!(await confirmDialog({text:t("dl.confirm"),confirm:t("dl.clear"),danger:true}))) return;
    clear.disabled=true;
    try { for (const track of downloads().tracks.slice()) await api.removeDownload(`${track.owner_id}_${track.track_id}`); }
    catch(error){toast(errorText(error),{error:true});}
    finally{await refreshDownloads();clear.disabled=false;}
  }});
  const jobPanel=h("div",{class:"download-job",hidden:true},jobText,h("div",{class:"set-inline"},stop,retry));
  summary.append(clear);
  screen.element.insertBefore(jobPanel,summary);
  function renderJob(){
    const job=downloadJob();
    jobPanel.hidden=!job || !job.running && !job.failed;
    if (!job) return;
    jobText.textContent=job.cancelled && job.running ? t("dl.stopping") : t("dl.progress",{done:job.completed,total:job.total,failed:job.failed});
    stop.hidden=!job.running; stop.disabled=job.cancelled;
    retry.hidden=job.running || !job.failed;
    clear.disabled=job.running;
  }
  subscribe("downloadJob",renderJob);
  renderJob();

  function render() {
    const { tracks, bytes } = downloads();
    screen.setTracks(tracks);
    summary.hidden = tracks.length === 0;
    sizeText.textContent = formatSize(bytes);
    countText.textContent = `${tn("library.count", tracks.length)} · ${t("dl.offline")}`;
    if (tracks.length) screen.element.querySelector(".meta").textContent += ` · ${t("dl.size", { size: formatSize(bytes) })}`;
  }

  subscribe("downloads", render);
  return {
    element: screen.element,
    show() { refreshDownloads(); renderJob(); },
  };
}
