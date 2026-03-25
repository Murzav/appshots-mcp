use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::color::OklchColor;
use crate::model::config::Caption;
use crate::model::device::{self, Device};
use crate::model::locale::AsoLocale;
use crate::service::template_resolver;
use crate::service::typst_renderer::{self, RenderParams, RenderResult};
use crate::service::typst_world;
use crate::tools::{ProjectCache, resolve_config};

const MAX_CONCURRENT_RENDERS: usize = 4;

#[derive(Debug, Serialize)]
pub struct ComposeResult {
    pub rendered: usize,
    pub output_dir: String,
    pub screenshots: Vec<ScreenshotInfo>,
}

#[derive(Debug, Serialize)]
pub struct ScreenshotInfo {
    pub locale: String,
    pub mode: u8,
    pub device: String,
    pub output_path: String,
    pub width: u32,
    pub height: u32,
}

/// Load captions for a locale from the project config's `extra["captions"][locale]`.
/// This is where `save_captions` stores them.
fn load_captions_from_config(
    config: &crate::model::config::ProjectConfig,
    locale: &AsoLocale,
) -> Vec<Caption> {
    let Some(captions_val) = config.extra.get("captions") else {
        return vec![];
    };
    let Some(locale_val) = captions_val.get(locale.code()) else {
        return vec![];
    };
    serde_json::from_value::<Vec<Caption>>(locale_val.clone()).unwrap_or_default()
}

