use std::path::Path;

use serde::Serialize;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::color::OklchColor;
use crate::model::device::Device;
use crate::model::locale::AsoLocale;
use crate::service::typst_renderer::{RenderParams, RenderResult};
use crate::service::{font_resolver, template_resolver};

/// Input parameters for preview_design.
pub struct PreviewParams<'a> {
    pub store: &'a dyn FileStore,
    pub project_dir: &'a Path,
    pub mode: u8,
    pub caption_title: &'a str,
    pub caption_subtitle: Option<&'a str>,
    pub bg_colors: Vec<OklchColor>,
    pub device: Device,
    pub locale: AsoLocale,
}

#[derive(Debug, Serialize)]
pub struct PreviewResult {
    pub preview_path: String,
    pub template_used: String,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
}

/// Render a single design preview.
pub(crate) async fn handle_preview_design(
    p: PreviewParams<'_>,
) -> Result<PreviewResult, AppShotsError> {
    let appshots_dir = p.project_dir.join("appshots");
    let base_dir = appshots_dir
        .to_str()
        .ok_or_else(|| AppShotsError::InvalidPath {
            path: appshots_dir.clone(),
            reason: "non-UTF-8 path".into(),
        })?;

    // Resolve template for this mode
    let template_path = template_resolver::resolve_template(base_dir, p.mode, |path| {
        p.store.exists(Path::new(path))
    })?;

    // Read template source
    let template_source = p.store.read(&template_path.resolved)?;

    // Load project fonts
    let project_fonts = super::load_project_fonts(p.store, p.project_dir);

    // Build render params
    let render_params = RenderParams {
        template_source,
        caption_title: p.caption_title.to_owned(),
        caption_subtitle: p.caption_subtitle.map(|s| s.to_owned()),
        keyword: None,
        bg_colors: p.bg_colors,
        device: p.device,
        locale: p.locale,
        screenshot_data: None,
        extra_fonts: project_fonts,
    };

    // Render
    let RenderResult {
        png_bytes,
        width,
        height,
        warnings,
    } = crate::service::typst_renderer::render_screenshot_async(&render_params).await?;

    // Save preview
    let preview_dir = appshots_dir.join("previews");
    p.store.create_parent_dirs(&preview_dir.join("_"))?;
    let preview_filename = format!("preview-{}-{}.png", p.mode, p.locale.code());
    let preview_path = preview_dir.join(&preview_filename);
    p.store.write_bytes(&preview_path, &png_bytes)?;

    Ok(PreviewResult {
        preview_path: preview_path.to_string_lossy().into_owned(),
        template_used: template_path.resolved.to_string_lossy().into_owned(),
        width,
        height,
        warnings,
    })
}

/// Save a Typst template to disk.
///
/// - `mode: None` → save to `appshots/template.typ` (single template)
/// - `mode: Some(N)` → save to `appshots/templates/template-{N}.typ` (per-screen)
pub(crate) async fn handle_save_template(
    store: &dyn FileStore,
    project_dir: &Path,
    template_source: &str,
    mode: Option<u8>,
) -> Result<serde_json::Value, AppShotsError> {
    let appshots_dir = project_dir.join("appshots");

    let template_path = match mode {
        None => appshots_dir.join("template.typ"),
        Some(m) => appshots_dir.join(format!("templates/template-{m}.typ")),
    };

    store.create_parent_dirs(&template_path)?;
    store.write(&template_path, template_source)?;

    Ok(serde_json::json!({
        "saved": template_path.to_string_lossy(),
        "mode": mode,
        "bytes": template_source.len(),
    }))
}

/// Read a template from disk.
///
/// - `mode: None` → read `appshots/template.typ`
/// - `mode: Some(N)` → resolve via template_resolver (mode-specific → shared → root)
pub(crate) async fn handle_get_template(
    store: &dyn FileStore,
    project_dir: &Path,
    mode: Option<u8>,
) -> Result<serde_json::Value, AppShotsError> {
    let appshots_dir = project_dir.join("appshots");
    let base_dir = appshots_dir
        .to_str()
        .ok_or_else(|| AppShotsError::InvalidPath {
            path: appshots_dir.clone(),
            reason: "non-UTF-8 path".into(),
        })?;

    let (path, source_desc) = match mode {
        None => {
            let p = appshots_dir.join("template.typ");
            (p, "single".to_owned())
        }
        Some(m) => {
            let resolved = template_resolver::resolve_template(base_dir, m, |path| {
                store.exists(Path::new(path))
            })?;
            let desc = format!("{:?}", resolved.source);
            (resolved.resolved, desc)
        }
    };

    let content = store.read(&path)?;

    Ok(serde_json::json!({
        "path": path.to_string_lossy(),
        "source": source_desc,
        "content": content,
    }))
}

