use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Script systems used by App Store locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::upper_case_acronyms)]
pub enum Script {
    Latin,
    Arabic,
    Hebrew,
    CJK,
    Devanagari,
    Thai,
    Cyrillic,
    Greek,
}

/// All 39 App Store Connect locales for ASO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
pub enum AsoLocale {
    ArSa,
    Ca,
    Cs,
    Da,
    DeDe,
    El,
    EnAu,
    EnCa,
    EnGb,
    EnUs,
    EsEs,
    EsMx,
    Fi,
    FrCa,
    FrFr,
    He,
    Hi,
    Hr,
    Hu,
    Id,
    It,
    Ja,
    Ko,
    Ms,
    Nb,
    NlNl,
    Pl,
    PtBr,
    PtPt,
    Ro,
    Ru,
    Sk,
    Sv,
    Th,
    Tr,
    Uk,
    Vi,
    ZhHans,
    ZhHant,
}

/// All 39 locales in alphabetical order by code.
pub const ALL: &[AsoLocale] = &[
    AsoLocale::ArSa,
    AsoLocale::Ca,
    AsoLocale::Cs,
    AsoLocale::Da,
    AsoLocale::DeDe,
    AsoLocale::El,
    AsoLocale::EnAu,
    AsoLocale::EnCa,
    AsoLocale::EnGb,
    AsoLocale::EnUs,
    AsoLocale::EsEs,
    AsoLocale::EsMx,
    AsoLocale::Fi,
    AsoLocale::FrCa,
    AsoLocale::FrFr,
    AsoLocale::He,
    AsoLocale::Hi,
    AsoLocale::Hr,
    AsoLocale::Hu,
    AsoLocale::Id,
    AsoLocale::It,
    AsoLocale::Ja,
    AsoLocale::Ko,
    AsoLocale::Ms,
    AsoLocale::Nb,
    AsoLocale::NlNl,
    AsoLocale::Pl,
    AsoLocale::PtBr,
    AsoLocale::PtPt,
    AsoLocale::Ro,
    AsoLocale::Ru,
    AsoLocale::Sk,
    AsoLocale::Sv,
    AsoLocale::Th,
    AsoLocale::Tr,
    AsoLocale::Uk,
    AsoLocale::Vi,
    AsoLocale::ZhHans,
    AsoLocale::ZhHant,
];

impl AsoLocale {
    /// The locale code string (e.g. "ar-SA", "en-US", "zh-Hans").
    pub fn code(&self) -> &'static str {
        match self {
            Self::ArSa => "ar-SA",
            Self::Ca => "ca",
            Self::Cs => "cs",
            Self::Da => "da",
            Self::DeDe => "de-DE",
            Self::El => "el",
            Self::EnAu => "en-AU",
            Self::EnCa => "en-CA",
            Self::EnGb => "en-GB",
            Self::EnUs => "en-US",
            Self::EsEs => "es-ES",
            Self::EsMx => "es-MX",
            Self::Fi => "fi",
            Self::FrCa => "fr-CA",
            Self::FrFr => "fr-FR",
            Self::He => "he",
            Self::Hi => "hi",
            Self::Hr => "hr",
            Self::Hu => "hu",
            Self::Id => "id",
            Self::It => "it",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Ms => "ms",
            Self::Nb => "nb",
            Self::NlNl => "nl-NL",
            Self::Pl => "pl",
            Self::PtBr => "pt-BR",
            Self::PtPt => "pt-PT",
            Self::Ro => "ro",
            Self::Ru => "ru",
            Self::Sk => "sk",
            Self::Sv => "sv",
            Self::Th => "th",
            Self::Tr => "tr",
            Self::Uk => "uk",
            Self::Vi => "vi",
            Self::ZhHans => "zh-Hans",
            Self::ZhHant => "zh-Hant",
        }
    }

    /// Returns the fallback locale for keyword/metadata inheritance.
    pub fn fallback(&self) -> Option<AsoLocale> {
        match self {
            Self::EsMx => Some(Self::EsEs),
            Self::FrCa => Some(Self::FrFr),
            Self::EnAu | Self::EnCa | Self::EnGb => Some(Self::EnUs),
            Self::PtPt => Some(Self::PtBr),
            Self::ZhHant => Some(Self::ZhHans),
            _ => None,
        }
    }

    /// Returns self followed by all fallbacks recursively.
    pub fn fallback_chain(&self) -> Vec<AsoLocale> {
        let mut chain = vec![*self];
        let mut current = *self;
        while let Some(fb) = current.fallback() {
            chain.push(fb);
            current = fb;
        }
        chain
    }

    /// The writing script used by this locale.
    pub fn script(&self) -> Script {
        match self {
            Self::ArSa => Script::Arabic,
            Self::He => Script::Hebrew,
            Self::Ja | Self::Ko | Self::ZhHans | Self::ZhHant => Script::CJK,
            Self::Hi => Script::Devanagari,
            Self::Th => Script::Thai,
            Self::El => Script::Greek,
            Self::Ru | Self::Uk => Script::Cyrillic,
            _ => Script::Latin,
        }
    }
}

