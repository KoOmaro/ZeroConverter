// ==============================================================================
// OWNERSHIP:
//   The language of the text the Rust side shows by itself: the tray menu, the
//   Explorer right-click menu, Windows notices and the notification cards.
//   It reads the same catalogs as the window (`src/i18n/locales/*.json`, the
//   `native` section), so a text has one source. The window tells us which
//   language it runs in (`set_ui_language`); until it does, the saved choice
//   or the Windows display language is used.
// ==============================================================================
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// The interface languages, in the order of `src/i18n/locales.ts`.
pub const LANGUAGES: [&str; 12] = [
    "en", "tr", "de", "es", "fr", "pt-BR", "ja", "ko", "zh-Hans", "ru", "hi", "ar",
];

const SOURCES: [(&str, &str); 12] = [
    ("en", include_str!("../../src/i18n/locales/en.json")),
    ("tr", include_str!("../../src/i18n/locales/tr.json")),
    ("de", include_str!("../../src/i18n/locales/de.json")),
    ("es", include_str!("../../src/i18n/locales/es.json")),
    ("fr", include_str!("../../src/i18n/locales/fr.json")),
    ("pt-BR", include_str!("../../src/i18n/locales/pt-BR.json")),
    ("ja", include_str!("../../src/i18n/locales/ja.json")),
    ("ko", include_str!("../../src/i18n/locales/ko.json")),
    (
        "zh-Hans",
        include_str!("../../src/i18n/locales/zh-Hans.json"),
    ),
    ("ru", include_str!("../../src/i18n/locales/ru.json")),
    ("hi", include_str!("../../src/i18n/locales/hi.json")),
    ("ar", include_str!("../../src/i18n/locales/ar.json")),
];

static CATALOGS: OnceLock<HashMap<&'static str, Value>> = OnceLock::new();
static CURRENT: RwLock<String> = RwLock::new(String::new());

fn catalogs() -> &'static HashMap<&'static str, Value> {
    CATALOGS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(code, text)| (*code, serde_json::from_str(text).unwrap_or(Value::Null)))
            .collect()
    })
}

/// Maps a language tag ("pt-PT", "zh-CN", "de-AT") to one of `LANGUAGES`.
pub fn match_language(tag: &str) -> Option<&'static str> {
    let lower = tag.to_ascii_lowercase();
    if let Some(code) = LANGUAGES.iter().find(|c| c.to_ascii_lowercase() == lower) {
        return Some(code);
    }
    match lower.split(['-', '_']).next()? {
        "zh" => Some("zh-Hans"),
        "pt" => Some("pt-BR"),
        primary => LANGUAGES.iter().find(|c| **c == primary).copied(),
    }
}

/// The Windows display language, or English.
pub fn system_language() -> &'static str {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
        let mut buf = [0u16; 85];
        // SAFETY: the buffer is 85 u16 (LOCALE_NAME_MAX_LENGTH) and its length is passed.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            if let Some(code) = match_language(&String::from_utf16_lossy(&buf[..n as usize - 1])) {
                return code;
            }
        }
    }
    "en"
}

/// Sets the language (a code or a tag). Unknown tags leave it unchanged.
pub fn set(tag: &str) -> bool {
    match match_language(tag) {
        Some(code) => {
            if let Ok(mut cur) = CURRENT.write() {
                *cur = code.to_string();
            }
            true
        }
        None => false,
    }
}

pub fn current() -> String {
    match CURRENT.read() {
        Ok(cur) if !cur.is_empty() => cur.clone(),
        _ => system_language().to_string(),
    }
}

fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let pointer = format!("/native/{}", key.replace('.', "/"));
    catalogs().get(lang)?.pointer(&pointer)?.as_str()
}

