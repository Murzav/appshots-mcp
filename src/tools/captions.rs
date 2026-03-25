use std::path::Path;

use indexmap::IndexMap;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::config::Caption;
use crate::model::locale::AsoLocale;
use crate::service::config_parser;
use crate::service::keyword_matcher;

use super::{CachedConfig, ProjectCache};

#[derive(Debug, Serialize)]
pub(crate) struct CaptionsResult {
    pub locale: String,
    pub captions: Vec<Caption>,
    pub total: usize,
}

/// Save captions for a locale. Upsert semantics: only touches modes present in input.
pub(crate) async fn handle_save_captions(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    write_lock: &Mutex<()>,
    config_path: &Path,
    locale: &str,
    captions: Vec<Caption>,
) -> Result<CaptionsResult, AppShotsError> {
    let _guard = write_lock.lock().await;

    // Re-read fresh from disk (write-lock pattern)
    let raw = store.read(config_path)?;
    let mut config = config_parser::parse_config(&raw)?;

    // Get or create the captions map: extra["captions"][locale] -> Vec<Caption>
    let mut all_captions: IndexMap<String, Vec<Caption>> = config
        .extra
        .get("captions")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let locale_captions = all_captions.entry(locale.to_owned()).or_default();

    // Upsert: replace by mode, append new
    for caption in captions {
        if let Some(pos) = locale_captions.iter().position(|c| c.mode == caption.mode) {
            locale_captions[pos] = caption;
        } else {
            locale_captions.push(caption);
        }
    }

    // Sort by mode for deterministic output
    locale_captions.sort_by_key(|c| c.mode);

    let result_captions = locale_captions.clone();
    let total = result_captions.len();

    config.extra.insert(
        "captions".to_owned(),
        serde_json::to_value(&all_captions).map_err(|e| AppShotsError::JsonParse(e.to_string()))?,
    );

    // Write back
    let json = config_parser::serialize_config(&config)?;
    store.write(config_path, &json)?;

    // Update cache
    let mtime = store.modified_time(config_path)?;
    let mut cache_guard = cache.lock().await;
    cache_guard.config = Some(CachedConfig {
        config,
        modified: mtime,
    });

    Ok(CaptionsResult {
        locale: locale.to_owned(),
        captions: result_captions,
        total,
    })
}

/// Get captions with optional locale/modes filters.
pub(crate) async fn handle_get_captions(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    config_path: &Path,
    locale: Option<&str>,
    modes: Option<&[u8]>,
) -> Result<serde_json::Value, AppShotsError> {
    let config = super::resolve_config(store, cache, config_path).await?;

    let all_captions: IndexMap<String, Vec<Caption>> = config
        .extra
        .get("captions")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Filter by locale if specified
    let filtered: IndexMap<String, Vec<Caption>> = all_captions
        .into_iter()
        .filter(|(loc, _)| locale.is_none() || locale == Some(loc.as_str()))
        .map(|(loc, caps)| {
            let filtered_caps = match modes {
                Some(m) => caps.into_iter().filter(|c| m.contains(&c.mode)).collect(),
                None => caps,
            };
            (loc, filtered_caps)
        })
        .collect();

    serde_json::to_value(&filtered).map_err(|e| AppShotsError::JsonParse(e.to_string()))
}

/// Get keywords.txt content for a locale.
pub(crate) async fn handle_get_locale_keywords(
    store: &dyn FileStore,
    project_dir: &Path,
    locale: &str,
) -> Result<serde_json::Value, AppShotsError> {
    let path = project_dir.join(format!("fastlane/metadata/{locale}/keywords.txt"));
    let content = store.read(&path)?;
    Ok(serde_json::json!({
        "locale": locale,
        "keywords": content.trim(),
    }))
}

/// Coverage matrix entry for one locale x mode combination.
#[derive(Debug, Serialize)]
pub(crate) struct CoverageEntry {
    pub locale: String,
    pub mode: u8,
    pub has_caption: bool,
}

