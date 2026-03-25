use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::locale::AsoLocale;
use crate::service::metadata_parser;

use super::ProjectCache;

/// Result of scanning the project's fastlane metadata.
#[derive(Debug, Serialize)]
pub(crate) struct ScanResult {
    pub(crate) locales_found: usize,
    pub(crate) locales: Vec<LocaleScanInfo>,
}

/// Per-locale scan summary.
#[derive(Debug, Serialize)]
pub(crate) struct LocaleScanInfo {
    pub(crate) locale: String,
    pub(crate) has_keywords: bool,
    pub(crate) keyword_count: usize,
    pub(crate) has_name: bool,
    pub(crate) has_subtitle: bool,
}

/// Scan fastlane/metadata/ for all locales.
pub(crate) async fn handle_scan_project(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    project_dir: &Path,
) -> Result<ScanResult, AppShotsError> {
    let metadata_dir = project_dir.join("fastlane/metadata");

    // List locale directories
    let locale_dirs = store.list_dir(&metadata_dir)?;

    let mut result = ScanResult {
        locales_found: 0,
        locales: vec![],
    };
    let mut metadata_map = HashMap::new();

    for dir in locale_dirs {
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Try to parse as AsoLocale — skip non-locale dirs
        let locale = match dir_name.parse::<AsoLocale>() {
            Ok(l) => l,
            Err(_) => continue,
        };

        // Read keywords.txt, name.txt, subtitle.txt
        let keywords_content = store.read(&dir.join("keywords.txt")).ok();
        let name_content = store.read(&dir.join("name.txt")).ok();
        let subtitle_content = store.read(&dir.join("subtitle.txt")).ok();

        let meta = metadata_parser::build_metadata(
            keywords_content.as_deref(),
            name_content.as_deref(),
            subtitle_content.as_deref(),
        );

        result.locales.push(LocaleScanInfo {
            locale: locale.to_string(),
            has_keywords: !meta.keywords.is_empty(),
            keyword_count: meta.keyword_count(),
            has_name: meta.name.is_some(),
            has_subtitle: meta.subtitle.is_some(),
        });

        metadata_map.insert(locale, meta);
    }

    result.locales_found = result.locales.len();
    result.locales.sort_by(|a, b| a.locale.cmp(&b.locale));

    // Update cache
    let mut guard = cache.lock().await;
    guard.metadata = metadata_map;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;
    use std::path::PathBuf;

    fn project_dir() -> PathBuf {
        PathBuf::from("/project")
    }

    fn write_locale(store: &MemoryStore, locale: &str, keywords: &str, name: &str, subtitle: &str) {
        let base = project_dir().join("fastlane/metadata").join(locale);
        store.write(&base.join("keywords.txt"), keywords).unwrap();
        if !name.is_empty() {
            store.write(&base.join("name.txt"), name).unwrap();
        }
        if !subtitle.is_empty() {
            store.write(&base.join("subtitle.txt"), subtitle).unwrap();
        }
    }

    #[tokio::test]
    async fn scan_with_three_locales() {
        let store = MemoryStore::new();
        write_locale(
            &store,
            "en-US",
            "photo,editor,filter",
            "MyApp",
            "Fast & Easy",
        );
        write_locale(&store, "de-DE", "foto,bearbeiter", "MeineApp", "");
        write_locale(&store, "ja", "写真,編集", "マイアプリ", "簡単＆速い");

        let cache = Mutex::new(ProjectCache::new());
        let result = handle_scan_project(&store, &cache, &project_dir())
            .await
            .unwrap();

        assert_eq!(result.locales_found, 3);
        // Sorted by locale code
        assert_eq!(result.locales[0].locale, "de-DE");
        assert_eq!(result.locales[1].locale, "en-US");
        assert_eq!(result.locales[2].locale, "ja");

        // Check en-US details
        let en = &result.locales[1];
        assert!(en.has_keywords);
        assert_eq!(en.keyword_count, 3);
        assert!(en.has_name);
        assert!(en.has_subtitle);

        // Check de-DE (no subtitle)
        let de = &result.locales[0];
        assert!(de.has_keywords);
        assert_eq!(de.keyword_count, 2);
        assert!(de.has_name);
        assert!(!de.has_subtitle);

        // Verify cache was updated
        let guard = cache.lock().await;
        assert_eq!(guard.metadata.len(), 3);
        assert!(guard.metadata.contains_key(&AsoLocale::EnUs));
    }

    #[tokio::test]
    async fn scan_empty_project_no_fastlane_dir() {
        let store = MemoryStore::new();
        let cache = Mutex::new(ProjectCache::new());

        let err = handle_scan_project(&store, &cache, &project_dir())
            .await
            .unwrap_err();
        assert!(matches!(err, AppShotsError::FileNotFound { .. }));
    }

    #[tokio::test]
    async fn scan_skips_non_locale_dirs() {
        let store = MemoryStore::new();
        write_locale(&store, "en-US", "photo", "MyApp", "");
        // Add a non-locale directory
        store
            .write(
                &project_dir().join("fastlane/metadata/screenshots/readme.txt"),
                "ignore me",
            )
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let result = handle_scan_project(&store, &cache, &project_dir())
            .await
            .unwrap();

        assert_eq!(result.locales_found, 1);
        assert_eq!(result.locales[0].locale, "en-US");
    }
}
