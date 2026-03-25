use std::path::Path;

use serde::Serialize;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::color::OklchColor;
use crate::model::device::Device;
use crate::model::locale::AsoLocale;
use crate::service::template_resolver;
use crate::service::typst_renderer::{RenderParams, RenderResult};

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
        extra_fonts: vec![],
    };

    // Render
    let RenderResult {
        png_bytes,
        width,
        height,
        warnings,
    } = crate::service::typst_renderer::render_screenshot(&render_params)?;

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
}
