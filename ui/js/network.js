// Проблема с подключением, способ решения проблемы приложением iOS (VPNHelpPolicy, VPNWarningCard):
// когда VK не может быть достигнут, скажите так, предложите проверить VPN (предложение, никогда не
// утверждайте, что он был обнаружен), предложите повторную попытку и повторите попытку самостоятельно,
// когда сеть восстановится.
//
// Здесь все зависит от нескольких точек перехвата: `decorateStatus` (вызывается, когда на экране
// отображается ошибка), `handleBackendEvent` (события механизма), `setReloader` (как перезагрузить
// текущий экран) и `initNetwork`.

import { h } from "./dom.js";
import { t, addStrings } from "./i18n.js";
import { api } from "./api.js";
import { icon } from "./icons.js";
import { toast } from "./toast.js";
import { store } from "./store.js";

addStrings({
  ru: {
    "error.network": "Сбой сети. Проверьте подключение.",
    "net.failed": "Не удаётся подключиться к VK",
    "net.hint": "Если используете VPN, попробуйте отключить его (или включить, если VK недоступен) и повторить.",
    "net.retry": "Повторить",
    "net.how": "Как проверить VPN",
    "net.dialog.intro": "VPN меняет сеть, из которой вы заходите в VK, а ссылки на музыку привязаны к той сети, где они выданы. Private Music сам обновляет ссылки и соединения, но если музыка не возвращается, проверьте VPN.",
    "net.dialog.windows": "Откройте Параметры → Сеть и Интернет → VPN (или значок сети рядом с часами), выберите подключение и нажмите «Отключить» — или «Подключить», если VK у вас открывается только через VPN. Если у VPN-программы есть своё окно, переключите его там.",
    "net.dialog.mac": "Откройте Системные настройки → VPN (в старых версиях — Сеть) и переключите подключение. Программы вроде WireGuard или Outline управляются из значка в строке меню.",
    "net.dialog.linux": "В сетевом меню или в Параметры → Сеть переключите VPN-подключение. Из терминала: nmcli connection down|up <имя>, wg-quick down|up <интерфейс> или systemctl stop|start openvpn-client@<имя>.",
    "net.dialog.note": "VPN — только одна из возможных причин сбоя. Если соединение не восстановилось, проверьте интернет и повторите попытку позже. Скачанная музыка доступна без подключения к VK.",
    "net.close": "Закрыть",
    "net.restored": "Соединение восстановлено",
    "net.retrying": "Восстанавливаем соединение… ({n}/{max})",
    "net.unreachable": "Нет подключения к сети",
    "net.checking": "Проверяем подключение к VK…",
    "net.serviceRetrying": "VK временно недоступен. Повторяем запрос… ({n}/{max})",
    "net.serviceFailed": "VK временно недоступен",
    "net.serviceHint": "Сервис перегружен или ограничил частоту запросов. Подождите немного и повторите.",
    "net.serviceWait": "VK попросил подождать {seconds} с. Повторим автоматически.",
    "error.rate": "VK ограничил частоту запросов. Подождите немного и повторите.",
    "error.service": "Сервис временно недоступен. Повторите позже.",
    "error.format": "Сервис прислал неподдерживаемый ответ. Повторите позже.",
    "error.rate.wait": "VK ограничил запросы. Повторите через {seconds} с.",
    "error.service.wait": "Сервис попросил подождать {seconds} с. Повторите позже.",
    "error.device": "Не удаётся вывести звук. Проверьте выбранное устройство в настройках.",
    "error.unconfirmed": "Не удалось подтвердить изменение. Обновите список перед повторной попыткой, чтобы не выполнить его дважды.",
  },
  en: {
    "error.network": "Network error. Check your connection.",
    "net.failed": "Can't connect to VK",
    "net.hint": "If you use a VPN, try turning it off (or on, if VK is unavailable) and try again.",
    "net.retry": "Try again",
    "net.how": "How to check your VPN",
    "net.dialog.intro": "A VPN changes the network you reach VK from, and music links are tied to the network they were issued on. Private Music refreshes links and connections by itself, but if the music does not come back, check your VPN.",
    "net.dialog.windows": "Open Settings → Network & internet → VPN (or the network icon by the clock), pick the connection and choose Disconnect — or Connect, if VK only opens for you through a VPN. If your VPN app has its own window, switch it there.",
    "net.dialog.mac": "Open System Settings → VPN (Network in older versions) and switch the connection. Apps such as WireGuard or Outline are controlled from their menu bar icon.",
    "net.dialog.linux": "Switch the VPN connection in the network menu or in Settings → Network. From a terminal: nmcli connection down|up <name>, wg-quick down|up <interface> or systemctl stop|start openvpn-client@<name>.",
    "net.dialog.note": "A VPN is only one possible cause. If the connection does not come back, check your internet and try again later. Downloaded music plays without a connection to VK.",
    "net.close": "Close",
    "net.restored": "Connection restored",
    "net.retrying": "Restoring the connection… ({n}/{max})",
    "net.unreachable": "No network connection",
    "net.checking": "Checking the connection to VK…",
    "net.serviceRetrying": "VK is temporarily unavailable. Retrying… ({n}/{max})",
    "net.serviceFailed": "VK is temporarily unavailable",
    "net.serviceHint": "The service is busy or limiting requests. Wait a little and try again.",
    "net.serviceWait": "VK asked to wait {seconds} s. Playback will retry automatically.",
    "error.rate": "VK is limiting requests. Wait a little and try again.",
    "error.service": "The service is temporarily unavailable. Try again later.",
    "error.format": "The service returned an unsupported response. Try again later.",
    "error.rate.wait": "VK is limiting requests. Try again in {seconds} s.",
    "error.service.wait": "The service asked to wait {seconds} s. Try again later.",
    "error.device": "Audio output is unavailable. Check the selected device in settings.",
    "error.unconfirmed": "The change could not be confirmed. Refresh the list before trying again to avoid applying it twice.",
  },
});

