import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { reportUiLanguage } from "../platform/tauri/languageGateway";
import {
  DEFAULT_LOCALE,
  LOCALES,
  isLocaleCode,
  systemLocale,
  type LanguagePreference,
  type LocaleCode,
} from "./locales";
import ar from "./locales/ar.json";
import de from "./locales/de.json";
import en from "./locales/en.json";
import es from "./locales/es.json";
import fr from "./locales/fr.json";
import hi from "./locales/hi.json";
import ja from "./locales/ja.json";
import ko from "./locales/ko.json";
import ptBR from "./locales/pt-BR.json";
import ru from "./locales/ru.json";
import tr from "./locales/tr.json";
import zhHans from "./locales/zh-Hans.json";

// Same key as the theme: every window (main, `+`, notification) reads it, and a
// change in one reaches the others through the `storage` event.
const KEY = "zeroconverter-language";

export function getLanguagePreference(): LanguagePreference {
  try {
    const saved = localStorage.getItem(KEY);
    if (isLocaleCode(saved)) return saved;
  } catch {
    // Storage can be blocked; fall through to the system language.
  }
  return "system";
}

function resolve(pref: LanguagePreference): LocaleCode {
  return pref === "system" ? systemLocale() : pref;
}

/** `lang` and `dir` on <html>; the CSS flips layout from `dir`. */
function applyToDocument(code: LocaleCode) {
  const root = document.documentElement;
  root.lang = code;
  root.dir = LOCALES.find((l) => l.code === code)?.dir ?? "ltr";
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    tr: { translation: tr },
    de: { translation: de },
    es: { translation: es },
    fr: { translation: fr },
    "pt-BR": { translation: ptBR },
    ja: { translation: ja },
    ko: { translation: ko },
    "zh-Hans": { translation: zhHans },
    ru: { translation: ru },
    hi: { translation: hi },
    ar: { translation: ar },
  },
  lng: resolve(getLanguagePreference()),
  // A key a language does not have yet shows the English text.
  fallbackLng: DEFAULT_LOCALE,
  supportedLngs: LOCALES.map((l) => l.code),
  interpolation: { escapeValue: false },
  returnNull: false,
  initAsync: false,
});

/** Rust makes the tray menu and the notices; it follows the window's language. Only the main window reports. */
function tellRust(lng: string) {
  if (window.location.hash === "" || window.location.hash === "#/") reportUiLanguage(lng).catch(() => {});
}

applyToDocument(i18n.language as LocaleCode);
tellRust(i18n.language);
i18n.on("languageChanged", (lng) => {
  applyToDocument(lng as LocaleCode);
  tellRust(lng);
});

export function setLanguagePreference(pref: LanguagePreference) {
  try {
    if (pref === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, pref);
  } catch {
    // The choice still applies until the window closes.
  }
  void i18n.changeLanguage(resolve(pref));
}

window.addEventListener("storage", (e) => {
  if (e.key === KEY) void i18n.changeLanguage(resolve(getLanguagePreference()));
});

export default i18n;
