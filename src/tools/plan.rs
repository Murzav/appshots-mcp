use std::path::Path;

use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::config::ScreenPlan;
use crate::service::config_parser;

use super::{CachedConfig, ProjectCache};

#[derive(Debug, Serialize)]
pub(crate) struct PlanResult {
    pub plans: Vec<ScreenPlan>,
    pub total_modes: usize,
}

/// Save screen plans to appshots.json. Upsert: only updates modes present in input.
pub(crate) async fn handle_plan_screens(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    write_lock: &Mutex<()>,
    config_path: &Path,
    plans: Vec<ScreenPlan>,
) -> Result<PlanResult, AppShotsError> {
    let _guard = write_lock.lock().await;

    // Re-read fresh from disk (write-lock pattern)
    let raw = store.read(config_path)?;
    let mut config = config_parser::parse_config(&raw)?;

    // Read existing plans from extra["plans"], default to empty array
    let mut existing: Vec<ScreenPlan> = config
        .extra
        .get("plans")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Upsert: replace by mode, append new
    for plan in plans {
        if let Some(pos) = existing.iter().position(|p| p.mode == plan.mode) {
            existing[pos] = plan;
        } else {
            existing.push(plan);
        }
    }

    // Sort by mode for deterministic output
    existing.sort_by_key(|p| p.mode);

    config.extra.insert(
        "plans".to_owned(),
        serde_json::to_value(&existing).map_err(|e| AppShotsError::JsonParse(e.to_string()))?,
    );

    // Write back
    let json = config_parser::serialize_config(&config)?;
    store.write(config_path, &json)?;

    // Update cache
    let mtime = store.modified_time(config_path)?;
    let total_modes = existing.len();
    let mut cache_guard = cache.lock().await;
    cache_guard.config = Some(CachedConfig {
        config,
        modified: mtime,
    });

    Ok(PlanResult {
        plans: existing,
        total_modes,
    })
}

/// Get current screen plans from appshots.json.
pub(crate) async fn handle_get_plans(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    config_path: &Path,
) -> Result<PlanResult, AppShotsError> {
    let config = super::resolve_config(store, cache, config_path).await?;

    let plans: Vec<ScreenPlan> = config
        .extra
        .get("plans")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let total_modes = plans.len();
    Ok(PlanResult { plans, total_modes })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio::sync::Mutex;

    use crate::io::memory::MemoryStore;
    use crate::model::config::ScreenPlan;
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

    fn sample_plan(mode: u8) -> ScreenPlan {
        ScreenPlan {
            mode,
            target_keywords: vec!["keyword1".into(), "keyword2".into()],
            messaging_angle: format!("Angle for mode {mode}"),
            notes: None,
        }
    }

    fn setup() -> (MemoryStore, Mutex<ProjectCache>, Mutex<()>) {
        let store = MemoryStore::new();
        let config_path = Path::new("/project/appshots.json");
        store.write(config_path, minimal_config_json()).unwrap();
        (store, Mutex::new(ProjectCache::new()), Mutex::new(()))
    }

    #[tokio::test]
    async fn save_plan_get_plan_roundtrip() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        let plans = vec![sample_plan(1), sample_plan(2)];
        let result = handle_plan_screens(&store, &cache, &write_lock, config_path, plans)
            .await
            .unwrap();

        assert_eq!(result.total_modes, 2);
        assert_eq!(result.plans.len(), 2);
        assert_eq!(result.plans[0].mode, 1);
        assert_eq!(result.plans[1].mode, 2);

        // Verify via get
        let get_result = handle_get_plans(&store, &cache, config_path).await.unwrap();
        assert_eq!(get_result.total_modes, 2);
        assert_eq!(get_result.plans[0].messaging_angle, "Angle for mode 1");
    }

    #[tokio::test]
    async fn upsert_preserves_other_modes() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        // Save modes 1, 2, 3
        let plans = vec![sample_plan(1), sample_plan(2), sample_plan(3)];
        handle_plan_screens(&store, &cache, &write_lock, config_path, plans)
            .await
            .unwrap();

        // Update only mode 2
        let updated = ScreenPlan {
            mode: 2,
            target_keywords: vec!["updated".into()],
            messaging_angle: "Updated angle".into(),
            notes: Some("new note".into()),
        };
        let result = handle_plan_screens(&store, &cache, &write_lock, config_path, vec![updated])
            .await
            .unwrap();

        assert_eq!(result.total_modes, 3);
        assert_eq!(result.plans[0].messaging_angle, "Angle for mode 1");
        assert_eq!(result.plans[1].messaging_angle, "Updated angle");
        assert_eq!(result.plans[1].target_keywords, vec!["updated"]);
        assert_eq!(result.plans[1].notes.as_deref(), Some("new note"));
        assert_eq!(result.plans[2].messaging_angle, "Angle for mode 3");
    }

    #[tokio::test]
    async fn get_plans_empty_returns_empty() {
        let (store, cache, _) = setup();
        let config_path = Path::new("/project/appshots.json");

        let result = handle_get_plans(&store, &cache, config_path).await.unwrap();
        assert_eq!(result.total_modes, 0);
        assert!(result.plans.is_empty());
    }

    #[tokio::test]
    async fn plans_sorted_by_mode() {
        let (store, cache, write_lock) = setup();
        let config_path = Path::new("/project/appshots.json");

        let plans = vec![sample_plan(5), sample_plan(1), sample_plan(3)];
        let result = handle_plan_screens(&store, &cache, &write_lock, config_path, plans)
            .await
            .unwrap();

        assert_eq!(result.plans[0].mode, 1);
        assert_eq!(result.plans[1].mode, 3);
        assert_eq!(result.plans[2].mode, 5);
    }
}
