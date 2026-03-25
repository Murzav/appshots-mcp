use std::path::Path;

use indexmap::IndexMap;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::config::Caption;
use crate::service::config_parser;

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
}
