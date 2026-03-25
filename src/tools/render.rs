use std::path::Path;
use std::str::FromStr;

use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::config::Caption;
use crate::model::device::{self, Device};
use crate::model::locale::AsoLocale;
use crate::service::template_resolver;
use crate::service::typst_renderer::{self, RenderParams};
use crate::tools::{ProjectCache, resolve_config};

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

    let screenshots_base = project_dir.join("fastlane/screenshots");
    let mut screenshots = Vec::new();

    for &locale in &target_locales {
        let captions = load_captions_from_config(&config, &locale);

        for &mode in &target_modes {
            // Resolve template
            let template_path = template_resolver::resolve_template(base_dir, mode, |path| {
                store.exists(Path::new(path))
            })?;
            let template_source = store.read(&template_path.resolved)?;

            // Find caption for this mode
            let caption = captions.iter().find(|c| c.mode == mode);
            let title = caption.map(|c| c.title.as_str()).unwrap_or("Screenshot");
            let subtitle = caption.and_then(|c| c.subtitle.as_deref());
            let keyword = caption.and_then(|c| c.keyword.as_deref());

            // Get bg_colors from per_screen_overrides if available
            let bg_colors = config
                .per_screen_overrides
                .as_ref()
                .and_then(|o| o.get(&mode))
                .and_then(|o| o.bg_colors.clone())
                .unwrap_or_default();

            // Load screenshot capture if available
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

                let params = RenderParams {
                    template_source: template_source.clone(),
                    caption_title: title.to_owned(),
                    caption_subtitle: subtitle.map(|s| s.to_owned()),
                    keyword: keyword.map(|s| s.to_owned()),
                    bg_colors: bg_colors.clone(),
                    device: dev,
                    locale,
                    screenshot_data,
                    extra_fonts: vec![],
                };

                let result = typst_renderer::render_screenshot(&params)?;

                // Save to fastlane/screenshots/{locale}/{mode}_{device}.png
                let locale_dir = screenshots_base.join(locale.code());
                store.create_parent_dirs(&locale_dir.join("_"))?;

                let filename = format!("{mode}_{}.png", dev.display_name());
                let output_path = locale_dir.join(&filename);
                store.write_bytes(&output_path, &result.png_bytes)?;

                screenshots.push(ScreenshotInfo {
                    locale: locale.code().to_owned(),
                    mode,
                    device: dev.display_name().to_owned(),
                    output_path: output_path.to_string_lossy().into_owned(),
                    width: result.width,
                    height: result.height,
                });
            }
        }
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
}