impl fmt::Display for AsoLocale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Error returned when parsing an invalid locale string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLocaleError(String);

impl fmt::Display for ParseLocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown ASO locale: '{}'", self.0)
    }
}

impl std::error::Error for ParseLocaleError {}

impl FromStr for AsoLocale {
    type Err = ParseLocaleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "ar-sa" => Ok(Self::ArSa),
            "ca" => Ok(Self::Ca),
            "cs" => Ok(Self::Cs),
            "da" => Ok(Self::Da),
            "de-de" => Ok(Self::DeDe),
            "el" => Ok(Self::El),
            "en-au" => Ok(Self::EnAu),
            "en-ca" => Ok(Self::EnCa),
            "en-gb" => Ok(Self::EnGb),
            "en-us" => Ok(Self::EnUs),
            "es-es" => Ok(Self::EsEs),
            "es-mx" => Ok(Self::EsMx),
            "fi" => Ok(Self::Fi),
            "fr-ca" => Ok(Self::FrCa),
            "fr-fr" => Ok(Self::FrFr),
            "he" => Ok(Self::He),
            "hi" => Ok(Self::Hi),
            "hr" => Ok(Self::Hr),
            "hu" => Ok(Self::Hu),
            "id" => Ok(Self::Id),
            "it" => Ok(Self::It),
            "ja" => Ok(Self::Ja),
            "ko" => Ok(Self::Ko),
            "ms" => Ok(Self::Ms),
            "nb" => Ok(Self::Nb),
            "nl-nl" => Ok(Self::NlNl),
            "pl" => Ok(Self::Pl),
            "pt-br" => Ok(Self::PtBr),
            "pt-pt" => Ok(Self::PtPt),
            "ro" => Ok(Self::Ro),
            "ru" => Ok(Self::Ru),
            "sk" => Ok(Self::Sk),
            "sv" => Ok(Self::Sv),
            "th" => Ok(Self::Th),
            "tr" => Ok(Self::Tr),
            "uk" => Ok(Self::Uk),
            "vi" => Ok(Self::Vi),
            "zh-hans" => Ok(Self::ZhHans),
            "zh-hant" => Ok(Self::ZhHant),
            _ => Err(ParseLocaleError(s.to_owned())),
        }
    }
}

impl Serialize for AsoLocale {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for AsoLocale {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn all_contains_39_locales() {
        assert_eq!(ALL.len(), 39);
    }