/// Compose final screenshots via Typst rendering.
pub(crate) async fn handle_compose_screenshots(
    store: &dyn FileStore,
    cache: &Mutex<ProjectCache>,
    config_path: &Path,
    project_dir: &Path,
    modes: Option<Vec<u8>>,
    locales: Option<Vec<String>>,
) -> Result<ComposeResult, AppShotsError> {
    let config = resolve_config(store, cache, config_path).await?;

    let appshots_dir = project_dir.join("appshots");
    let base_dir = appshots_dir
        .to_str()
        .ok_or_else(|| AppShotsError::InvalidPath {
            path: appshots_dir.clone(),
            reason: "non-UTF-8 path".into(),
        })?;

    // Determine target modes from config + filter
    let target_modes: Vec<u8> = {
        let all_modes: Vec<u8> = config.screens.iter().map(|s| s.mode).collect();
        match modes {
            Some(ref filter) => all_modes
                .into_iter()
                .filter(|m| filter.contains(m))
                .collect(),
            None => all_modes,
        }
    };

    // Determine target locales (None = all from scanned metadata)
    let target_locales: Vec<AsoLocale> = match locales {
        Some(ref codes) => codes
            .iter()
            .map(|c| AsoLocale::from_str(c))
            .collect::<Result<Vec<_>, _>>()?,
        None => {
            let guard = cache.lock().await;
            let mut all: Vec<AsoLocale> = guard.metadata.keys().copied().collect();
            all.sort_by(|a, b| a.code().cmp(b.code()));
            if all.is_empty() {
                vec![AsoLocale::EnUs] // fallback if no scan was done
            } else {
                all
            }
        }
    };

    // Determine devices from config
    let target_devices: &[Device] = if config.devices.is_empty() {
        device::REQUIRED
    } else {
        &config.devices
    };

    // Load project fonts once before the render loop
    let project_fonts: Arc<Vec<Vec<u8>>> =
        Arc::new(typst_world::load_project_fonts(store, project_dir));

    let screenshots_base = project_dir.join("fastlane/screenshots");

    // Task A: Cache template sources by mode before the main loop
    let mut template_cache: HashMap<u8, Arc<String>> = HashMap::with_capacity(target_modes.len());
    for &mode in &target_modes {
        let template_path = template_resolver::resolve_template(base_dir, mode, |path| {
            store.exists(Path::new(path))
        })?;
        let source = store.read(&template_path.resolved)?;
        template_cache.insert(mode, Arc::new(source));
    }

    // Pre-compute bg_colors per mode
    let mut bg_colors_cache: HashMap<u8, Vec<OklchColor>> =
        HashMap::with_capacity(target_modes.len());
    for &mode in &target_modes {
        let bg_colors = config
            .per_screen_overrides
            .as_ref()
            .and_then(|o| o.get(&mode))
            .and_then(|o| o.bg_colors.clone())
            .unwrap_or_default();
        bg_colors_cache.insert(mode, bg_colors);
    }

    // Collect all render combos: read I/O upfront, then render in parallel
    struct RenderCombo {
        locale: AsoLocale,
        mode: u8,
        device: Device,
        output_path: PathBuf,
        params: RenderParams,
    }

    let mut combos = Vec::new();

    for &locale in &target_locales {
        let captions = load_captions_from_config(&config, &locale);

        for &mode in &target_modes {
            let template_source = Arc::clone(template_cache.get(&mode).expect("mode was cached"));

            let caption = captions.iter().find(|c| c.mode == mode);
            let title = caption.map(|c| c.title.as_str()).unwrap_or("Screenshot");
            let subtitle = caption.and_then(|c| c.subtitle.as_deref());
            let keyword = caption.and_then(|c| c.keyword.as_deref());
            let bg_colors = bg_colors_cache.get(&mode).cloned().unwrap_or_default();

            for &dev in target_devices {
                let capture_path = appshots_dir
                    .join("captures")
                    .join(dev.display_name())
                    .join(locale.code())
                    .join(format!("mode-{mode}.png"));
                let screenshot_data = if store.exists(&capture_path) {
                    Some(store.read_bytes(&capture_path)?)
                } else {
                    None
                };

                let locale_dir = screenshots_base.join(locale.code());
                let filename = format!("{mode}_{}.png", dev.display_name());
                let output_path = locale_dir.join(&filename);

                let params = RenderParams {
                    template_source: (*template_source).clone(),
                    caption_title: title.to_owned(),
                    caption_subtitle: subtitle.map(|s| s.to_owned()),
                    keyword: keyword.map(|s| s.to_owned()),
                    bg_colors: bg_colors.clone(),
                    device: dev,
                    locale,
                    screenshot_data,
                    extra_fonts: (*project_fonts).clone(),
                };

                combos.push(RenderCombo {
                    locale,
                    mode,
                    device: dev,
                    output_path,
                    params,
                });
            }
        }
    }

    // Task B: Parallel rendering with semaphore
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_RENDERS));
    let mut join_set = JoinSet::new();

    for (idx, combo) in combos.into_iter().enumerate() {
        let sem = Arc::clone(&semaphore);
        join_set.spawn(async move {
            let _permit = sem
                .acquire()
                .await
                .map_err(|e| AppShotsError::RenderError(format!("semaphore closed: {e}")))?;
            let result = typst_renderer::render_screenshot_async(&combo.params).await?;
            Ok::<(usize, AsoLocale, u8, Device, PathBuf, RenderResult), AppShotsError>((
                idx,
                combo.locale,
                combo.mode,
                combo.device,
                combo.output_path,
                result,
            ))
        });
    }

    // Collect results, preserving deterministic order
    let mut indexed_results = Vec::new();
    while let Some(join_result) = join_set.join_next().await {
        let render_result = join_result
            .map_err(|e| AppShotsError::RenderError(format!("render task panicked: {e}")))??;
        indexed_results.push(render_result);
    }
    indexed_results.sort_by_key(|(idx, ..)| *idx);

    // Write outputs sequentially (FileStore is borrowed, not Send into tasks)
    let mut screenshots = Vec::with_capacity(indexed_results.len());
    for (_idx, locale, mode, device, output_path, result) in indexed_results {
        store.create_parent_dirs(&output_path)?;
        store.write_bytes(&output_path, &result.png_bytes)?;

        screenshots.push(ScreenshotInfo {
            locale: locale.code().to_owned(),
            mode,
            device: device.display_name().to_owned(),
            output_path: output_path.to_string_lossy().into_owned(),
            width: result.width,
            height: result.height,
        });
    }

    Ok(ComposeResult {
        rendered: screenshots.len(),
        output_dir: screenshots_base.to_string_lossy().into_owned(),
        screenshots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;
    use std::path::PathBuf;

    const MINIMAL_TEMPLATE: &str = r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
#sys.inputs.caption_title"#;

    fn minimal_config_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [{"mode": 1, "name": "Home"}],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#
    }

    fn setup_store(store: &MemoryStore, project_dir: &Path) {
        // Config
        let config_path = project_dir.join("appshots.json");
        store.write(&config_path, minimal_config_json()).unwrap();

        // Template
        let template_path = project_dir.join("appshots/template.typ");
        store.write(&template_path, MINIMAL_TEMPLATE).unwrap();
    }

    #[tokio::test]
    async fn compose_single_screenshot() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store(&store, &project_dir);

        let cache = Mutex::new(ProjectCache::new());
        let config_path = project_dir.join("appshots.json");

        let result = handle_compose_screenshots(
            &store,
            &cache,
            &config_path,
            &project_dir,
            Some(vec![1]),
            Some(vec!["en-US".to_owned()]),
        )
        .await;

        assert!(result.is_ok(), "compose failed: {:?}", result.err());
        let result = result.unwrap();
        assert_eq!(result.rendered, 1); // 1 mode × 1 locale × 1 device
        assert_eq!(result.screenshots[0].mode, 1);
        assert_eq!(result.screenshots[0].locale, "en-US");

        // Verify file was written
        let path = Path::new(&result.screenshots[0].output_path);
        assert!(store.exists(path));
    }

    #[tokio::test]
    async fn compose_with_captions_from_config() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");

        // Config with embedded captions
        let config_with_captions = r#"{
            "bundleId": "com.example.app",
            "screens": [{"mode": 1, "name": "Dashboard"}],
            "templateMode": "single",
            "devices": [],
            "captions": {
                "en-US": [{"mode": 1, "title": "Track Easily", "subtitle": "With one tap"}]
            }
        }"#;
        let config_path = project_dir.join("appshots.json");
        store.write(&config_path, config_with_captions).unwrap();

        // Template
        let template_path = project_dir.join("appshots/template.typ");
        store.write(&template_path, MINIMAL_TEMPLATE).unwrap();

        let cache = Mutex::new(ProjectCache::new());

        let result = handle_compose_screenshots(
            &store,
            &cache,
            &config_path,
            &project_dir,
            Some(vec![1]),
            Some(vec!["en-US".to_owned()]),
        )
        .await
        .unwrap();

        // 1 mode × 1 locale × 2 devices (REQUIRED) = 2 screenshots
        assert_eq!(result.rendered, 2);
    }

    #[tokio::test]
    async fn compose_config_not_found() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        let cache = Mutex::new(ProjectCache::new());
        let config_path = project_dir.join("appshots.json");

        let result =
            handle_compose_screenshots(&store, &cache, &config_path, &project_dir, None, None)
                .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppShotsError::ConfigNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn compose_template_not_found() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");

        // Config but no template
        let config_path = project_dir.join("appshots.json");
        store.write(&config_path, minimal_config_json()).unwrap();

        let cache = Mutex::new(ProjectCache::new());

        let result = handle_compose_screenshots(
            &store,
            &cache,
            &config_path,
            &project_dir,
            Some(vec![1]),
            Some(vec!["en-US".to_owned()]),
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppShotsError::TemplateNotFound { .. }
        ));
    }

    fn make_config() -> crate::model::config::ProjectConfig {
        serde_json::from_str(minimal_config_json()).unwrap()
    }

    #[test]
    fn load_captions_from_config_missing_returns_empty() {
        let config = make_config();
        let captions = load_captions_from_config(&config, &AsoLocale::EnUs);
        assert!(captions.is_empty());
    }

    #[test]
    fn load_captions_from_config_parses_stored_captions() {
        let mut config = make_config();
        let locale_captions = serde_json::json!([
            {"mode": 1, "title": "Hello"},
            {"mode": 2, "title": "World", "subtitle": "Sub"}
        ]);
        let mut captions_map = serde_json::Map::new();
        captions_map.insert("en-US".to_owned(), locale_captions);
        config.extra.insert(
            "captions".to_owned(),
            serde_json::Value::Object(captions_map),
        );

        let captions = load_captions_from_config(&config, &AsoLocale::EnUs);
        assert_eq!(captions.len(), 2);
        assert_eq!(captions[0].title, "Hello");
        assert_eq!(captions[1].subtitle.as_deref(), Some("Sub"));
    }

    #[tokio::test]
    async fn compose_multiple_modes_batch_count() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");

        let config_json = r#"{
            "bundleId": "com.example.app",
            "screens": [
                {"mode": 1, "name": "Home"},
                {"mode": 2, "name": "Settings"},
                {"mode": 3, "name": "Profile"}
            ],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#;
        let config_path = project_dir.join("appshots.json");
        store.write(&config_path, config_json).unwrap();

        let template_path = project_dir.join("appshots/template.typ");
        store.write(&template_path, MINIMAL_TEMPLATE).unwrap();

        let cache = Mutex::new(ProjectCache::new());

        let result = handle_compose_screenshots(
            &store,
            &cache,
            &config_path,
            &project_dir,
            None,
            Some(vec!["en-US".to_owned(), "ja".to_owned()]),
        )
        .await
        .unwrap();

        // 3 modes × 2 locales × 1 device = 6 screenshots
        assert_eq!(result.rendered, 6);
    }
}