// Таблица стилей поставляется вместе с этим модулем, поэтому больше ничего не нужно указывать.
if (typeof document !== "undefined" && !document.querySelector("link[data-net-css]")) {
  document.head.append(h("link", { rel: "stylesheet", href: new URL("../css/network.css", import.meta.url).href, "data-net-css": "" }));
}

/**
 * Только тайм-ауты и сбои транспорта предполагают наличие соединения (VPNHelpPolicy): никогда не
 * возникает проблем со входом в систему, не отклоняется запрос или не воспроизводится трек, который
 * невозможно воспроизвести.
 */
const NETWORK_KINDS = new Set(["offline", "timeout", "network"]);
export const isNetworkError = (error) => Boolean(error) && NETWORK_KINDS.has(error.kind);

const networkTexts = () => new Set([t("error.offline"), t("error.timeout"), t("error.network")]);

// ------------------------------------------------------------------ карта

/** Как проверить VPN для каждой платформы в виде простого диалогового окна. */
export function openHowTo() {
  const close = h("button", { class: "btn primary", type: "button", text: t("net.close") });
  const platform = (name, key) => h("li", {}, h("strong", { text: `${name}: ` }), t(key));
  const dialog = h("dialog", { class: "dialog net-dialog", "aria-label": t("net.how") },
    h("h2", { text: t("net.how") }),
    h("p", { class: "dialog-text", text: t("net.dialog.intro") }),
    h("ul", { class: "net-platforms" },
      platform("Windows", "net.dialog.windows"), platform("macOS", "net.dialog.mac"), platform("Linux", "net.dialog.linux")),
    h("p", { class: "dialog-text net-note", text: t("net.dialog.note") }),
    h("div", { class: "dialog-actions" }, close));
  const finish = () => {
    dialog.close();
    dialog.remove();
    if (!document.querySelector("dialog.dialog")) document.body.classList.remove("overlay-open");
  };
  close.addEventListener("click", finish);
  dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(); });
  document.body.classList.add("overlay-open");
  document.body.append(dialog);
  dialog.showModal();
  close.focus();
}

/** Компактная карточка: что произошло, подсказка, повтор и инструкции. */
export function vpnHint({ onRetry }) {
  return h("div", { class: "net-hint", role: "group", "aria-label": t("net.failed") },
    h("p", { class: "net-hint-title" }, icon("alert", 16), h("span", { text: t("net.failed") })),
    h("p", { class: "net-hint-text", text: t("net.hint") }),
    h("div", { class: "net-hint-actions" },
      h("button", { class: "btn net-retry", type: "button", text: t("net.retry"), onclick: onRetry }),
      h("button", { class: "net-how", type: "button", text: t("net.how"), onclick: openHowTo })));
}

// ------------------------------------------------------- Не удалось загрузить список

