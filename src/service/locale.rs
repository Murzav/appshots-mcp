use std::collections::HashMap;
use std::str::FromStr;

use crate::error::AppShotsError;
use crate::model::locale::{ALL, AsoLocale, Script};

/// Resolve locale content with fallback: try exact match, then walk fallback chain.
pub fn resolve_locale_content<'a, T>(
    primary: &AsoLocale,
    available: &'a HashMap<AsoLocale, T>,
) -> Option<(&'a AsoLocale, &'a T)> {
    for locale in primary.fallback_chain() {
        if let Some((k, v)) = available.get_key_value(&locale) {
            return Some((k, v));
        }
    }
    None
}

/// Get text direction for a locale.
pub fn text_direction(locale: &AsoLocale) -> &'static str {
    match locale.script() {
        Script::Arabic | Script::Hebrew => "rtl",
        _ => "ltr",
    }
}

/// All 39 ASO locales.
pub fn all_locales() -> &'static [AsoLocale] {
    ALL
}

/// Validate a locale code string, return AsoLocale or error.
pub fn validate_locale(code: &str) -> Result<AsoLocale, AppShotsError> {
    AsoLocale::from_str(code).map_err(|e| AppShotsError::LocaleNotFound(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_direction_rtl() {
        assert_eq!(text_direction(&AsoLocale::ArSa), "rtl");
        assert_eq!(text_direction(&AsoLocale::He), "rtl");
    }

    #[test]
    fn text_direction_ltr() {
        assert_eq!(text_direction(&AsoLocale::EnUs), "ltr");
        assert_eq!(text_direction(&AsoLocale::Ja), "ltr");
        assert_eq!(text_direction(&AsoLocale::ZhHans), "ltr");
        assert_eq!(text_direction(&AsoLocale::Ru), "ltr");
        assert_eq!(text_direction(&AsoLocale::Hi), "ltr");
        assert_eq!(text_direction(&AsoLocale::Th), "ltr");
    }

    #[test]
    fn validate_locale_valid() {
        assert_eq!(validate_locale("en-US").unwrap(), AsoLocale::EnUs);
        assert_eq!(validate_locale("zh-Hans").unwrap(), AsoLocale::ZhHans);
    }

    #[test]
    fn validate_locale_case_insensitive() {
        assert_eq!(validate_locale("EN-US").unwrap(), AsoLocale::EnUs);
    }

    #[test]
    fn validate_locale_invalid() {
        assert!(validate_locale("xx-YY").is_err());
        assert!(validate_locale("").is_err());
    }

    #[test]
    fn all_locales_count() {
        assert_eq!(all_locales().len(), 39);
    }

    #[test]
    fn resolve_exact_match() {
        let mut map = HashMap::new();
        map.insert(AsoLocale::EnUs, "hello");
        map.insert(AsoLocale::FrFr, "bonjour");

        let (locale, val) = resolve_locale_content(&AsoLocale::EnUs, &map).unwrap();
        assert_eq!(*locale, AsoLocale::EnUs);
        assert_eq!(*val, "hello");
    }

    #[test]
    fn resolve_with_fallback() {
        let mut map = HashMap::new();
        map.insert(AsoLocale::EnUs, "hello");

        // en-AU falls back to en-US
        let (locale, val) = resolve_locale_content(&AsoLocale::EnAu, &map).unwrap();
        assert_eq!(*locale, AsoLocale::EnUs);
        assert_eq!(*val, "hello");
    }

    #[test]
    fn resolve_prefers_exact_over_fallback() {
        let mut map = HashMap::new();
        map.insert(AsoLocale::EnAu, "g'day");
        map.insert(AsoLocale::EnUs, "hello");

        let (locale, val) = resolve_locale_content(&AsoLocale::EnAu, &map).unwrap();
        assert_eq!(*locale, AsoLocale::EnAu);
        assert_eq!(*val, "g'day");
    }

    #[test]
    fn resolve_none_when_missing() {
        let map: HashMap<AsoLocale, &str> = HashMap::new();
        assert!(resolve_locale_content(&AsoLocale::Ja, &map).is_none());
    }

    #[test]
    fn resolve_fallback_chain_es_mx() {
        let mut map = HashMap::new();
        map.insert(AsoLocale::EsEs, "hola");

        let (locale, val) = resolve_locale_content(&AsoLocale::EsMx, &map).unwrap();
        assert_eq!(*locale, AsoLocale::EsEs);
        assert_eq!(*val, "hola");
    }
}
