use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::color::OklchColor;
use super::device::Device;
use super::template::TemplateMode;

/// Top-level project configuration (`appshots.json`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub bundle_id: String,
    pub screens: Vec<ScreenConfig>,
    pub template_mode: TemplateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_screen_overrides: Option<IndexMap<u8, PerScreenOverride>>,
    pub devices: Vec<Device>,
    #[serde(flatten)]
    pub extra: IndexMap<String, serde_json::Value>,
}

/// A single screenshot screen definition.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScreenConfig {
    pub mode: u8,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Caption text for a single screen in a single locale.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Caption {
    pub mode: u8,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
}

/// Per-screen design overrides.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PerScreenOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_colors: Option<Vec<OklchColor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_override: Option<String>,
}

/// AI-generated plan for a single screenshot screen.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPlan {
    pub mode: u8,
    pub target_keywords: Vec<String>,
    pub messaging_angle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Parsed fastlane metadata for a single locale.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocaleMetadata {
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

impl LocaleMetadata {
    /// Number of keywords (computed, not stored).
    pub fn keyword_count(&self) -> usize {
        self.keywords.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project_config() -> ProjectConfig {
        ProjectConfig {
            bundle_id: "com.example.app".into(),
            screens: vec![
                ScreenConfig {
                    mode: 1,
                    name: "Home".into(),
                    description: Some("Main home screen".into()),
                },
                ScreenConfig {
                    mode: 2,
                    name: "Settings".into(),
                    description: None,
                },
            ],
            template_mode: TemplateMode::PerScreen,
            per_screen_overrides: Some({
                let mut map = IndexMap::new();
                map.insert(
                    1,
                    PerScreenOverride {
                        bg_colors: Some(vec![OklchColor {
                            l: 70.0,
                            c: 0.15,
                            h: 250.0,
                            alpha: 1.0,
                        }]),
                        font_override: Some("Menlo".into()),
                    },
                );
                map
            }),
            devices: vec![Device::Iphone6_9, Device::Ipad13],
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn project_config_serde_roundtrip() {
        let config = sample_project_config();
        let json = serde_json::to_value(&config).unwrap();
        insta::assert_json_snapshot!("project_config_full", json);

        let back: ProjectConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.bundle_id, "com.example.app");
        assert_eq!(back.screens.len(), 2);
        assert_eq!(back.devices.len(), 2);
    }

    #[test]
    fn caption_serde_optional_fields_omitted() {
        let caption = Caption {
            mode: 1,
            title: "Fast & Easy".into(),
            subtitle: None,
            keyword: None,
        };
        let json = serde_json::to_value(&caption).unwrap();
        assert!(!json.as_object().unwrap().contains_key("subtitle"));
        assert!(!json.as_object().unwrap().contains_key("keyword"));

        let back: Caption = serde_json::from_value(json).unwrap();
        assert_eq!(back.mode, 1);
        assert!(back.subtitle.is_none());
    }

    #[test]
    fn per_screen_override_oklch_roundtrip() {
        let ovr = PerScreenOverride {
            bg_colors: Some(vec![
                OklchColor {
                    l: 60.0,
                    c: 0.2,
                    h: 120.0,
                    alpha: 1.0,
                },
                OklchColor {
                    l: 80.0,
                    c: 0.1,
                    h: 300.0,
                    alpha: 0.5,
                },
            ]),
            font_override: None,
        };
        let json = serde_json::to_value(&ovr).unwrap();
        let back: PerScreenOverride = serde_json::from_value(json).unwrap();
        assert_eq!(back.bg_colors.as_ref().unwrap().len(), 2);
        assert!(back.font_override.is_none());
    }

    #[test]
    fn minimal_project_config() {
        let json = serde_json::json!({
            "bundleId": "com.test.min",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        });
        let config: ProjectConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.bundle_id, "com.test.min");
        assert!(config.screens.is_empty());
        assert!(config.per_screen_overrides.is_none());
        assert_eq!(config.devices.len(), 1);
    }

    #[test]
    fn indexmap_preserves_order() {
        let mut extra = IndexMap::new();
        extra.insert("zebra".to_owned(), serde_json::json!("z"));
        extra.insert("alpha".to_owned(), serde_json::json!("a"));
        extra.insert("middle".to_owned(), serde_json::json!("m"));

        let config = ProjectConfig {
            bundle_id: "com.test.order".into(),
            screens: vec![],
            template_mode: TemplateMode::Single,
            per_screen_overrides: None,
            devices: vec![],
            extra,
        };

        let json_str = serde_json::to_string(&config).unwrap();
        // Verify insertion order is preserved: zebra before alpha before middle
        let pos_z = json_str.find("zebra").unwrap();
        let pos_a = json_str.find("alpha").unwrap();
        let pos_m = json_str.find("middle").unwrap();
        assert!(pos_z < pos_a);
        assert!(pos_a < pos_m);
    }

    #[test]
    fn locale_metadata_keyword_count() {
        let meta = LocaleMetadata {
            keywords: vec!["photo".into(), "editor".into(), "filter".into()],
            name: Some("PhotoApp".into()),
            subtitle: None,
        };
        assert_eq!(meta.keyword_count(), 3);
    }
}