let reloader = null;
/** Как перезагрузить отображаемый экран (для ошибок, не требующих повторной попытки). */
export function setReloader(fn) { reloader = fn; }

const pending = new Set();

let automatic = false;

async function retryStatus(element, { check = true } = {}) {
  const mine = connectionGeneration;
  if (check) await api.networkCheck().catch(() => {});
  if (mine !== connectionGeneration || !element.isConnected) return;
  const own = element.querySelector(":scope > .retry");
  if (own) {
    // Нажатие собственной кнопки панели не означает повторного запроса пользователя.
    const previous = automatic;
    automatic = true;
    try { own.click(); } finally { automatic = previous; }
  } else reloader?.();
}

/**
 * Вызывается всякий раз, когда экран отображает ошибку на панели состояния. При сбое соединения
 * (распознаваемом по тексту, который выдают только эти типы) под сообщением появляется карта VPN, и
 * панель запоминается, поэтому она перезагружается сама, когда сеть восстанавливается.
 */
export function decorateStatus(element, text) {
  pending.delete(element);
  if (!networkTexts().has(text)) return;
  if (element.classList.contains("inline")) {
    // Примечание в одну строку: только путь к инструкции, подсказка в виде ее всплывающей подсказки.
    element.append(h("button", { class: "net-how", type: "button", title: t("net.hint"), text: t("net.how"), onclick: openHowTo }));
  } else {
    element.append(vpnHint({ onRetry: () => retryStatus(element) }));
  }
  watchRetry(element);
  pending.add(element);
  trouble = true;
}

const watched = new WeakSet();
export function forgetStatus(element) { pending.delete(element); }

/** Собственная кнопка панели «Повторить» также сначала отменяет соединения. */
function watchRetry(element) {
  if (watched.has(element)) return;
  watched.add(element);
  element.addEventListener("click", (event) => {
    if (!automatic && event.target instanceof Element && event.target.closest(".retry")) api.networkCheck().catch(() => {});
  }, { capture: true });
}

// -------------------------------------------- Воспроизведение и т. д.

let trouble = false;
let float = null;
let chip = null;
let chipKind = "";
let banner = null;
let connectionGeneration = 0;
let reconnecting = null;
let lastProbeAt = -Infinity;
let reconnectTimer = null;
let reconnectRequested = false;

function cancelReconnectTimer() {
  clearTimeout(reconnectTimer);
  reconnectTimer = null;
  reconnectRequested = false;
}

function floating() {
  if (!float) {
    float = h("div", { class: "net-float" });
    document.body.append(float);
  }
  return float;
}

function showChip(text, kind) {
  chipKind = kind;
  if (!chip) {
    chip = h("div", { class: "net-chip glass strong", role: "status" });
    floating().append(chip);
  }
  chip.textContent = text;
}

function hideChip() {
  chip?.remove();
  chip = null;
}

function hideBanner() {
  banner?.remove();
  banner = null;
}

/** Воспроизведение остановилось из-за проблемы с подключением: та же карта, над панелью плеера. */
function showBanner() {
  hideChip();
  trouble = true;
  if (banner?.dataset.kind === "service") hideBanner();
  if (banner) return;
  banner = h("div", { class: "net-banner glass strong", role: "alert" },
    vpnHint({
      onRetry: async () => {
        hideBanner();
        await api.networkCheck().catch(() => {});
        api.retryPlayback().catch(() => {});
      },
    }));
  floating().append(banner);
}

function showServiceBanner() {
  hideChip();
  hideBanner();
  trouble = true;
  banner = h("div", { class:"net-banner glass strong", role:"alert", "data-kind":"service" },
    h("div", {class:"net-hint"},
      h("p", {class:"net-hint-title",text:t("net.serviceFailed")}),
      h("p", {class:"net-hint-text",text:t("net.serviceHint")}),
      h("button", {class:"btn",type:"button",text:t("net.retry"),onclick:() => {
        hideBanner(); api.retryPlayback().catch(() => {});
      }})));
  floating().append(banner);
}

/** Соединение восстановлено: перезагрузите то, что не удалось загрузить, и скажите об этом один раз. */
function confirmedBack(verified) {
  cancelReconnectTimer();
  hideChip();
  hideBanner();
  const waiting = [...pending].filter((element) => element.isConnected && element.getClientRects().length && element.querySelector(":scope > .net-hint, :scope > .net-how"));
  for (const element of [...pending]) if (!element.isConnected || waiting.includes(element)) pending.delete(element);
  for (const element of waiting) retryStatus(element, { check: false });
  if (trouble && verified) {
    trouble = false;
    toast(t("net.restored"));
  }
}

