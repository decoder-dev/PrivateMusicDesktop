import { ft } from "./feature-i18n.js";
import { h } from "./dom.js";
import { api } from "./api.js";
import { toast } from "./toast.js";
import { errorText } from "./i18n.js";
export function createAudioOptions(settings, onChange) {
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
    [0, 1, 2, 3, 4, 5, 6, 8].map((value) =>
      h("option", { value, text: value ? `${value} ${ft("с")}` : ft("Выключен") }),
    ),
  );
  const row = (label, control, note) =>
    h(
      "div",
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
    row(
      ft("Предзагрузка следующего трека"),
      preload,
      ft("Готовит звук заранее. Дополнительный трафик и память; выключите на ограниченном интернете."),
    ),
    row(
      ft("Без пауз между треками"),
      seamless,
      ft("Использует один аудиовыход. При недоступном следующем треке сработает обычное восстановление."),
    ),
    row(
      ft("Плавный переход"),
      fade,
      ft("Сведение конца и начала композиции. Короткие треки используют меньшую длительность. Требуется предзагрузка."),
    ),
    row(
      ft("Выравнивание громкости"),
      normalize,
      ft("Применяется со следующего запуска трека. Анализ с K-взвешиванием; полный результат сохраняется после успешного декодирования без перемотки. Первое прослушивание использует предварительную оценку."),
    ),
  );
  function render() {
    preload.checked = settings.prefetch;
    seamless.checked = settings.gapless;
    normalize.checked = settings.normalization;
    fade.value = String(settings.crossfade);
    fade.disabled = !settings.prefetch;
    seamless.disabled = !settings.prefetch;
  }
  render();
  return element;
}
