import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { it } from "../locales/it";
import { en } from "../locales/en";

export const LANGS = ["it", "en"] as const;
export type Lang = (typeof LANGS)[number];

const KEY = "rdt-lang";
const DEFAULT_LANG: Lang = "it";

function isLang(v: string | null): v is Lang {
  return v === "it" || v === "en";
}

function initialLang(): Lang {
  // 20260807 ++ RG #i18n default all'italiano (lingua base); la scelta esplicita è persistita.
  // Per l'auto-detect dalla lingua del browser: leggere navigator.language qui.
  const stored = localStorage.getItem(KEY);
  return isLang(stored) ? stored : DEFAULT_LANG;
}

i18n.use(initReactI18next).init({
  resources: { it: { translation: it }, en: { translation: en } },
  lng: initialLang(),
  fallbackLng: DEFAULT_LANG,
  interpolation: { escapeValue: false },
});

export function getLang(): Lang {
  return isLang(i18n.language) ? i18n.language : DEFAULT_LANG;
}

export function setLang(lang: Lang) {
  localStorage.setItem(KEY, lang);
  i18n.changeLanguage(lang);
  document.documentElement.setAttribute("lang", lang);
}

document.documentElement.setAttribute("lang", getLang());

export default i18n;