/// Coverage matrix across all locales and modes.
#[derive(Debug, Serialize)]
pub(crate) struct CoverageMatrix {
    pub locales: Vec<String>,
    pub modes: Vec<u8>,
    pub coverage: Vec<CoverageEntry>,
    pub total_slots: usize,
    pub filled_slots: usize,
}

/// Build a coverage matrix: for each locale x mode, check if caption exists.
pub(crate) async fn handle_get_caption_coverage(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    config_path: &Path,
) -> Result<CoverageMatrix, AppShotsError> {
    let config = super::resolve_config(store, cache, config_path).await?;

    let all_captions: IndexMap<String, Vec<Caption>> = config
        .extra
        .get("captions")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Collect all modes from screen definitions
    let modes: Vec<u8> = config.screens.iter().map(|s| s.mode).collect();

    // Collect all locales that have any captions
    let locales: Vec<String> = all_captions.keys().cloned().collect();

    let mut coverage = Vec::new();
    let mut filled = 0;

    for locale in &locales {
        let locale_caps = all_captions.get(locale);
        for &mode in &modes {
            let has_caption = locale_caps
                .map(|caps| caps.iter().any(|c| c.mode == mode))
                .unwrap_or(false);
            if has_caption {
                filled += 1;
            }
            coverage.push(CoverageEntry {
                locale: locale.clone(),
                mode,
                has_caption,
            });
        }
    }

    let total_slots = locales.len() * modes.len();

    Ok(CoverageMatrix {
        locales,
        modes,
        coverage,
        total_slots,
        filled_slots: filled,
    })
}

/// Review result for a single caption.
#[derive(Debug, Serialize)]
pub(crate) struct CaptionReview {
    pub mode: u8,
    pub locale: String,
    pub keyword_coverage_percent: f64,
    pub matched_keywords: Vec<String>,
    pub gap_keywords: Vec<String>,
}

