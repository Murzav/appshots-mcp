use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::model::config::Caption;
use crate::model::locale::AsoLocale;
use crate::service::keyword_matcher;

use super::ProjectCache;

/// Result of keyword analysis for a locale.
#[derive(Debug, Serialize)]
pub(crate) struct AnalysisResult {
    pub(crate) locale: String,
    pub(crate) total_keywords: usize,
    pub(crate) coverage_percent: f64,
    pub(crate) gaps: Vec<String>,
    pub(crate) covered_keywords: Vec<String>,
}

/// Analyze keyword coverage for a locale's captions.
pub(crate) async fn handle_analyze_keywords(
    cache: &Mutex<ProjectCache>,
    locale: &AsoLocale,
    captions: &[Caption],
) -> Result<AnalysisResult, AppShotsError> {
    let guard = cache.lock().await;

    // Get metadata for locale (with fallback chain)
    let meta = locale
        .fallback_chain()
        .iter()
        .find_map(|l| guard.metadata.get(l))
        .ok_or_else(|| AppShotsError::LocaleNotFound(locale.to_string()))?;

    let report = keyword_matcher::coverage_report(captions, &meta.keywords, locale);

    let covered_keywords: Vec<String> = meta
        .keywords
        .iter()
        .filter(|k| {
            !report
                .gaps
                .iter()
                .any(|g| g.to_lowercase() == k.to_lowercase())
        })
        .cloned()
        .collect();

    Ok(AnalysisResult {
        locale: locale.to_string(),
        total_keywords: report.total_keywords,
        coverage_percent: report.coverage_percent,
        gaps: report.gaps,
        covered_keywords,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::config::LocaleMetadata;
    use std::collections::HashMap;

    fn make_caption(title: &str, subtitle: Option<&str>, keyword: Option<&str>) -> Caption {
        Caption {
            mode: 1,
            title: title.to_owned(),
            subtitle: subtitle.map(|s| s.to_owned()),
            keyword: keyword.map(|s| s.to_owned()),
        }
    }

    fn cache_with_metadata(locale: AsoLocale, keywords: Vec<String>) -> Mutex<ProjectCache> {
        let mut metadata = HashMap::new();
        metadata.insert(
            locale,
            LocaleMetadata {
                keywords,
                name: Some("TestApp".into()),
                subtitle: None,
            },
        );
        Mutex::new(ProjectCache {
            config: None,
            metadata,
        })
    }

    #[tokio::test]
    async fn analyze_with_some_gaps() {
        let keywords = vec![
            "glucose".into(),
            "blood sugar".into(),
            "insulin".into(),
            "diabetes".into(),
            "health".into(),
        ];
        let cache = cache_with_metadata(AsoLocale::EnUs, keywords);

        let captions = vec![
            make_caption("Track Glucose Levels", None, None),
            make_caption("Monitor Blood Sugar", None, None),
        ];

        let result = handle_analyze_keywords(&cache, &AsoLocale::EnUs, &captions)
            .await
            .unwrap();

        assert_eq!(result.locale, "en-US");
        assert_eq!(result.total_keywords, 5);
        assert!((result.coverage_percent - 40.0).abs() < 0.01);
        assert_eq!(result.gaps.len(), 3);
        assert!(result.gaps.contains(&"insulin".to_string()));
        assert!(result.gaps.contains(&"diabetes".to_string()));
        assert!(result.gaps.contains(&"health".to_string()));
        assert_eq!(result.covered_keywords.len(), 2);
    }

    #[tokio::test]
    async fn analyze_full_coverage() {
        let keywords = vec!["glucose".into(), "monitor".into()];
        let cache = cache_with_metadata(AsoLocale::EnUs, keywords);

        let captions = vec![make_caption("Glucose Monitor", None, None)];

        let result = handle_analyze_keywords(&cache, &AsoLocale::EnUs, &captions)
            .await
            .unwrap();

        assert!((result.coverage_percent - 100.0).abs() < 0.01);
        assert!(result.gaps.is_empty());
        assert_eq!(result.covered_keywords.len(), 2);
    }

    #[tokio::test]
    async fn analyze_with_fallback_locale() {
        // en-AU falls back to en-US
        let keywords = vec!["photo".into(), "editor".into()];
        let cache = cache_with_metadata(AsoLocale::EnUs, keywords);

        let captions = vec![make_caption("Photo Editor", None, None)];

        let result = handle_analyze_keywords(&cache, &AsoLocale::EnAu, &captions)
            .await
            .unwrap();

        assert_eq!(result.locale, "en-AU");
        assert!((result.coverage_percent - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn analyze_locale_not_found() {
        let cache = Mutex::new(ProjectCache::new());
        let captions = vec![make_caption("Hello", None, None)];

        let err = handle_analyze_keywords(&cache, &AsoLocale::Ja, &captions)
            .await
            .unwrap_err();
        assert!(matches!(err, AppShotsError::LocaleNotFound(_)));
    }
}
