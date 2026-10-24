import { ft } from "./feature-i18n.js";
import { h } from "./dom.js";
import { api } from "./api.js";
import { toast } from "./toast.js";
import { errorText, addStrings, t } from "./i18n.js";
addStrings({"ru": {"audio.preamp": "Предусиление", "audio.preampNote": "От −12 до +6 дБ. Подъём может уменьшать запас до перегруза.", "audio.compression": "Сжатие динамического диапазона", "audio.compressionNote": "Смягчает громкие фрагменты — удобно при тихом прослушивании.", "audio.spatial": "Расширение стерео", "audio.spatialNote": "Расширяет звучание в стереонаушниках.", "audio.width": "Ширина стерео", "audio.db": "дБ"}, "en": {"audio.preamp": "Preamp", "audio.preampNote": "From −12 to +6 dB. Boosting reduces headroom.", "audio.compression": "Dynamic range compression", "audio.compressionNote": "Softens loud passages for quiet listening.", "audio.spatial": "Stereo widening", "audio.spatialNote": "Widens the sound in stereo headphones.", "audio.width": "Stereo width", "audio.db": "dB"}});
addStrings({ru:{"audio.advance":"Пропускать недоступные треки","audio.advanceNote":"После попыток восстановления переходить к следующему треку. Сбой сети не пропускает очередь."},en:{"audio.advance":"Skip unavailable tracks","audio.advanceNote":"Advance after recovery attempts. Network failures do not skip the queue."}});
addStrings({ru:{"audio.bluetooth":"Продолжать при подключении Bluetooth","audio.pauseZero":"Пауза при нулевой громкости телефона"},en:{"audio.bluetooth":"Resume when Bluetooth connects","audio.pauseZero":"Pause at zero system volume"}});
addStrings({ru:{"audio.quality":"Высокое качество","audio.qualityNote":"Лучшее доступное качество. Выключите для экономии трафика, если у трека есть варианты качества. Применяется со следующего трека."},en:{"audio.quality":"High quality","audio.qualityNote":"The best available quality. Turn off to save data when the track offers quality options. Applies to the next track."}});
addStrings({ru:{"audio.gaplessNote":"Следующая композиция начинается сразу после текущей.","audio.fadeNote":"Смешивает конец текущего трека с началом следующего. Требуется предзагрузка.","audio.normalizeNote":"Сглаживает разницу громкости между треками. Применяется со следующего запуска композиции."},en:{"audio.gaplessNote":"Starts the next track immediately after the current one.","audio.fadeNote":"Blends the end of the current track into the next. Requires preloading.","audio.normalizeNote":"Smooths volume differences between tracks. Applies the next time a track starts."}});
export function createAudioOptions(settings, onChange) {
  let audio={preamp:0,compression:false,spatial:false,spatialIntensity:0.35,...settings.audio};
  let pending=null,sending=false;
  async function processing(patch) {
    audio={...audio,...patch};pending={...audio};renderProcessing();
    if(sending)return;
    sending=true;
    try {
      while(pending) {
        const current=pending;pending=null;
        try {await api.setAudioProcessing(current);onChange({audio:current});}
        catch(error) {toast(errorText(error),{error:true});if(!pending){audio={...settings.audio};renderProcessing();}}
      }
    }finally{sending=false;}
  }
  const preamp=h("input",{type:"range",min:-12,max:6,step:0.5,"aria-label":t("audio.preamp"),oninput:()=>{preampValue.textContent=`${preamp.value} ${t("audio.db")}`;},onchange:()=>processing({preamp:Number(preamp.value)})});
  const preampValue=h("output",{class:"audio-value"});
  const compression=h("input",{type:"checkbox",role:"switch","aria-label":t("audio.compression"),onchange:()=>processing({compression:compression.checked})});
  const spatial=h("input",{type:"checkbox",role:"switch","aria-label":t("audio.spatial"),onchange:()=>processing({spatial:spatial.checked})});
  const width=h("input",{type:"range",min:0,max:1,step:0.05,"aria-label":t("audio.width"),oninput:()=>{widthValue.textContent=`${Math.round(Number(width.value)*100)}%`;},onchange:()=>processing({spatialIntensity:Number(width.value)})});
  const widthValue=h("output",{class:"audio-value"});
  function renderProcessing() {
    preamp.value=String(audio.preamp);preampValue.textContent=`${audio.preamp} ${t("audio.db")}`;
    compression.checked=audio.compression;spatial.checked=audio.spatial;
    width.value=String(audio.spatialIntensity);width.disabled=!audio.spatial;widthValue.textContent=`${Math.round(audio.spatialIntensity*100)}%`;
  }
  async function save(patch) {
    const next = { ...settings, ...patch };
    try {
      if ("normalization" in patch)
        await api.setNormalization(next.normalization);
      else
        await api.setTransitions(next.prefetch, next.gapless, next.crossfade);
      onChange(patch);
      render();
    } catch (error) {
      toast(errorText(error), { error: true });
      render();
    }
  }
  async function playbackPreferences(patch) {
    const next={...settings,...patch};
    try{await api.setPlaybackPreferences(next.resumeBluetooth,next.pauseAtZero);onChange(patch);}catch(error){toast(errorText(error),{error:true});}
    resumeBluetooth.checked=settings.resumeBluetooth;pauseAtZero.checked=settings.pauseAtZero;
  }
  const resumeBluetooth=h("input",{type:"checkbox",role:"switch","aria-label":t("audio.bluetooth"),onchange:()=>playbackPreferences({resumeBluetooth:resumeBluetooth.checked})});
  const pauseAtZero=h("input",{type:"checkbox",role:"switch","aria-label":t("audio.pauseZero"),onchange:()=>playbackPreferences({pauseAtZero:pauseAtZero.checked})});
  const quality=h("input",{type:"checkbox",role:"switch","aria-label":t("audio.quality"),onchange:async()=>{try{await api.setHighQuality(quality.checked);onChange({highQuality:quality.checked});}catch(error){quality.checked=settings.highQuality;toast(errorText(error),{error:true});}}});
  const advance=h("input",{type:"checkbox",role:"switch","aria-label":t("audio.advance"),onchange:async()=>{try{await api.setAdvanceOnError(advance.checked);onChange({advanceOnError:advance.checked});}catch(error){advance.checked=settings.advanceOnError;toast(errorText(error),{error:true});}}});
  const preload = h("input", {
    type: "checkbox",
    role:"switch",
    "aria-label": ft("Предзагрузка следующего трека"),
    onchange: () => save({ prefetch: preload.checked }),
  });
  const seamless = h("input", {
    type: "checkbox",
    role:"switch",
    "aria-label": ft("Без пауз между треками"),
    onchange: () => save({ gapless: seamless.checked }),
  });
  const normalize = h("input", {
    type: "checkbox",
    role:"switch",
    "aria-label": ft("Выравнивание громкости"),
    onchange: () => save({ normalization: normalize.checked }),
  });
  const fade = h(
    "select",
    {
      class: "field",
      "aria-label": ft("Плавный переход"),
      onchange: () => save({ crossfade: Number(fade.value) }),
    },
    [0,...Array.from({length:20},(_,index)=>(index+1)/2)].map((value) =>
      h("option", { value, text: value ? `${value} ${ft("с")}` : ft("Выключен") }),
    ),
  );
  const row = (label, control, note) =>
    h(
      control.matches?.('input[type="checkbox"]') ? "label" : "div",
      { class: "set-row" },
      h(
        "div",
        { class: "set-label" },
        h(
          "div",
          { class: "set-text" },
          label,
          h("div", { class: "set-note", text: note }),
        ),
      ),
      control,
    );
  const element = h(
    "div",
    { class: "audio-options" },
    row(t("audio.quality"),quality,t("audio.qualityNote")),
    row(t("audio.bluetooth"),resumeBluetooth),
    row(t("audio.pauseZero"),pauseAtZero),
    row(t("audio.advance"),advance,t("audio.advanceNote")),
    row(t("audio.preamp"),h("div",{class:"audio-range"},preamp,preampValue),t("audio.preampNote")),
    row(t("audio.compression"),compression,t("audio.compressionNote")),
    row(t("audio.spatial"),spatial,t("audio.spatialNote")),
    row(t("audio.width"),h("div",{class:"audio-range"},width,widthValue)),
    row(
      ft("Предзагрузка следующего трека"),
      preload,
      ft("Готовит звук заранее. Дополнительный трафик и память; выключите на ограниченном интернете."),
    ),
    row(
      ft("Без пауз между треками"),
      seamless,
      t("audio.gaplessNote"),
    ),
    row(
      ft("Плавный переход"),
      fade,
      t("audio.fadeNote"),
    ),
    row(
      ft("Выравнивание громкости"),
      normalize,
      t("audio.normalizeNote"),
    ),
  );
  function render() {
    resumeBluetooth.checked=settings.resumeBluetooth;pauseAtZero.checked=settings.pauseAtZero;
    quality.checked=settings.highQuality;
    advance.checked=settings.advanceOnError;
    preload.checked = settings.prefetch;
    seamless.checked = settings.gapless;
    normalize.checked = settings.normalization;
    fade.value = String(settings.crossfade);
    fade.disabled = !settings.prefetch;
    seamless.disabled = !settings.prefetch;
  }
  renderProcessing();
  render();
  return element;
}
