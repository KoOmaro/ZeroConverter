/** The interface languages. `name` is written in that language, so a person who cannot read the current one can still find theirs. */
export const LOCALES = [
  { code: "en", name: "English", dir: "ltr" },
  { code: "tr", name: "Türkçe", dir: "ltr" },
  { code: "de", name: "Deutsch", dir: "ltr" },
  { code: "es", name: "Español", dir: "ltr" },
  { code: "fr", name: "Français", dir: "ltr" },
  { code: "pt-BR", name: "Português (Brasil)", dir: "ltr" },
  { code: "ja", name: "日本語", dir: "ltr" },
  { code: "ko", name: "한국어", dir: "ltr" },
  { code: "zh-Hans", name: "简体中文", dir: "ltr" },
  { code: "ru", name: "Русский", dir: "ltr" },
  { code: "hi", name: "हिन्दी", dir: "ltr" },
  { code: "ar", name: "العربية", dir: "rtl" },
] as const;

export type LocaleCode = (typeof LOCALES)[number]["code"];
export const DEFAULT_LOCALE: LocaleCode = "en";

/** "system" follows Windows. */
export type LanguagePreference = "system" | LocaleCode;

export function isLocaleCode(value: unknown): value is LocaleCode {
  return LOCALES.some((l) => l.code === value);
}

/** Maps a BCP 47 tag from the system ("pt-PT", "zh-CN", "de-AT") to a supported locale. */
export function matchLocale(tag: string): LocaleCode | null {
  const lower = tag.toLowerCase();
  const exact = LOCALES.find((l) => l.code.toLowerCase() === lower);
  if (exact) return exact.code;
  const primary = lower.split("-")[0];
  if (primary === "zh") return "zh-Hans";
  if (primary === "pt") return "pt-BR";
  return LOCALES.find((l) => l.code === primary)?.code ?? null;
}

export function systemLocale(languages: readonly string[] = typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language]): LocaleCode {
  for (const tag of languages) {
    const hit = matchLocale(tag);
    if (hit) return hit;
  }
  return DEFAULT_LOCALE;
}
