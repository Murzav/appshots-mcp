use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::AppShotsError;

fn default_alpha() -> f64 {
    1.0
}

fn is_default_alpha(v: &f64) -> bool {
    (*v - 1.0).abs() < f64::EPSILON
}

/// OKLCH color. All colors in the project use this space exclusively.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OklchColor {
    /// Lightness, 0–100 (percent).
    pub l: f64,
    /// Chroma, 0–0.4.
    pub c: f64,
    /// Hue, 0–360 (degrees).
    pub h: f64,
    /// Alpha, 0–1. Defaults to 1.0.
    #[serde(default = "default_alpha", skip_serializing_if = "is_default_alpha")]
    pub alpha: f64,
}

impl Default for OklchColor {
    fn default() -> Self {
        Self {
            l: 0.0,
            c: 0.0,
            h: 0.0,
            alpha: 1.0,
        }
    }
}

impl OklchColor {
    /// Format as a Typst oklch() call.
    /// Omits alpha when it equals 1.0.
    pub fn to_typst(&self) -> String {
        if (self.alpha - 1.0).abs() < f64::EPSILON {
            format!("oklch({:.0}%, {}, {:.0}deg)", self.l, self.c, self.h)
        } else {
            format!(
                "oklch({:.0}%, {}, {:.0}deg, {:.0}%)",
                self.l,
                self.c,
                self.h,
                self.alpha * 100.0
            )
        }
    }

    /// Validate that all components are within their legal ranges.
    pub fn validate(&self) -> Result<(), AppShotsError> {
        if !(0.0..=100.0).contains(&self.l) {
            return Err(AppShotsError::InvalidColor(format!(
                "lightness {} out of range [0, 100]",
                self.l
            )));
        }
        if !(0.0..=0.4).contains(&self.c) {
            return Err(AppShotsError::InvalidColor(format!(
                "chroma {} out of range [0, 0.4]",
                self.c
            )));
        }
        if !(0.0..=360.0).contains(&self.h) {
            return Err(AppShotsError::InvalidColor(format!(
                "hue {} out of range [0, 360]",
                self.h
            )));
        }
        if !(0.0..=1.0).contains(&self.alpha) {
            return Err(AppShotsError::InvalidColor(format!(
                "alpha {} out of range [0, 1]",
                self.alpha
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_typst_no_alpha() {
        let c = OklchColor {
            l: 50.0,
            c: 0.15,
            h: 240.0,
            alpha: 1.0,
        };
        assert_eq!(c.to_typst(), "oklch(50%, 0.15, 240deg)");
    }

    #[test]
    fn to_typst_with_alpha() {
        let c = OklchColor {
            l: 50.0,
            c: 0.15,
            h: 240.0,
            alpha: 0.8,
        };
        assert_eq!(c.to_typst(), "oklch(50%, 0.15, 240deg, 80%)");
    }

    #[test]
    fn validate_ok() {
        let c = OklchColor {
            l: 50.0,
            c: 0.2,
            h: 180.0,
            alpha: 1.0,
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_lightness_out_of_range() {
        let c = OklchColor {
            l: 101.0,
            c: 0.1,
            h: 0.0,
            alpha: 1.0,
        };
        let err = c.validate().unwrap_err();
        assert!(matches!(err, AppShotsError::InvalidColor(_)));
    }

    #[test]
    fn validate_chroma_out_of_range() {
        let c = OklchColor {
            l: 50.0,
            c: 0.5,
            h: 0.0,
            alpha: 1.0,
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_hue_out_of_range() {
        let c = OklchColor {
            l: 50.0,
            c: 0.1,
            h: 361.0,
            alpha: 1.0,
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_alpha_out_of_range() {
        let c = OklchColor {
            l: 50.0,
            c: 0.1,
            h: 0.0,
            alpha: 1.5,
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_boundary_values() {
        let c = OklchColor {
            l: 0.0,
            c: 0.0,
            h: 0.0,
            alpha: 0.0,
        };
        assert!(c.validate().is_ok());

        let c = OklchColor {
            l: 100.0,
            c: 0.4,
            h: 360.0,
            alpha: 1.0,
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn default_is_zero_with_full_alpha() {
        let c = OklchColor::default();
        assert_eq!(c.l, 0.0);
        assert_eq!(c.c, 0.0);
        assert_eq!(c.h, 0.0);
        assert_eq!(c.alpha, 1.0);
    }

    #[test]
    fn serde_roundtrip() {
        let c = OklchColor {
            l: 65.0,
            c: 0.25,
            h: 120.0,
            alpha: 0.9,
        };
        let json = serde_json::to_value(&c).unwrap();
        insta::assert_json_snapshot!(json, @r#"
        {
          "l": 65.0,
          "c": 0.25,
          "h": 120.0,
          "alpha": 0.9
        }
        "#);
        let deserialized: OklchColor = serde_json::from_value(json).unwrap();
        assert_eq!(c, deserialized);
    }

    #[test]
    fn serde_default_alpha() {
        let json = r#"{"l": 50, "c": 0.1, "h": 200}"#;
        let c: OklchColor = serde_json::from_str(json).unwrap();
        assert_eq!(c.alpha, 1.0);
    }
}