    #[test]
    fn all_is_sorted_by_code() {
        let codes: Vec<&str> = ALL.iter().map(|l| l.code()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn fromstr_display_roundtrip_all() {
        for locale in ALL {
            let code = locale.to_string();
            let parsed: AsoLocale = code.parse().unwrap();
            assert_eq!(*locale, parsed);
        }
    }

    #[test]
    fn fromstr_case_insensitive() {
        assert_eq!("EN-US".parse::<AsoLocale>().unwrap(), AsoLocale::EnUs);
        assert_eq!("Zh-Hans".parse::<AsoLocale>().unwrap(), AsoLocale::ZhHans);
        assert_eq!("AR-SA".parse::<AsoLocale>().unwrap(), AsoLocale::ArSa);
    }

    #[test]
    fn fromstr_invalid() {
        assert!("xx-YY".parse::<AsoLocale>().is_err());
        assert!("".parse::<AsoLocale>().is_err());
        assert!("english".parse::<AsoLocale>().is_err());
    }

    #[test]
    fn fallback_chains() {
        assert_eq!(
            AsoLocale::EsMx.fallback_chain(),
            vec![AsoLocale::EsMx, AsoLocale::EsEs]
        );
        assert_eq!(
            AsoLocale::FrCa.fallback_chain(),
            vec![AsoLocale::FrCa, AsoLocale::FrFr]
        );
        assert_eq!(
            AsoLocale::EnAu.fallback_chain(),
            vec![AsoLocale::EnAu, AsoLocale::EnUs]
        );
        assert_eq!(
            AsoLocale::PtPt.fallback_chain(),
            vec![AsoLocale::PtPt, AsoLocale::PtBr]
        );
        assert_eq!(
            AsoLocale::ZhHant.fallback_chain(),
            vec![AsoLocale::ZhHant, AsoLocale::ZhHans]
        );
    }

    #[test]
    fn fallback_none_for_root_locales() {
        assert_eq!(AsoLocale::EnUs.fallback(), None);
        assert_eq!(AsoLocale::EsEs.fallback(), None);
        assert_eq!(AsoLocale::FrFr.fallback(), None);
        assert_eq!(AsoLocale::PtBr.fallback(), None);
        assert_eq!(AsoLocale::ZhHans.fallback(), None);
        assert_eq!(AsoLocale::Ja.fallback(), None);
    }

    #[test]
    fn fallback_chain_no_fallback_is_self_only() {
        assert_eq!(AsoLocale::EnUs.fallback_chain(), vec![AsoLocale::EnUs]);
        assert_eq!(AsoLocale::Ja.fallback_chain(), vec![AsoLocale::Ja]);
    }

    #[test]
    fn script_detection() {
        assert_eq!(AsoLocale::ArSa.script(), Script::Arabic);
        assert_eq!(AsoLocale::He.script(), Script::Hebrew);
        assert_eq!(AsoLocale::Ja.script(), Script::CJK);
        assert_eq!(AsoLocale::Ko.script(), Script::CJK);
        assert_eq!(AsoLocale::ZhHans.script(), Script::CJK);
        assert_eq!(AsoLocale::ZhHant.script(), Script::CJK);
        assert_eq!(AsoLocale::Hi.script(), Script::Devanagari);
        assert_eq!(AsoLocale::Th.script(), Script::Thai);
        assert_eq!(AsoLocale::El.script(), Script::Greek);
        assert_eq!(AsoLocale::Ru.script(), Script::Cyrillic);
        assert_eq!(AsoLocale::Uk.script(), Script::Cyrillic);
        assert_eq!(AsoLocale::EnUs.script(), Script::Latin);
        assert_eq!(AsoLocale::DeDe.script(), Script::Latin);
        assert_eq!(AsoLocale::FrFr.script(), Script::Latin);
    }

    #[test]
    fn serde_roundtrip() {
        for locale in ALL {
            let json = serde_json::to_string(locale).unwrap();
            assert_eq!(json, format!("\"{}\"", locale.code()));
            let back: AsoLocale = serde_json::from_str(&json).unwrap();
            assert_eq!(*locale, back);
        }
    }

    #[test]
    fn serde_deserialize_case_insensitive() {
        let back: AsoLocale = serde_json::from_str("\"EN-US\"").unwrap();
        assert_eq!(back, AsoLocale::EnUs);
    }
}
