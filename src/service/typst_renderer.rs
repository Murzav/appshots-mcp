use std::collections::HashMap;

use typst::foundations::{Dict, IntoValue, Str};

use crate::error::AppShotsError;
use crate::model::color::OklchColor;
use crate::model::device::Device;
use crate::model::locale::AsoLocale;
use crate::service::locale::text_direction;
use crate::service::typst_world::{AppWorld, compile_template};

/// Parameters for rendering a single screenshot.
pub struct RenderParams {
    pub template_source: String,
    pub caption_title: String,
    pub caption_subtitle: Option<String>,
    pub keyword: Option<String>,
    pub bg_colors: Vec<OklchColor>,
    pub device: Device,
    pub locale: AsoLocale,
    pub screenshot_data: Option<Vec<u8>>,
    pub extra_fonts: Vec<Vec<u8>>,
}

/// Render result with PNG bytes and metadata.
pub struct RenderResult {
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
}

/// Build the `sys.inputs` dictionary from render params.
fn build_inputs(params: &RenderParams) -> Dict {
    let mut inputs = Dict::new();

    inputs.insert(
        "caption_title".into(),
        Str::from(params.caption_title.as_str()).into_value(),
    );

    if let Some(ref subtitle) = params.caption_subtitle {
        inputs.insert(
            "caption_subtitle".into(),
            Str::from(subtitle.as_str()).into_value(),
        );
    }

    if let Some(ref keyword) = params.keyword {
        inputs.insert("keyword".into(), Str::from(keyword.as_str()).into_value());
    }

    // Background color (first color as typst string)
    if let Some(first) = params.bg_colors.first() {
        inputs.insert(
            "bg_color".into(),
            Str::from(first.to_typst().as_str()).into_value(),
        );
    }

    // Background gradient (all colors as comma-separated string)
    if !params.bg_colors.is_empty() {
        let gradient: String = params
            .bg_colors
            .iter()
            .map(|c| c.to_typst())
            .collect::<Vec<_>>()
            .join(", ");
        inputs.insert(
            "bg_gradient".into(),
            Str::from(gradient.as_str()).into_value(),
        );
    }

    // Device dimensions
    let (w, h) = params.device.canvas_size();
    inputs.insert(
        "device_width".into(),
        Str::from(w.to_string().as_str()).into_value(),
    );
    inputs.insert(
        "device_height".into(),
        Str::from(h.to_string().as_str()).into_value(),
    );

    // Locale
    inputs.insert(
        "locale".into(),
        Str::from(params.locale.code()).into_value(),
    );

    // Text direction
    inputs.insert(
        "text_direction".into(),
        Str::from(text_direction(&params.locale)).into_value(),
    );

    inputs
}

