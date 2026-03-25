pub mod analyze;
pub mod captions;
pub mod capture;
pub mod design;
pub mod glossary;
pub mod plan;
pub mod render;
pub mod scan;
pub mod validate;

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use rmcp::RoleServer;
use rmcp::model::{LoggingLevel, LoggingMessageNotificationParam};
use rmcp::service::Peer;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::config::{LocaleMetadata, ProjectConfig};
use crate::model::locale::AsoLocale;
use crate::service::config_parser;

/// Cached project configuration with mtime tracking.
pub(crate) struct CachedConfig {
    pub(crate) config: ProjectConfig,
    pub(crate) modified: SystemTime,
}

/// Cache for project state: config + scanned metadata.
pub(crate) struct ProjectCache {
    pub(crate) config: Option<CachedConfig>,
    pub(crate) metadata: HashMap<AsoLocale, LocaleMetadata>,
}

impl ProjectCache {
    pub(crate) fn new() -> Self {
        Self {
            config: None,
            metadata: HashMap::new(),
        }
    }
}

/// Send an MCP log notification. Fire-and-forget.
/// Will be used when individual tools add progress logging.
#[allow(dead_code)]
pub(crate) async fn mcp_log(peer: Option<&Peer<RoleServer>>, level: LoggingLevel, msg: &str) {
    let Some(peer) = peer else { return };
    let param =
        LoggingMessageNotificationParam::new(level, serde_json::json!(msg)).with_logger("appshots");
    let _ = peer.notify_logging_message(param).await;
}

/// Load or refresh the project config from appshots.json.
pub(crate) async fn resolve_config(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    config_path: &Path,
) -> Result<ProjectConfig, AppShotsError> {
    // Check if cached config is still fresh (mtime unchanged)
    {
        let guard = cache.lock().await;
        if let Some(ref cached) = guard.config
            && let Ok(current_mtime) = store.modified_time(config_path)
            && current_mtime == cached.modified
        {
            return Ok(cached.config.clone());
        }
    }

    // Read fresh from disk
    if !store.exists(config_path) {
        return Err(AppShotsError::ConfigNotFound {
            path: config_path.to_path_buf(),
        });
    }
    let raw = store.read(config_path)?;
    let config = config_parser::parse_config(&raw)?;
    let mtime = store.modified_time(config_path)?;

    // Update cache
    let mut guard = cache.lock().await;
    guard.config = Some(CachedConfig {
        config: config.clone(),
        modified: mtime,
    });

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;
    use std::path::PathBuf;

    fn minimal_config_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#
    }

    #[test]
    fn project_cache_new_is_empty() {
        let cache = ProjectCache::new();
        assert!(cache.config.is_none());
        assert!(cache.metadata.is_empty());
    }

    #[tokio::test]
    async fn resolve_config_reads_from_store_and_caches() {
        let store = MemoryStore::new();
        let path = PathBuf::from("/project/appshots.json");
        store.write(&path, minimal_config_json()).unwrap();

        let cache = Mutex::new(ProjectCache::new());
        let config = resolve_config(&store, &cache, &path).await.unwrap();
        assert_eq!(config.bundle_id, "com.example.app");

        // Verify it was cached
        let guard = cache.lock().await;
        assert!(guard.config.is_some());
        assert_eq!(
            guard.config.as_ref().unwrap().config.bundle_id,
            "com.example.app"
        );
    }

    #[tokio::test]
    async fn resolve_config_returns_cached_on_same_mtime() {
        let store = MemoryStore::new();
        let path = PathBuf::from("/project/appshots.json");
        store.write(&path, minimal_config_json()).unwrap();

        let cache = Mutex::new(ProjectCache::new());

        // Pre-populate cache with a known mtime
        let mtime = store.modified_time(&path).unwrap();
        let config = config_parser::parse_config(minimal_config_json()).unwrap();
        {
            let mut guard = cache.lock().await;
            guard.config = Some(CachedConfig {
                config: ProjectConfig {
                    bundle_id: "cached-value".into(),
                    ..config
                },
                modified: mtime,
            });
        }

        // Should return cached value (bundle_id = "cached-value")
        let result = resolve_config(&store, &cache, &path).await.unwrap();
        assert_eq!(result.bundle_id, "cached-value");
    }

    #[tokio::test]
    async fn resolve_config_refreshes_on_mtime_change() {
        let store = MemoryStore::new();
        let path = PathBuf::from("/project/appshots.json");
        store.write(&path, minimal_config_json()).unwrap();

        let cache = Mutex::new(ProjectCache::new());

        // Pre-populate cache with a stale mtime
        let config = config_parser::parse_config(minimal_config_json()).unwrap();
        {
            let mut guard = cache.lock().await;
            guard.config = Some(CachedConfig {
                config: ProjectConfig {
                    bundle_id: "stale-value".into(),
                    ..config
                },
                modified: SystemTime::UNIX_EPOCH, // stale mtime
            });
        }

        // Should re-read from store because mtime differs
        let result = resolve_config(&store, &cache, &path).await.unwrap();
        assert_eq!(result.bundle_id, "com.example.app");
    }

    #[tokio::test]
    async fn resolve_config_returns_config_not_found_for_missing_file() {
        let store = MemoryStore::new();
        let path = PathBuf::from("/project/appshots.json");
        let cache = Mutex::new(ProjectCache::new());

        let err = resolve_config(&store, &cache, &path).await.unwrap_err();
        assert!(matches!(err, AppShotsError::ConfigNotFound { .. }));
    }

    #[tokio::test]
    async fn mcp_log_with_none_peer_is_noop() {
        // Should not panic
        mcp_log(None, LoggingLevel::Info, "test message").await;
    }
}