/// Review captions against keyword coverage.
pub(crate) async fn handle_review_captions(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    config_path: &Path,
    project_dir: &Path,
    locale: Option<&str>,
    modes: Option<&[u8]>,
) -> Result<serde_json::Value, AppShotsError> {
    let config = super::resolve_config(store, cache, config_path).await?;

    let all_captions: IndexMap<String, Vec<Caption>> = config
        .extra
        .get("captions")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let mut reviews: Vec<CaptionReview> = Vec::new();

    for (loc_str, caps) in &all_captions {
        if let Some(filter_locale) = locale
            && loc_str != filter_locale
        {
            continue;
        }

        // Try to read keywords for this locale
        let keywords_path = project_dir.join(format!("fastlane/metadata/{loc_str}/keywords.txt"));
        let keywords: Vec<String> = if let Ok(content) = store.read(&keywords_path) {
            content
                .trim()
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            Vec::new()
        };

        let parsed_locale: AsoLocale = match loc_str.parse() {
            Ok(l) => l,
            Err(_) => continue,
        };

        for caption in caps {
            if let Some(mode_filter) = modes
                && !mode_filter.contains(&caption.mode)
            {
                continue;
            }

            let report = keyword_matcher::coverage_report(
                std::slice::from_ref(caption),
                &keywords,
                &parsed_locale,
            );

            reviews.push(CaptionReview {
                mode: caption.mode,
                locale: loc_str.clone(),
                keyword_coverage_percent: report.coverage_percent,
                matched_keywords: report.matches.iter().map(|m| m.keyword.clone()).collect(),
                gap_keywords: report.gaps,
            });
        }
    }

    serde_json::to_value(&reviews).map_err(|e| AppShotsError::JsonParse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio::sync::Mutex;

    use crate::io::memory::MemoryStore;
    use crate::model::config::Caption;
    use crate::tools::ProjectCache;

    use super::*;

    fn minimal_config_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#
    }

    fn sample_caption(mode: u8) -> Caption {
        Caption {
            mode,
            title: format!("Title for mode {mode}"),
            subtitle: Some(format!("Subtitle for mode {mode}")),
            keyword: Some("keyword".into()),
        }
    }

    fn setup() -> (MemoryStore, Mutex<ProjectCache>, Mutex<()>) {
        let store = MemoryStore::new();
        let config_path = Path::new("/project/appshots.json");
        store.write(config_path, minimal_config_json()).unwrap();
        (store, Mutex::new(ProjectCache::new()), Mutex::new(()))
    }

    #[tokio::test]
    async fn save_captions_get_captions_roundtrip() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        let captions = vec![sample_caption(1), sample_caption(2)];
        let result =
            handle_save_captions(&store, &cache, &write_lock, config_path, "en-US", captions)
                .await
                .unwrap();

        assert_eq!(result.locale, "en-US");
        assert_eq!(result.total, 2);
        assert_eq!(result.captions[0].mode, 1);
        assert_eq!(result.captions[1].mode, 2);

        // Verify via get
        let get_result = handle_get_captions(&store, &cache, config_path, Some("en-US"), None)
            .await
            .unwrap();
        let en_us = get_result.get("en-US").unwrap().as_array().unwrap();
        assert_eq!(en_us.len(), 2);
    }

    #[tokio::test]
    async fn upsert_single_mode_preserves_others() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        // Save modes 1, 2, 3
        let captions = vec![sample_caption(1), sample_caption(2), sample_caption(3)];
        handle_save_captions(&store, &cache, &write_lock, config_path, "en-US", captions)
            .await
            .unwrap();

        // Update only mode 2
        let updated = Caption {
            mode: 2,
            title: "Updated title".into(),
            subtitle: None,
            keyword: None,
        };
        let result = handle_save_captions(
            &store,
            &cache,
            &write_lock,
            config_path,
            "en-US",
            vec![updated],
        )
        .await
        .unwrap();

        assert_eq!(result.total, 3);
        assert_eq!(result.captions[0].title, "Title for mode 1");
        assert_eq!(result.captions[1].title, "Updated title");
        assert!(result.captions[1].subtitle.is_none());
        assert_eq!(result.captions[2].title, "Title for mode 3");
    }

    #[tokio::test]
    async fn get_captions_with_locale_filter() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        // Save captions for two locales
        handle_save_captions(
            &store,
            &cache,
            &write_lock,
            config_path,
            "en-US",
            vec![sample_caption(1)],
        )
        .await
        .unwrap();
        handle_save_captions(
            &store,
            &cache,
            &write_lock,
            config_path,
            "ja",
            vec![sample_caption(1)],
        )
        .await
        .unwrap();

        // Filter by en-US only
        let result = handle_get_captions(&store, &cache, config_path, Some("en-US"), None)
            .await
            .unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("en-US"));

        // No filter — both locales
        let result_all = handle_get_captions(&store, &cache, config_path, None, None)
            .await
            .unwrap();
        let obj_all = result_all.as_object().unwrap();
        assert_eq!(obj_all.len(), 2);
    }

    #[tokio::test]
    async fn get_captions_with_mode_filter() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        let captions = vec![sample_caption(1), sample_caption(2), sample_caption(3)];
        handle_save_captions(&store, &cache, &write_lock, config_path, "en-US", captions)
            .await
            .unwrap();

        // Filter modes 1 and 3
        let result = handle_get_captions(&store, &cache, config_path, Some("en-US"), Some(&[1, 3]))
            .await
            .unwrap();
        let en_us = result.get("en-US").unwrap().as_array().unwrap();
        assert_eq!(en_us.len(), 2);
        assert_eq!(en_us[0]["mode"], 1);
        assert_eq!(en_us[1]["mode"], 3);
    }

    #[tokio::test]
    async fn get_locale_keywords_reads_correct_file() {
        let store = MemoryStore::new();
        let project_dir = Path::new("/project");
        let keywords_path = Path::new("/project/fastlane/metadata/en-US/keywords.txt");
        store
            .write(keywords_path, "photo,editor,filter,camera\n")
            .unwrap();

        let result = handle_get_locale_keywords(&store, project_dir, "en-US")
            .await
            .unwrap();
        assert_eq!(result["locale"], "en-US");
        assert_eq!(result["keywords"], "photo,editor,filter,camera");
    }

    #[tokio::test]
    async fn get_locale_keywords_file_not_found() {
        let store = MemoryStore::new();
        let project_dir = Path::new("/project");

        let err = handle_get_locale_keywords(&store, project_dir, "xx-XX")
            .await
            .unwrap_err();
        assert!(matches!(err, AppShotsError::FileNotFound { .. }));
    }

    fn config_with_screens_and_captions() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [
                {"mode": 1, "name": "Home"},
                {"mode": 2, "name": "Settings"},
                {"mode": 3, "name": "Stats"}
            ],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""],
            "captions": {
                "en-US": [
                    {"mode": 1, "title": "Track Glucose Levels", "keyword": "glucose"},
                    {"mode": 2, "title": "Custom Settings"}
                ],
                "de-DE": [
                    {"mode": 1, "title": "Blutzucker verfolgen"}
                ]
            }
        }"#
    }

    #[tokio::test]
    async fn coverage_matrix_basic() {
        let store = MemoryStore::new();
        let config_path = Path::new("/project/appshots.json");
        store
            .write(config_path, config_with_screens_and_captions())
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let result = handle_get_caption_coverage(&store, &cache, config_path)
            .await
            .unwrap();

        assert_eq!(result.modes, vec![1, 2, 3]);
        assert_eq!(result.locales.len(), 2);
        // en-US has modes 1,2 filled; de-DE has mode 1 filled
        assert_eq!(result.filled_slots, 3);
        assert_eq!(result.total_slots, 6); // 2 locales * 3 modes
    }

    #[tokio::test]
    async fn coverage_matrix_empty_captions() {
        let (store, cache, _) = setup();
        let config_path = Path::new("/project/appshots.json");

        let result = handle_get_caption_coverage(&store, &cache, config_path)
            .await
            .unwrap();
        assert!(result.locales.is_empty());
        assert_eq!(result.filled_slots, 0);
    }

    #[tokio::test]
    async fn review_captions_with_keywords() {
        let store = MemoryStore::new();
        let config_path = Path::new("/project/appshots.json");
        let project_dir = Path::new("/project");
        store
            .write(config_path, config_with_screens_and_captions())
            .unwrap();

        // Write keywords
        store
            .write(
                &project_dir.join("fastlane/metadata/en-US/keywords.txt"),
                "glucose,blood sugar,tracker,health",
            )
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let result = handle_review_captions(
            &store,
            &cache,
            config_path,
            project_dir,
            Some("en-US"),
            None,
        )
        .await
        .unwrap();

        let reviews = result.as_array().unwrap();
        assert_eq!(reviews.len(), 2); // 2 captions for en-US

        // First caption has "glucose" in title and keyword field
        let first = &reviews[0];
        assert_eq!(first["mode"], 1);
        assert!(
            first["matched_keywords"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k == "glucose")
        );
    }

    #[tokio::test]
    async fn review_captions_with_mode_filter() {
        let store = MemoryStore::new();
        let config_path = Path::new("/project/appshots.json");
        let project_dir = Path::new("/project");
        store
            .write(config_path, config_with_screens_and_captions())
            .unwrap();
        store
            .write(
                &project_dir.join("fastlane/metadata/en-US/keywords.txt"),
                "glucose",
            )
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let result = handle_review_captions(
            &store,
            &cache,
            config_path,
            project_dir,
            Some("en-US"),
            Some(&[1]),
        )
        .await
        .unwrap();

        let reviews = result.as_array().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0]["mode"], 1);
    }

    #[tokio::test]
    async fn review_captions_no_keywords_file() {
        let store = MemoryStore::new();
        let config_path = Path::new("/project/appshots.json");
        let project_dir = Path::new("/project");
        store
            .write(config_path, config_with_screens_and_captions())
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let result = handle_review_captions(
            &store,
            &cache,
            config_path,
            project_dir,
            Some("en-US"),
            None,
        )
        .await
        .unwrap();

        let reviews = result.as_array().unwrap();
        // With no keywords, coverage should be 100% (no keywords to match)
        for review in reviews {
            assert_eq!(review["keyword_coverage_percent"], 100.0);
        }
    }
}