// Событие online и стабильный маршрут не доказывают, что VK доступен:
// за ними могут оставаться страница входа Wi-Fi, отказ DNS или блокировка сервиса.
async function connectivityBack() {
  const visiblePending = [...pending].some(element => element.isConnected && element.getClientRects().length);
  if (!trouble && !visiblePending) return;
  if (reconnecting) { reconnectRequested = true; return; }
  const remaining = 1500 - (performance.now() - lastProbeAt);
  if (remaining > 0) {
    // Последний сигнал не теряется из-за защиты от частых событий.
    reconnectTimer ??= setTimeout(() => { reconnectTimer = null; connectivityBack(); },remaining);
    return;
  }
  const mine = connectionGeneration;
  lastProbeAt = performance.now();
  const work = (async () => {
    if (store.session.signed_in) {
      showChip(t("net.checking"), "checking");
      try { await api.networkProbe(); }
      catch (error) {
        if (mine !== connectionGeneration) return;
        // Ответ об истёкшей сессии тоже подтверждает доступность API.
        // Само продление остаётся задачей обычного запроса приложения.
        if (error?.kind !== "unauthorized") {
          if (banner) hideChip();
          else showChip(t("net.failed"), "failed");
          return;
        }
      }
    }
    if (mine === connectionGeneration) confirmedBack(store.session.signed_in);
  })();
  reconnecting = work;
  try { await work; } finally {
    if (reconnecting === work) {
      reconnecting = null;
      if (reconnectRequested) { reconnectRequested = false; connectivityBack(); }
    }
  }
}

export function resetNetwork() {
  cancelReconnectTimer();
  connectionGeneration++;
  lastProbeAt = -Infinity;
  pending.clear();
  trouble = false;
  chipKind = "";
  hideChip();
  hideBanner();
}

function connectionLost() {
  cancelReconnectTimer();
  connectionGeneration++;
  lastProbeAt = -Infinity;
  trouble = true;
  showChip(t("net.unreachable"), "unreachable");
}

/**
 * События механизма (`pm-event`). Ничего не возвращает; вызывающий объект по-прежнему обрабатывает
 * свои собственные запросы.
 */
export function handleBackendEvent(event) {
  switch (event.type) {
    case "network":
      if (event.state === "changed") { cancelReconnectTimer(); connectionGeneration++; lastProbeAt = -Infinity; }
      else if (event.state === "unreachable") connectionLost();
      else if (event.state === "ok") connectivityBack();
      break;
    case "recovery":
      if (event.phase === "retrying" && (event.kind === "service" || event.attempt >= 2) && event.kind !== "device") {
        const waiting = event.kind === "service" && Number.isFinite(event.retry_in_secs) && event.retry_in_secs >= 2;
        trouble = true; showChip(t(waiting ? "net.serviceWait" : event.kind === "service" ? "net.serviceRetrying" : "net.retrying", { n: event.attempt, max: event.max,seconds:event.retry_in_secs }), "retrying");
      }
      else if (event.phase === "failed" && event.kind === "network") showBanner();
      else if (event.phase === "failed" && event.kind === "service") showServiceBanner();
      else if (event.phase === "recovered") {
        connectionGeneration++;
        confirmedBack(true);
      }
      break;
    case "error":
      if (isNetworkError(event)) showBanner();
      else if (["rate","service"].includes(event.kind)) showServiceBanner();
      break;
    case "state":
      // Играю снова: больше не за что извиняться.
      if (event.status === "playing") { if (chipKind === "retrying") hideChip(); hideBanner(); }
      else if (["idle","paused"].includes(event.status) && chipKind === "retrying") hideChip();
      break;
  }
}

/** Сообщает движку о фокусе окна и прослушивает собственные события подключения браузера. */
export function initNetwork() {
  const report = () => api.networkFocus(document.hasFocus() && !document.hidden).catch(() => {});
  window.addEventListener("focus", report);
  window.addEventListener("blur", report);
  document.addEventListener("visibilitychange", report);
  window.addEventListener("online", connectivityBack);
  window.addEventListener("offline", connectionLost);
  report();
}
