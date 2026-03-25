use std::fmt;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::color::OklchColor;

/// Whether the project uses a single shared template or per-screen templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateMode {
    Single,
    PerScreen,
}

/// Per-screen or global padding values (in points).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Padding {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Default for Padding {
    fn default() -> Self {
        Self {
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        }
    }
}

/// Design-time template configuration stored in `appshots.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfig {
    pub mode: TemplateMode,
    pub bg_colors: Vec<OklchColor>,
    pub text_color: OklchColor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size_title: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size_subtitle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_offset_y: Option<f64>,
}

/// A resolved template file path together with how it was found.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePath {
    pub resolved: PathBuf,
    pub source: ResolutionSource,
}

/// How a template path was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    /// Found a mode-specific template: `templates/template-{mode}.typ`
    ModeSpecific { mode: u8 },
    /// Fell back to shared template: `templates/template.typ`
    SharedFallback,
    /// Fell back to root template: `template.typ`
    RootFallback,
}

impl fmt::Display for ResolutionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModeSpecific { mode } => write!(f, "mode-specific (mode {mode})"),
            Self::SharedFallback => write!(f, "shared fallback"),
            Self::RootFallback => write!(f, "root fallback"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_mode_serde_single() {
        let json = serde_json::to_string(&TemplateMode::Single).unwrap();
        assert_eq!(json, r#""single""#);
        let back: TemplateMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TemplateMode::Single);
    }

    #[test]
    fn template_mode_serde_per_screen() {
        let json = serde_json::to_string(&TemplateMode::PerScreen).unwrap();
        assert_eq!(json, r#""per_screen""#);
        let back: TemplateMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TemplateMode::PerScreen);
    }

    #[test]
    fn template_config_serde_roundtrip_full() {
        let config = TemplateConfig {
            mode: TemplateMode::PerScreen,
            bg_colors: vec![
                OklchColor {
                    l: 70.0,
                    c: 0.15,
                    h: 250.0,
                    alpha: 1.0,
                },
                OklchColor {
                    l: 50.0,
                    c: 0.20,
                    h: 30.0,
                    alpha: 0.9,
                },
            ],
            text_color: OklchColor {
                l: 98.0,
                c: 0.0,
                h: 0.0,
                alpha: 1.0,
            },
            font_family: Some("SF Pro Display".into()),
            font_size_title: Some(48.0),
            font_size_subtitle: Some(24.0),
            padding: Some(Padding {
                top: 40.0,
                bottom: 20.0,
                left: 16.0,
                right: 16.0,
            }),
            screenshot_scale: Some(0.85),
            screenshot_offset_y: Some(120.0),
        };

        let json = serde_json::to_value(&config).unwrap();
        insta::assert_json_snapshot!("template_config_full", json);

        let back: TemplateConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.mode, TemplateMode::PerScreen);
        assert_eq!(back.bg_colors.len(), 2);
        assert_eq!(back.font_family.as_deref(), Some("SF Pro Display"));
    }

    #[test]
    fn template_config_required_fields_only() {
        let json = serde_json::json!({
            "mode": "single",
            "bgColors": [{"l": 70.0, "c": 0.15, "h": 250.0}],
            "textColor": {"l": 98.0, "c": 0.0, "h": 0.0}
        });

        let config: TemplateConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.mode, TemplateMode::Single);
        assert!(config.font_family.is_none());
        assert!(config.font_size_title.is_none());
        assert!(config.padding.is_none());
        assert!(config.screenshot_scale.is_none());
    }

    #[test]
    fn padding_default() {
        let pad = Padding::default();
        assert_eq!(pad.top, 0.0);
        assert_eq!(pad.bottom, 0.0);
        assert_eq!(pad.left, 0.0);
        assert_eq!(pad.right, 0.0);
    }

    #[test]
    fn resolution_source_display() {
        assert_eq!(
            ResolutionSource::ModeSpecific { mode: 3 }.to_string(),
            "mode-specific (mode 3)"
        );
        assert_eq!(
            ResolutionSource::SharedFallback.to_string(),
            "shared fallback"
        );
        assert_eq!(ResolutionSource::RootFallback.to_string(), "root fallback");
    }
}
