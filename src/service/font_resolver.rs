use crate::model::locale::{AsoLocale, Script};

/// Font info describing an available font.
#[derive(Debug, Clone)]
pub struct FontInfo {
    pub name: String,
    pub path: String,
    pub scripts: Vec<Script>,
}

/// Select the best font for a locale from available fonts.
///
/// Priority: font supporting the target script, then first available font.
pub fn resolve_font<'a>(locale: &AsoLocale, available: &'a [FontInfo]) -> Option<&'a FontInfo> {
    let target_script = locale.script();

    // First: find font supporting the target script
    let script_match = available
        .iter()
        .find(|f| f.scripts.contains(&target_script));
    if script_match.is_some() {
        return script_match;
    }

    // Fallback: first available font
    available.first()
}

/// Suggest a system font family name for a locale's script.
pub fn suggest_system_font(locale: &AsoLocale) -> &'static str {
    match locale.script() {
        Script::CJK => match locale {
            AsoLocale::Ja => "Hiragino Sans",
            AsoLocale::Ko => "Apple SD Gothic Neo",
            _ => "PingFang SC", // zh-Hans, zh-Hant
        },
        Script::Arabic => "SF Arabic",
        Script::Hebrew => "SF Hebrew",
        Script::Devanagari => "Kohinoor Devanagari",
        Script::Thai => "Thonburi",
        _ => "SF Pro Display",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latin_font() -> FontInfo {
        FontInfo {
            name: "SF Pro".into(),
            path: "/fonts/sf-pro.otf".into(),
            scripts: vec![Script::Latin],
        }
    }

    fn cjk_font() -> FontInfo {
        FontInfo {
            name: "Noto CJK".into(),
            path: "/fonts/noto-cjk.otf".into(),
            scripts: vec![Script::CJK],
        }
    }

    fn multi_font() -> FontInfo {
        FontInfo {
            name: "Universal".into(),
            path: "/fonts/universal.otf".into(),
            scripts: vec![Script::Latin, Script::CJK, Script::Arabic],
        }
    }

    #[test]
    fn resolve_font_exact_script_match() {
        let fonts = vec![latin_font(), cjk_font()];
        let result = resolve_font(&AsoLocale::Ja, &fonts).unwrap();
        assert_eq!(result.name, "Noto CJK");
    }

    #[test]
    fn resolve_font_fallback_to_first() {
        let fonts = vec![latin_font()];
        // Arabic not in available fonts — falls back to first
        let result = resolve_font(&AsoLocale::ArSa, &fonts).unwrap();
        assert_eq!(result.name, "SF Pro");
    }

    #[test]
    fn resolve_font_empty_list() {
        let fonts: Vec<FontInfo> = vec![];
        assert!(resolve_font(&AsoLocale::EnUs, &fonts).is_none());
    }

    #[test]
    fn resolve_font_multi_script() {
        let fonts = vec![multi_font()];
        let result = resolve_font(&AsoLocale::ArSa, &fonts).unwrap();
        assert_eq!(result.name, "Universal");
    }

    #[test]
    fn resolve_font_prefers_script_match_over_first() {
        let fonts = vec![latin_font(), cjk_font()];
        let result = resolve_font(&AsoLocale::EnUs, &fonts).unwrap();
        assert_eq!(result.name, "SF Pro");
    }

    #[test]
    fn suggest_system_font_latin() {
        assert_eq!(suggest_system_font(&AsoLocale::EnUs), "SF Pro Display");
        assert_eq!(suggest_system_font(&AsoLocale::DeDe), "SF Pro Display");
        assert_eq!(suggest_system_font(&AsoLocale::FrFr), "SF Pro Display");
    }

    #[test]
    fn suggest_system_font_cjk() {
        assert_eq!(suggest_system_font(&AsoLocale::Ja), "Hiragino Sans");
        assert_eq!(suggest_system_font(&AsoLocale::Ko), "Apple SD Gothic Neo");
        assert_eq!(suggest_system_font(&AsoLocale::ZhHans), "PingFang SC");
        assert_eq!(suggest_system_font(&AsoLocale::ZhHant), "PingFang SC");
    }

    #[test]
    fn suggest_system_font_rtl() {
        assert_eq!(suggest_system_font(&AsoLocale::ArSa), "SF Arabic");
        assert_eq!(suggest_system_font(&AsoLocale::He), "SF Hebrew");
    }

    #[test]
    fn suggest_system_font_indic_thai() {
        assert_eq!(suggest_system_font(&AsoLocale::Hi), "Kohinoor Devanagari");
        assert_eq!(suggest_system_font(&AsoLocale::Th), "Thonburi");
    }

    #[test]
    fn suggest_system_font_cyrillic_greek() {
        // Cyrillic and Greek fall through to SF Pro Display
        assert_eq!(suggest_system_font(&AsoLocale::Ru), "SF Pro Display");
        assert_eq!(suggest_system_font(&AsoLocale::El), "SF Pro Display");
    }
}
