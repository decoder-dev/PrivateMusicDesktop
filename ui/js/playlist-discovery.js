import { t } from "./i18n.js";
import "./mobile/strings.js";

export const discoveryField = () => ({name:"discovery",label:t("pl.discovery"),value:"",options:[
  ["",t("pl.discovery.keep")],["show",t("pl.discovery.show")],["hide",t("pl.discovery.hide")],
]});
export const discoveryChoice = value => value === "hide" ? true : value === "show" ? false : null;
