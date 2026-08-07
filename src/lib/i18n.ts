import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { it } from "../locales/it";
import { en } from "../locales/en";
import { LANGS, STORAGE_KEYS, type Lang } from "./constants";
import { DEFAULT_LANG } from "./defaults";

export { LANGS };
export type { Lang };

function isLang(v: string | null): v is Lang {
  return v !== null && (LANGS as readonly string[]).includes(v);
}

function initialLang(): Lang {
  const stored = localStorage.getItem(STORAGE_KEYS.lang);
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
  localStorage.setItem(STORAGE_KEYS.lang, lang);
  i18n.changeLanguage(lang);
  document.documentElement.setAttribute("lang", lang);
}

document.documentElement.setAttribute("lang", getLang());

export default i18n;