/// Suggest a system font for a locale's script.
pub(crate) fn handle_suggest_font(locale: &AsoLocale) -> serde_json::Value {
    serde_json::json!({
        "locale": locale.code(),
        "script": format!("{:?}", locale.script()),
        "suggested_font": font_resolver::suggest_system_font(locale),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;
    use std::path::PathBuf;

    const MINIMAL_TEMPLATE: &str = r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
#sys.inputs.caption_title"#;

    fn setup_store_with_template(store: &MemoryStore, project_dir: &Path) {
        let template_path = project_dir.join("appshots/template.typ");
        store.write(&template_path, MINIMAL_TEMPLATE).unwrap();
    }

    #[tokio::test]
    async fn preview_design_produces_png() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store_with_template(&store, &project_dir);

        let result = handle_preview_design(PreviewParams {
            store: &store,
            project_dir: &project_dir,
            mode: 1,
            caption_title: "Hello World",
            caption_subtitle: None,
            bg_colors: vec![],
            device: Device::Iphone6_9,
            locale: AsoLocale::EnUs,
        })
        .await;

        assert!(result.is_ok(), "preview failed: {:?}", result.err());
        let result = result.unwrap();
        assert!(result.width > 0);
        assert!(result.height > 0);
        assert!(result.preview_path.contains("preview-1-en-US.png"));

        // Verify PNG was written to store
        let written = store.read_bytes(Path::new(&result.preview_path)).unwrap();
        assert!(written.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    }

    #[tokio::test]
    async fn preview_design_uses_correct_template() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");

        // Set up mode-specific template
        let mode_template_path = project_dir.join("appshots/templates/template-2.typ");
        store
            .write(
                &mode_template_path,
                r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
Mode 2"#,
            )
            .unwrap();

        let result = handle_preview_design(PreviewParams {
            store: &store,
            project_dir: &project_dir,
            mode: 2,
            caption_title: "Test",
            caption_subtitle: None,
            bg_colors: vec![],
            device: Device::Iphone6_9,
            locale: AsoLocale::EnUs,
        })
        .await
        .unwrap();

        assert!(result.template_used.contains("template-2.typ"));
    }

    #[tokio::test]
    async fn preview_design_template_not_found() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        // No template files

        let result = handle_preview_design(PreviewParams {
            store: &store,
            project_dir: &project_dir,
            mode: 1,
            caption_title: "Hello",
            caption_subtitle: None,
            bg_colors: vec![],
            device: Device::Iphone6_9,
            locale: AsoLocale::EnUs,
        })
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppShotsError::TemplateNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn preview_design_with_subtitle_and_colors() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store_with_template(&store, &project_dir);

        let colors = vec![OklchColor {
            l: 50.0,
            c: 0.15,
            h: 240.0,
            alpha: 1.0,
        }];

        let result = handle_preview_design(PreviewParams {
            store: &store,
            project_dir: &project_dir,
            mode: 1,
            caption_title: "Title",
            caption_subtitle: Some("Subtitle"),
            bg_colors: colors,
            device: Device::Ipad13,
            locale: AsoLocale::FrFr,
        })
        .await;

        assert!(result.is_ok(), "preview failed: {:?}", result.err());
        let result = result.unwrap();
        assert!(result.preview_path.contains("fr-FR"));
    }

    // --- save_template / get_template tests ---

    #[tokio::test]
    async fn save_and_get_template_roundtrip_single() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        let source = "#set page(width: 440pt, height: 956pt)\nHello";

        let save_result = handle_save_template(&store, &project_dir, source, None)
            .await
            .unwrap();
        assert!(
            save_result["saved"]
                .as_str()
                .unwrap()
                .contains("template.typ")
        );

        let get_result = handle_get_template(&store, &project_dir, None)
            .await
            .unwrap();
        assert_eq!(get_result["content"].as_str().unwrap(), source);
        assert_eq!(get_result["source"].as_str().unwrap(), "single");
    }

    #[tokio::test]
    async fn save_and_get_template_per_screen() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        let source = "#set page()\nMode 3 template";

        handle_save_template(&store, &project_dir, source, Some(3))
            .await
            .unwrap();

        let result = handle_get_template(&store, &project_dir, Some(3))
            .await
            .unwrap();
        assert_eq!(result["content"].as_str().unwrap(), source);
        assert!(result["path"].as_str().unwrap().contains("template-3.typ"));
    }

    #[tokio::test]
    async fn get_template_not_found() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");

        let result = handle_get_template(&store, &project_dir, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_template_mode_fallback() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");

        // Only save root template
        let source = "root fallback";
        handle_save_template(&store, &project_dir, source, None)
            .await
            .unwrap();

        // Request mode 5 — should fall back to root
        let result = handle_get_template(&store, &project_dir, Some(5))
            .await
            .unwrap();
        assert_eq!(result["content"].as_str().unwrap(), source);
    }

    // --- suggest_font tests ---

    #[test]
    fn suggest_font_latin() {
        let result = handle_suggest_font(&AsoLocale::EnUs);
        assert_eq!(result["suggested_font"], "SF Pro Display");
        assert_eq!(result["locale"], "en-US");
        assert_eq!(result["script"], "Latin");
    }

    #[test]
    fn suggest_font_cjk_japanese() {
        let result = handle_suggest_font(&AsoLocale::Ja);
        assert_eq!(result["suggested_font"], "Hiragino Sans");
        assert_eq!(result["script"], "CJK");
    }

    #[test]
    fn suggest_font_arabic() {
        let result = handle_suggest_font(&AsoLocale::ArSa);
        assert_eq!(result["suggested_font"], "SF Arabic");
        assert_eq!(result["script"], "Arabic");
    }

    #[test]
    fn suggest_font_all_scripts_return_value() {
        // Verify no panic for all locales
        for locale in crate::model::locale::ALL {
            let result = handle_suggest_font(locale);
            assert!(result["suggested_font"].is_string());
            assert!(result["locale"].is_string());
            assert!(result["script"].is_string());
        }
    }
}
