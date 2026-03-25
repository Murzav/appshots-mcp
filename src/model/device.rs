use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Supported Apple device targets for App Store screenshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Device {
    #[serde(rename = "iPhone 6.9\"")]
    Iphone6_9,
    #[serde(rename = "iPad 13\"")]
    Ipad13,
}

/// All devices required by App Store (2026).
pub const REQUIRED: &[Device] = &[Device::Iphone6_9, Device::Ipad13];

impl Device {
    /// Canvas pixel dimensions (width, height).
    pub fn canvas_size(self) -> (u32, u32) {
        match self {
            Self::Iphone6_9 => (1320, 2868),
            Self::Ipad13 => (2064, 2752),
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Iphone6_9 => "iPhone 6.9\"",
            Self::Ipad13 => "iPad 13\"",
        }
    }

    /// Simulator device name for `xcrun simctl`.
    pub fn simulator_name(self) -> &'static str {
        match self {
            Self::Iphone6_9 => "iPhone 17 Pro Max",
            Self::Ipad13 => "iPad Pro 13-inch (M4)",
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_sizes() {
        assert_eq!(Device::Iphone6_9.canvas_size(), (1320, 2868));
        assert_eq!(Device::Ipad13.canvas_size(), (2064, 2752));
    }

    #[test]
    fn display_names() {
        assert_eq!(Device::Iphone6_9.to_string(), "iPhone 6.9\"");
        assert_eq!(Device::Ipad13.to_string(), "iPad 13\"");
    }

    #[test]
    fn simulator_names() {
        assert_eq!(Device::Iphone6_9.simulator_name(), "iPhone 17 Pro Max");
        assert_eq!(Device::Ipad13.simulator_name(), "iPad Pro 13-inch (M4)");
    }

    #[test]
    fn required_devices() {
        assert_eq!(REQUIRED.len(), 2);
        assert!(REQUIRED.contains(&Device::Iphone6_9));
        assert!(REQUIRED.contains(&Device::Ipad13));
    }

    #[test]
    fn serde_roundtrip_iphone() {
        let json = serde_json::to_string(&Device::Iphone6_9).unwrap();
        assert_eq!(json, r#""iPhone 6.9\"""#);
        let d: Device = serde_json::from_str(&json).unwrap();
        assert_eq!(d, Device::Iphone6_9);
    }

    #[test]
    fn serde_roundtrip_ipad() {
        let json = serde_json::to_string(&Device::Ipad13).unwrap();
        assert_eq!(json, r#""iPad 13\"""#);
        let d: Device = serde_json::from_str(&json).unwrap();
        assert_eq!(d, Device::Ipad13);
    }
}