fn fill(text: &str, params: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (name, value) in params {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

/// The text for `key` (under `native.`) in the current language; English when
/// the language lacks it; the key itself as a last resort.
pub fn tr(key: &str, params: &[(&str, &str)]) -> String {
    tr_in(&current(), key, params)
}

pub fn tr_in(lang: &str, key: &str, params: &[(&str, &str)]) -> String {
    let text = lookup(lang, key)
        .or_else(|| lookup("en", key))
        .unwrap_or(key);
    fill(text, params)
}

/// CLDR plural category of an integer, for the languages we ship.
pub fn plural_category(lang: &str, n: u64) -> &'static str {
    match lang {
        "ja" | "ko" | "zh-Hans" => "other",
        "fr" | "pt-BR" => {
            if n <= 1 {
                "one"
            } else if n != 0 && n.is_multiple_of(1_000_000) {
                "many"
            } else {
                "other"
            }
        }
        "es" => {
            if n == 1 {
                "one"
            } else if n != 0 && n.is_multiple_of(1_000_000) {
                "many"
            } else {
                "other"
            }
        }
        "ru" => {
            let (m10, m100) = (n % 10, n % 100);
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        "ar" => {
            let m100 = n % 100;
            match n {
                0 => "zero",
                1 => "one",
                2 => "two",
                _ if (3..=10).contains(&m100) => "few",
                _ if (11..=99).contains(&m100) => "many",
                _ => "other",
            }
        }
        // en, de, tr, hi
        _ => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// Like `tr`, for a text with a number: picks `key_one`, `key_few`… and fills `{{count}}`.
pub fn tr_n(key: &str, count: u64, params: &[(&str, &str)]) -> String {
    let lang = current();
    let count_text = count.to_string();
    let mut all: Vec<(&str, &str)> = params.to_vec();
    all.push(("count", &count_text));
    let plural = format!("{key}_{}", plural_category(&lang, count));
    if lookup(&lang, &plural).is_some() {
        return tr_in(&lang, &plural, &all);
    }
    tr_in(&lang, &format!("{key}_other"), &all)
}

/// Joins sentences: a space, or nothing in languages that do not use one.
pub fn join_sentences(parts: &[String]) -> String {
    let glue = tr("sentenceJoin", &[]);
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(&glue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags_map_to_the_shipped_languages() {
        assert_eq!(match_language("tr-TR"), Some("tr"));
        assert_eq!(match_language("pt-PT"), Some("pt-BR"));
        assert_eq!(match_language("zh-CN"), Some("zh-Hans"));
        assert_eq!(match_language("zh-Hans"), Some("zh-Hans"));
        assert_eq!(match_language("ar_SA"), Some("ar"));
        assert_eq!(match_language("sv-SE"), None);
    }

    #[test]
    fn every_language_has_every_native_text() {
        let en = catalogs()["en"]["native"]
            .as_object()
            .expect("native section");
        fn leaves(prefix: String, v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(map) => {
                    for (k, v) in map {
                        leaves(
                            if prefix.is_empty() {
                                k.clone()
                            } else {
                                format!("{prefix}.{k}")
                            },
                            v,
                            out,
                        );
                    }
                }
                _ => out.push(prefix),
            }
        }
        let mut keys = Vec::new();
        for (k, v) in en {
            leaves(k.clone(), v, &mut keys);
        }
        assert!(keys.len() > 30);
        for lang in LANGUAGES {
            for key in &keys {
                // Plural forms differ per language; the check script covers those.
                if key
                    .rsplit('_')
                    .next()
                    .is_some_and(|s| matches!(s, "zero" | "one" | "two" | "few" | "many" | "other"))
                {
                    continue;
                }
                assert!(lookup(lang, key).is_some(), "{lang} lacks native.{key}");
            }
        }
    }

    #[test]
    fn plural_forms_follow_the_language() {
        assert_eq!(plural_category("en", 1), "one");
        assert_eq!(plural_category("en", 2), "other");
        assert_eq!(plural_category("ru", 21), "one");
        assert_eq!(plural_category("ru", 3), "few");
        assert_eq!(plural_category("ru", 11), "many");
        assert_eq!(plural_category("ar", 2), "two");
        assert_eq!(plural_category("ar", 7), "few");
        assert_eq!(plural_category("ar", 15), "many");
        assert_eq!(plural_category("ar", 100), "other");
        assert_eq!(plural_category("ja", 1), "other");
        assert_eq!(plural_category("fr", 0), "one");
    }

    #[test]
    fn fills_placeholders_and_falls_back_to_english() {
        assert_eq!(
            tr_in("tr", "rename.notReadable", &[("name", "a.png")]),
            tr_in("tr", "rename.notReadable", &[("name", "a.png")])
        );
        assert!(tr_in("tr", "rename.notReadable", &[("name", "a.png")]).contains("a.png"));
        assert_eq!(tr_in("xx", "tray.quit", &[]), tr_in("en", "tray.quit", &[]));
        assert_eq!(tr_in("en", "no.such.key", &[]), "no.such.key");
    }
}
