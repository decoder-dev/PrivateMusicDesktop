import { isAndroid } from "./mobile/layout.js";
import { api } from "./api.js";
import { listen } from "./bridge.js";
import { h } from "./dom.js";
import { own } from "./store.js";
import { addStrings, t } from "./i18n.js";
addStrings({ ru: {
 "update.title":"Обновления", "update.check":"Проверить обновления", "update.download":"Скачать обновление", "update.cancel":"Отменить загрузку", "update.open":"Открыть установщик", "update.folder":"Показать установщик", "update.idle":"Новые версии загружаются прямо в приложении.", "update.checking":"Проверяем новую версию…", "update.current":"Установлена актуальная версия", "update.available":"Доступна версия {version}", "update.downloading":"Загружаем {version}", "update.verifying":"Проверяем целостность файла…", "update.ready":"Обновление загружено и проверено", "update.cancelled":"Загрузка отменена. Можно попробовать снова.", "update.error":"Не удалось завершить обновление", "update.access":"Доступ к закрытому репозиторию", "update.access.note":"Для закрытого репозитория нужен токен GitHub с доступом Contents: Read к decoder-dev/PrivateMusicDesktop. Он хранится в системном хранилище секретов отдельно от аккаунта VK.", "update.token":"Токен GitHub", "update.save":"Сохранить доступ", "update.forget":"Удалить токен", "update.saved":"Доступ сохранён", "update.notes":"Что нового", "update.err.access":"GitHub не разрешил доступ. Проверьте токен и разрешение на чтение репозитория. Для публичного репозитория токен не нужен.", "update.err.network":"Не удалось связаться с GitHub. Проверьте сеть и повторите.", "update.err.checksum":"Проверка контрольной суммы не пройдена. Файл не будет открыт — повторите загрузку.", "update.err.size":"Размер файла не совпал с релизом. Повторите загрузку.", "update.err.disk":"Не удалось сохранить файл. Проверьте свободное место и доступ к папке приложения.", "update.err.secret":"Системное хранилище секретов недоступно. Токен не сохранён.", "update.err.platform":"В релизе нет установщика для этой системы.", "update.err.release":"Не удалось прочитать данные релиза. Повторите проверку позже.", "update.err.rate":"GitHub временно ограничил запросы. Попробуйте позже.", "update.err.token":"Проверьте формат токена GitHub.", "update.err.busy":"Обновление уже выполняется.", "update.err.installer":"Не удалось открыть установщик.", "update.err.worker":"Не удалось выполнить обновление. Попробуйте снова.", "window.mode":"Размер окна", "window.medium":"Среднее", "window.maximized":"Развернуть", "window.fullscreen":"Полный экран", "window.note":"Фиксированный размер без растягивания за края. F11 — полный экран, Esc — выход из него."
}, en: {
 "update.title":"Updates", "update.check":"Check for updates", "update.download":"Download update", "update.cancel":"Cancel download", "update.open":"Open installer", "update.folder":"Show installer", "update.idle":"New versions download directly in the app.", "update.checking":"Checking for updates…", "update.current":"You have the latest version", "update.available":"Version {version} is available", "update.downloading":"Downloading {version}", "update.verifying":"Verifying file integrity…", "update.ready":"Update downloaded and verified", "update.cancelled":"Download cancelled. You can try again.", "update.error":"Update could not be completed", "update.access":"Private repository access", "update.access.note":"A private repository requires a GitHub token with Contents: Read access to decoder-dev/PrivateMusicDesktop. It is kept in the system secret store separately from your VK account.", "update.token":"GitHub token", "update.save":"Save access", "update.forget":"Remove token", "update.saved":"Access saved", "update.notes":"What's new", "update.err.access":"GitHub denied access. Check your token and repository read permission. Public repositories do not require a token.", "update.err.network":"Could not reach GitHub. Check your connection and retry.", "update.err.checksum":"Checksum verification failed. The file will not be opened. Download it again.", "update.err.size":"File size does not match the release. Download it again.", "update.err.disk":"Could not save the file. Check disk space and access to the app folder.", "update.err.secret":"The system secret store is unavailable. The token was not saved.", "update.err.platform":"This release has no installer for your system.", "update.err.release":"Could not read the release. Check again later.", "update.err.rate":"GitHub temporarily limited requests. Try again later.", "update.err.token":"Check your GitHub token format.", "update.err.busy":"An update is already running.", "update.err.installer":"Could not open the installer.", "update.err.worker":"The update could not be completed. Please retry.", "window.mode":"Window size", "window.medium":"Medium", "window.maximized":"Maximize", "window.fullscreen":"Fullscreen", "window.note":"Fixed sizes without edge resizing. F11 enters fullscreen; Esc leaves it."
}});
addStrings({ru:{
 "update.install":"Установить обновление", "update.installing":"Устанавливаем обновление… Приложение перезапустится.", "update.installed":"Обновление установлено", "update.err.install_permission":"Разрешите установку из Private Music на открывшемся экране Android, вернитесь и нажмите «Установить обновление».", "update.err.verification":"Не удалось скачать или проверить подпись обновления. Проверьте сеть и повторите загрузку. Установка не начата.", "update.idle":"Проверка, загрузка и установка запускаются одной кнопкой. На Android установку подтверждает система."
},en:{
 "update.install":"Install update", "update.installing":"Installing update… The app will restart.", "update.installed":"Update installed", "update.err.install_permission":"Allow installation from Private Music in Android settings, then return and press Install update.", "update.err.verification":"Could not download or verify the update signature. Check the network and retry. Installation has not started.", "update.idle":"One button checks, downloads and installs updates. Android asks you to confirm installation."
}});
const bytes = n => `${(Math.max(0, n || 0) / 1048576).toFixed(1)} MB`;
export function createUpdater() {
 let state = {phase:"idle"}, pending=false, disposed=false, revision=0, actionError=null, cancelRequested=false;
 const title=h("strong"), message=h("div",{class:"set-note",role:"status","aria-live":"polite"});
 const error=h("p",{class:"update-error",role:"alert",hidden:true});
 const progress=h("progress",{max:100,value:0,"aria-label":t("update.download"),hidden:true});
 const amount=h("span",{class:"set-note"});
 const check=h("button",{class:"btn",type:"button",text:t("update.check"),onclick:()=>runUpdate(api.updateCheck)});
 const download=h("button",{class:"btn primary",type:"button",text:t("update.download"),hidden:true,onclick:()=>runUpdate(api.updateDownload)});
 const cancel=h("button",{class:"btn",type:"button",text:t("update.cancel"),hidden:true,onclick:async()=>{cancelRequested=true;cancel.disabled=true;try{await api.updateCancel();}catch(e){showError(e);} }});
 const open=h("button",{class:"btn primary",type:"button",text:t("update.install"),hidden:true,onclick:()=>run(api.updateOpen,false)});
 const token=h("input",{class:"field",type:"password",autocomplete:"off",spellcheck:"false",maxlength:512,"aria-label":t("update.token"),placeholder:t("update.token")});
 const save=h("button",{class:"btn",type:"submit",text:t("update.save")});
 const forget=h("button",{class:"btn",type:"button",text:t("update.forget"),hidden:true,onclick:()=>run(()=>api.updateAccess(""))});
 const access=h("details",{class:"update-access"},h("summary",{text:t("update.access")}),h("p",{class:"set-note",text:t("update.access.note")}),h("form",{class:"set-inline",onsubmit:e=>{e.preventDefault();const value=token.value;token.value="";if(value.trim())run(()=>api.updateAccess(value));}},token,save,forget));
 const notesText=h("pre",{class:"update-notes"});
 const notes=h("details",{hidden:true},h("summary",{text:t("update.notes")}),notesText);
 const element=h("div",{class:"update-panel"},title,message,error,progress,amount,h("div",{class:"set-inline update-actions"},check,download,cancel,open),notes,access);
 function showError(value){actionError=value;const code=typeof value==="string"?value:"worker";error.textContent=t(`update.err.${["access","network","checksum","size","disk","secret","platform","release","rate","token","busy","installer","worker","install_permission","verification"].includes(code)?code:"worker"}`);error.hidden=false;}
 function render(next){if(disposed)return;state=next??state;
  const busy=["checking","downloading","verifying","installing"].includes(state.phase);
  element.setAttribute("aria-busy",String(busy));
  title.textContent=state.current?`Private Music ${state.current}`:"Private Music";
  message.textContent=t(`update.${state.phase}`,{version:state.version??""});
  check.disabled=busy||pending; save.disabled=forget.disabled=busy||pending;
  download.hidden=!(state.version&&["available","cancelled","error"].includes(state.phase));download.disabled=busy||pending;
  cancel.hidden=!["checking","downloading","verifying"].includes(state.phase);cancel.disabled=false;
  open.hidden=state.phase!=="ready";open.disabled=pending;
  progress.hidden=!["downloading","verifying"].includes(state.phase);
  progress.value=state.total?Math.min(100,(state.downloaded||0)/state.total*100):0;
  amount.textContent=progress.hidden?"":`${bytes(state.downloaded)} / ${bytes(state.total)}`;
  notes.hidden=!state.notes;notesText.textContent=state.notes??"";
  forget.hidden=!state.access_saved;
  access.querySelector("summary").textContent=t(state.access_saved?"update.saved":"update.access");
  error.hidden=!(state.error||actionError);if(state.error||actionError)showError(state.error||actionError);
  if(state.error==="access")access.open=true;
  access.hidden=isAndroid();
 }
 async function runUpdate(action){
  cancelRequested=false;
  await run(async()=>{
   let next=await action();
   if(next?.phase==="available" && !cancelRequested)next=await api.updateDownload();
   if(next?.phase==="ready" && !cancelRequested){
    render(next);
    await api.updateOpen();
   }
   return next;
  });
 }
 async function run(action, snapshot=true){if(pending)return;pending=true;actionError=null;error.hidden=true;render();try{const next=await action();if(snapshot&&next){revision++;actionError=null;render(next);}}catch(e){if(["checking","downloading","verifying","installing"].includes(state.phase))state={...state,phase:"error"};showError(e);}finally{pending=false;render();}}
 let unlisten=null;
 listen("pm-update",next=>{revision++;actionError=null;render(next);}).then(fn=>{if(disposed)fn();else unlisten=fn;}).catch(()=>{});
 own(()=>{disposed=true;unlisten?.();token.value="";});
 render();
 return {element,async show(){const mine=revision;try{const next=await api.updateState();if(mine===revision)render(next);}catch(e){showError(e);}}};
}
