use crate::error::AppShotsError;
use crate::model::config::ProjectConfig;

/// Parse appshots.json content into ProjectConfig.
pub fn parse_config(content: &str) -> Result<ProjectConfig, AppShotsError> {
    serde_json::from_str(content).map_err(|e| AppShotsError::JsonParse(e.to_string()))
}

/// Serialize ProjectConfig to pretty JSON.
pub fn serialize_config(config: &ProjectConfig) -> Result<String, AppShotsError> {
    serde_json::to_string_pretty(config).map_err(|e| AppShotsError::JsonParse(e.to_string()))
}

/// Validate a ProjectConfig: check required fields and OKLCH colors.
pub fn validate_config(config: &ProjectConfig) -> Result<(), AppShotsError> {
    if config.bundle_id.is_empty() {
        return Err(AppShotsError::InvalidFormat("bundle_id is required".into()));
    }

    // Validate OKLCH colors in per_screen_overrides
    if let Some(overrides) = &config.per_screen_overrides {
        for (mode, ovr) in overrides {
            if let Some(colors) = &ovr.bg_colors {
                for (i, color) in colors.iter().enumerate() {
                    color.validate().map_err(|e| {
                        AppShotsError::InvalidColor(format!("screen {mode}, bg_color[{i}]: {e}"))
                    })?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::color::OklchColor;
    use crate::model::config::PerScreenOverride;
    use crate::model::device::Device;
    use crate::model::template::TemplateMode;
    use indexmap::IndexMap;

    fn minimal_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#
    }

    fn minimal_config() -> ProjectConfig {
        ProjectConfig {
            bundle_id: "com.example.app".into(),
            screens: vec![],
            template_mode: TemplateMode::Single,
            per_screen_overrides: None,
            devices: vec![Device::Iphone6_9],
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn parse_config_valid() {
        let config = parse_config(minimal_json()).unwrap();
        assert_eq!(config.bundle_id, "com.example.app");
        assert!(config.screens.is_empty());
    }

    #[test]
    fn parse_config_invalid_json() {
        let result = parse_config("not json");
        assert!(result.is_err());
    }

    #[test]
    fn serialize_config_roundtrip() {
        let config = minimal_config();
        let json = serialize_config(&config).unwrap();
        let back = parse_config(&json).unwrap();
        assert_eq!(back.bundle_id, config.bundle_id);
        assert_eq!(back.devices.len(), config.devices.len());
    }

    #[test]
    fn validate_config_valid() {
        let config = minimal_config();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn validate_config_empty_bundle_id() {
        let config = ProjectConfig {
            bundle_id: String::new(),
            ..minimal_config()
        };
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, AppShotsError::InvalidFormat(_)));
    }

    #[test]
    fn validate_config_invalid_oklch_color() {
        let mut overrides = IndexMap::new();
        overrides.insert(
            1,
            PerScreenOverride {
                bg_colors: Some(vec![OklchColor {
                    l: 200.0, // out of range
                    c: 0.1,
                    h: 0.0,
                    alpha: 1.0,
                }]),
                font_override: None,
            },
        );
        let config = ProjectConfig {
            per_screen_overrides: Some(overrides),
            ..minimal_config()
        };
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, AppShotsError::InvalidColor(_)));
    }

    #[test]
    fn validate_config_valid_oklch_colors() {
        let mut overrides = IndexMap::new();
        overrides.insert(
            1,
            PerScreenOverride {
                bg_colors: Some(vec![OklchColor {
                    l: 70.0,
                    c: 0.15,
                    h: 250.0,
                    alpha: 1.0,
                }]),
                font_override: None,
            },
        );
        let config = ProjectConfig {
            per_screen_overrides: Some(overrides),
            ..minimal_config()
        };
        assert!(validate_config(&config).is_ok());
    }
}
