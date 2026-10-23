import { api } from "./api.js";
import { h } from "./dom.js";
import { icon } from "./icons.js";
import { t } from "./i18n.js";
import { isAndroid } from "./mobile/layout.js";
import "./mobile/strings.js";

function legacyCopy(url) {
  const previous=document.activeElement;
  const proxy=h("textarea",{class:"clipboard-copy-proxy",readonly:true,tabindex:-1});
  proxy.value=url;document.body.append(proxy);
  try {
    proxy.focus({preventScroll:true});proxy.select();
    if(!document.execCommand("copy"))throw new Error("copy unavailable");
  } finally {proxy.remove();if(previous?.isConnected)previous.focus({preventScroll:true});}
}
export async function copyLink(url, toast) {
  if(!url) {toast(t("share.unavailable"),{error:true});return;}
  try {
    if(isAndroid()) await api.copyLink(url);
    else if(navigator.clipboard?.writeText) {
      try {await navigator.clipboard.writeText(url);} catch {legacyCopy(url);}
    } else legacyCopy(url);
    toast(t("share.copied"));
  } catch {toast(t("share.copyFailed"),{error:true});}
}
export async function shareLink(url,title,toast) {
  if(!url) {toast(t("share.unavailable"),{error:true});return;}
  if(!isAndroid())return copyLink(url,toast);
  try {await api.shareLink(url,title??"");}
  catch {toast(t("share.failed"),{error:true});}
}
export function linkControls(getURL,getTitle,toast) {
  const copy=h("button",{class:"icon-btn circle-btn collection-copy",type:"button",title:t("share.copy"),"aria-label":t("share.copy"),onclick:()=>copyLink(getURL(),toast)},icon("copy",18));
  const share=h("button",{class:"icon-btn circle-btn collection-share",type:"button",title:t("share.open"),"aria-label":t("share.open"),onclick:()=>shareLink(getURL(),getTitle(),toast)},icon("share",18));
  return {copy,share,refresh(){copy.disabled=share.disabled=!getURL();share.hidden=!isAndroid();}};
}