/// Render a screenshot template to PNG bytes.
pub fn render_screenshot(params: &RenderParams) -> Result<RenderResult, AppShotsError> {
    let inputs = build_inputs(params);

    // Build files map for embedded images
    let mut files = HashMap::new();
    if let Some(ref data) = params.screenshot_data {
        files.insert("/screenshot.png".to_owned(), data.clone());
    }

    // Create world and compile
    let world = AppWorld::new(
        &params.template_source,
        inputs,
        params.extra_fonts.clone(),
        files,
    );
    let (document, warnings) = compile_template(&world)?;

    if document.pages.is_empty() {
        return Err(AppShotsError::RenderError(
            "template produced no pages".into(),
        ));
    }

    let page = &document.pages[0];
    let page_size = page.frame.size();

    // Calculate pixel_per_pt to match target device size
    let (target_w, _target_h) = params.device.canvas_size();
    let page_width_pt = page_size.x.to_pt() as f32;
    let pixel_per_pt = if page_width_pt > 0.0 {
        target_w as f32 / page_width_pt
    } else {
        2.0
    };

    let pixmap = typst_render::render(page, pixel_per_pt);
    let width = pixmap.width();
    let height = pixmap.height();

    let png_bytes = pixmap
        .encode_png()
        .map_err(|e| AppShotsError::RenderError(format!("PNG encoding failed: {e}")))?;

    Ok(RenderResult {
        png_bytes,
        width,
        height,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_params(template: &str) -> RenderParams {
        RenderParams {
            template_source: template.to_owned(),
            caption_title: "Hello World".to_owned(),
            caption_subtitle: None,
            keyword: None,
            bg_colors: vec![],
            device: Device::Iphone6_9,
            locale: AsoLocale::EnUs,
            screenshot_data: None,
            extra_fonts: vec![],
        }
    }

    #[test]
    fn render_minimal_template_produces_valid_png() {
        let params = minimal_params(
            r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
Hello World"#,
        );
        let result = render_screenshot(&params);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        let result = result.unwrap();
        // PNG signature: 0x89 0x50 0x4E 0x47
        assert!(result.png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(result.width > 0);
        assert!(result.height > 0);
    }

    #[test]
    fn render_with_oklch_gradient() {
        let params = RenderParams {
            template_source: r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
#sys.inputs.bg_color"#
                .to_owned(),
            caption_title: "Test".to_owned(),
            caption_subtitle: None,
            keyword: None,
            bg_colors: vec![
                OklchColor {
                    l: 50.0,
                    c: 0.15,
                    h: 240.0,
                    alpha: 1.0,
                },
                OklchColor {
                    l: 70.0,
                    c: 0.2,
                    h: 300.0,
                    alpha: 1.0,
                },
            ],
            device: Device::Iphone6_9,
            locale: AsoLocale::EnUs,
            screenshot_data: None,
            extra_fonts: vec![],
        };
        let result = render_screenshot(&params);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn render_with_inputs_accessible() {
        let params = RenderParams {
            template_source: r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
#sys.inputs.caption_title
#sys.inputs.locale"#
                .to_owned(),
            caption_title: "My Caption".to_owned(),
            caption_subtitle: Some("Subtitle".to_owned()),
            keyword: Some("productivity".to_owned()),
            bg_colors: vec![],
            device: Device::Iphone6_9,
            locale: AsoLocale::FrFr,
            screenshot_data: None,
            extra_fonts: vec![],
        };
        let result = render_screenshot(&params);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
    }

    #[test]
    fn render_compilation_error_returns_template_compile_error() {
        let params = minimal_params("#let x = ");
        let result = render_screenshot(&params);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            matches!(err, AppShotsError::TemplateCompileError(_)),
            "expected TemplateCompileError, got: {err:?}"
        );
    }

    #[test]
    fn render_result_dimensions_match_device() {
        let params = minimal_params(
            r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
Hello"#,
        );
        let result = render_screenshot(&params).unwrap();
        // The template sets 440pt width, device is 1320px → pixel_per_pt = 3.0
        // Allow small rounding tolerance
        assert_eq!(result.width, 1320);
    }

    #[test]
    fn build_inputs_includes_all_fields() {
        let params = RenderParams {
            template_source: String::new(),
            caption_title: "Title".to_owned(),
            caption_subtitle: Some("Sub".to_owned()),
            keyword: Some("kw".to_owned()),
            bg_colors: vec![OklchColor {
                l: 50.0,
                c: 0.1,
                h: 200.0,
                alpha: 1.0,
            }],
            device: Device::Ipad13,
            locale: AsoLocale::ArSa,
            screenshot_data: None,
            extra_fonts: vec![],
        };
        let inputs = build_inputs(&params);
        assert!(inputs.get(&Str::from("caption_title")).is_ok());
        assert!(inputs.get(&Str::from("caption_subtitle")).is_ok());
        assert!(inputs.get(&Str::from("keyword")).is_ok());
        assert!(inputs.get(&Str::from("bg_color")).is_ok());
        assert!(inputs.get(&Str::from("bg_gradient")).is_ok());
        assert!(inputs.get(&Str::from("device_width")).is_ok());
        assert!(inputs.get(&Str::from("device_height")).is_ok());
        assert!(inputs.get(&Str::from("locale")).is_ok());
        assert!(inputs.get(&Str::from("text_direction")).is_ok());
    }
}
