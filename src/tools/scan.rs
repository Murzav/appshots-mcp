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

/// Summary of the project's readiness state.
#[derive(Debug, Serialize)]
pub(crate) struct ProjectStatus {
    pub config_exists: bool,
    pub template_exists: bool,
    pub locales_scanned: usize,
    pub captions_count: usize,
    pub captures_count: usize,
    pub ready_to_compose: bool,
}

/// Get the current project status.
pub(crate) async fn handle_get_project_status(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    project_dir: &Path,
    config_path: &Path,
) -> Result<ProjectStatus, AppShotsError> {
    let config_exists = store.exists(config_path);

    // Check for any template (single or per-screen)
    let appshots_dir = project_dir.join("appshots");
    let template_exists = store.exists(&appshots_dir.join("template.typ"))
        || store.exists(&appshots_dir.join("templates/template.typ"));

    // Count scanned locales from cache
    let locales_scanned = {
        let guard = cache.lock().await;
        guard.metadata.len()
    };

    // Count captions from config
    let captions_count = if config_exists {
        if let Ok(config) = super::resolve_config(store, cache, config_path).await {
            config
                .extra
                .get("captions")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.values()
                        .filter_map(|v| v.as_array())
                        .map(|a| a.len())
                        .sum()
                })
                .unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    // Count captures (PNG files in appshots/captures/)
    let captures_dir = appshots_dir.join("captures");
    let captures_count = if store.exists(&captures_dir) {
        count_png_files(store, &captures_dir)
    } else {
        0
    };

    let ready_to_compose = config_exists && template_exists && captions_count > 0;

    Ok(ProjectStatus {
        config_exists,
        template_exists,
        locales_scanned,
        captions_count,
        captures_count,
        ready_to_compose,
    })
}

/// Recursively count PNG files under a directory.
fn count_png_files(store: &dyn FileStore, dir: &Path) -> usize {
    let Ok(entries) = store.list_dir(dir) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries {
        if entry.extension().and_then(|e| e.to_str()) == Some("png") {
            count += 1;
        } else if store.list_dir(&entry).is_ok() {
            count += count_png_files(store, &entry);
        }
    }
    count
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

    fn minimal_config_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#
    }

    fn config_with_captions_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""],
            "captions": {
                "en-US": [
                    {"mode": 1, "title": "Title 1"},
                    {"mode": 2, "title": "Title 2"}
                ],
                "de-DE": [
                    {"mode": 1, "title": "Titel 1"}
                ]
            }
        }"#
    }

    #[tokio::test]
    async fn project_status_empty_project() {
        let store = MemoryStore::new();
        let cache = Mutex::new(ProjectCache::new());
        let config_path = project_dir().join("appshots.json");

        let status = handle_get_project_status(&store, &cache, &project_dir(), &config_path)
            .await
            .unwrap();

        assert!(!status.config_exists);
        assert!(!status.template_exists);
        assert_eq!(status.locales_scanned, 0);
        assert_eq!(status.captions_count, 0);
        assert_eq!(status.captures_count, 0);
        assert!(!status.ready_to_compose);
    }

    #[tokio::test]
    async fn project_status_with_config_and_template() {
        let store = MemoryStore::new();
        let config_path = project_dir().join("appshots.json");
        store.write(&config_path, minimal_config_json()).unwrap();
        store
            .write(&project_dir().join("appshots/template.typ"), "#set page()")
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let status = handle_get_project_status(&store, &cache, &project_dir(), &config_path)
            .await
            .unwrap();

        assert!(status.config_exists);
        assert!(status.template_exists);
        assert!(!status.ready_to_compose); // no captions yet
    }

    #[tokio::test]
    async fn project_status_ready_to_compose() {
        let store = MemoryStore::new();
        let config_path = project_dir().join("appshots.json");
        store
            .write(&config_path, config_with_captions_json())
            .unwrap();
        store
            .write(&project_dir().join("appshots/template.typ"), "#set page()")
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let status = handle_get_project_status(&store, &cache, &project_dir(), &config_path)
            .await
            .unwrap();

        assert!(status.config_exists);
        assert!(status.template_exists);
        assert_eq!(status.captions_count, 3); // 2 en-US + 1 de-DE
        assert!(status.ready_to_compose);
    }

    #[tokio::test]
    async fn project_status_counts_scanned_locales() {
        let store = MemoryStore::new();
        let config_path = project_dir().join("appshots.json");
        store.write(&config_path, minimal_config_json()).unwrap();
        write_locale(&store, "en-US", "photo", "App", "Sub");
        write_locale(&store, "de-DE", "foto", "App", "");

        let cache = Mutex::new(ProjectCache::new());
        // Scan first to populate cache
        handle_scan_project(&store, &cache, &project_dir())
            .await
            .unwrap();

        let status = handle_get_project_status(&store, &cache, &project_dir(), &config_path)
            .await
            .unwrap();
        assert_eq!(status.locales_scanned, 2);
    }

    #[tokio::test]
    async fn project_status_counts_captures() {
        let store = MemoryStore::new();
        let config_path = project_dir().join("appshots.json");
        store.write(&config_path, minimal_config_json()).unwrap();

        // Add some PNG "captures"
        let captures_dir = project_dir().join("appshots/captures/iPhone/en-US");
        store
            .write(&captures_dir.join("mode-1.png"), "fake-png")
            .unwrap();
        store
            .write(&captures_dir.join("mode-2.png"), "fake-png")
            .unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let status = handle_get_project_status(&store, &cache, &project_dir(), &config_path)
            .await
            .unwrap();
        assert_eq!(status.captures_count, 2);
    }

    #[test]
    fn project_status_serialization() {
        let status = ProjectStatus {
            config_exists: true,
            template_exists: true,
            locales_scanned: 5,
            captions_count: 10,
            captures_count: 20,
            ready_to_compose: true,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["config_exists"], true);
        assert_eq!(json["captions_count"], 10);
        assert_eq!(json["ready_to_compose"], true);
    }
}
