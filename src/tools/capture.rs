use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::AppShotsError;
use crate::io::FileStore;

/// Timeout for external simulator/capture commands.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Serialize)]
pub struct CaptureResult {
    pub captured: usize,
    pub captures: Vec<CaptureInfo>,
}

#[derive(Debug, Serialize)]
pub struct CaptureInfo {
    pub mode: u8,
    pub locale: String,
    pub device: String,
    pub output_path: String,
}

/// Build the simctl launch command for a screenshot mode.
pub(crate) fn build_launch_command(bundle_id: &str, mode: u8) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args([
        "simctl",
        "launch",
        "booted",
        bundle_id,
        &format!("--screenshot-{mode}"),
    ]);
    cmd
}

/// Build the simctl screenshot command for the booted simulator.
pub(crate) fn build_capture_command(output_path: &str) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args(["simctl", "io", "booted", "screenshot", output_path]);
    cmd
}

/// Capture screenshots from simulator.
///
/// NOTE: Actual simulator interaction requires macOS with Xcode.
/// For testability, `build_launch_command` and `build_capture_command` are exposed separately.
pub(crate) async fn handle_capture_screenshots(
    store: &dyn FileStore,
    project_dir: &Path,
    bundle_id: &str,
    device: &str,
    modes: Option<Vec<u8>>,
    locales: Option<Vec<String>>,
    delay_ms: u64,
) -> Result<CaptureResult, AppShotsError> {
    let target_modes = modes.unwrap_or_else(|| (1..=5).collect());
    let target_locales = locales.unwrap_or_else(|| {
        crate::model::locale::ALL
            .iter()
            .map(|l| l.code().to_owned())
            .collect()
    });

    let captures_dir = project_dir.join("appshots/captures").join(device);
    let mut captures = Vec::new();

    for locale in &target_locales {
        let locale_dir = captures_dir.join(locale);
        store.create_parent_dirs(&locale_dir.join("_"))?;

        for &mode in &target_modes {
            // Launch app in screenshot mode
            let mut launch_cmd = build_launch_command(bundle_id, mode);
            let launch_output = timeout(COMMAND_TIMEOUT, launch_cmd.output())
                .await
                .map_err(|_| AppShotsError::SimctlTimeout {
                    command: "launch",
                    timeout_secs: COMMAND_TIMEOUT.as_secs(),
                })?
                .map_err(|e| AppShotsError::SimctlFailed {
                    command: "launch",
                    detail: e.to_string(),
                })?;

            if !launch_output.status.success() {
                let stderr = String::from_utf8_lossy(&launch_output.stderr);
                return Err(AppShotsError::SimctlFailed {
                    command: "launch",
                    detail: stderr.into_owned(),
                });
            }

            // Wait for the app to settle
            if delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }

            // Capture screenshot via simctl io
            let output_filename = format!("mode-{mode}.png");
            let output_path = locale_dir.join(&output_filename);
            let output_path_str = output_path.to_string_lossy().to_string();

            let mut capture_cmd = build_capture_command(&output_path_str);
            let capture_output = timeout(COMMAND_TIMEOUT, capture_cmd.output())
                .await
                .map_err(|_| AppShotsError::CaptureTimeout {
                    timeout_secs: COMMAND_TIMEOUT.as_secs(),
                })?
                .map_err(|e| AppShotsError::CaptureFailed {
                    device: device.to_owned(),
                    detail: e.to_string(),
                })?;

            if !capture_output.status.success() {
                let stderr = String::from_utf8_lossy(&capture_output.stderr);
                return Err(AppShotsError::CaptureFailed {
                    device: device.to_owned(),
                    detail: stderr.into_owned(),
                });
            }

            captures.push(CaptureInfo {
                mode,
                locale: locale.clone(),
                device: device.to_owned(),
                output_path: output_path_str,
            });
        }
    }

    Ok(CaptureResult {
        captured: captures.len(),
        captures,
    })
}

#[derive(Debug, Serialize)]
pub(crate) struct SimulatorInfo {
    pub name: String,
    pub udid: String,
    pub state: String,
    pub runtime: String,
}

/// List available iOS simulators via `xcrun simctl list devices -j`.
pub(crate) async fn handle_list_simulators() -> Result<Vec<SimulatorInfo>, AppShotsError> {
    let output = timeout(
        COMMAND_TIMEOUT,
        Command::new("xcrun")
            .args(["simctl", "list", "devices", "-j"])
            .output(),
    )
    .await
    .map_err(|_| AppShotsError::SimctlTimeout {
        command: "list devices",
        timeout_secs: COMMAND_TIMEOUT.as_secs(),
    })?
    .map_err(|e| AppShotsError::SimctlFailed {
        command: "list devices",
        detail: e.to_string(),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppShotsError::SimctlFailed {
            command: "list devices",
            detail: stderr.into_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_simctl_devices(&stdout)
}

/// Parse the JSON output of `xcrun simctl list devices -j`.
pub(crate) fn parse_simctl_devices(json: &str) -> Result<Vec<SimulatorInfo>, AppShotsError> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|e| AppShotsError::JsonParse(e.to_string()))?;

    let devices = root
        .get("devices")
        .and_then(|d| d.as_object())
        .ok_or_else(|| AppShotsError::JsonParse("missing 'devices' key in simctl output".into()))?;

    let mut result = Vec::new();
    for (runtime, device_list) in devices {
        let Some(arr) = device_list.as_array() else {
            continue;
        };
        for device in arr {
            let name = device
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let udid = device
                .get("udid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let state = device
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            // Extract runtime name from the key (e.g. "com.apple.CoreSimulator.SimRuntime.iOS-18-0" -> "iOS-18-0")
            let runtime_short = runtime
                .rsplit('.')
                .next()
                .unwrap_or(runtime)
                .replace('-', ".");

            result.push(SimulatorInfo {
                name,
                udid,
                state,
                runtime: runtime_short,
            });
        }
    }

    result.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.runtime.cmp(&b.runtime)));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn build_launch_command_has_correct_args() {
        let cmd = build_launch_command("com.example.app", 3);
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("xcrun"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("launch"),
                OsStr::new("booted"),
                OsStr::new("com.example.app"),
                OsStr::new("--screenshot-3"),
            ]
        );
    }

    #[test]
    fn build_capture_command_has_correct_args() {
        let cmd = build_capture_command("/output/screenshot.png");
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("xcrun"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("io"),
                OsStr::new("booted"),
                OsStr::new("screenshot"),
                OsStr::new("/output/screenshot.png"),
            ]
        );
    }

    #[test]
    fn build_launch_command_different_modes() {
        for mode in [1u8, 5, 10] {
            let cmd = build_launch_command("com.test", mode);
            let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
            let expected = format!("--screenshot-{mode}");
            assert_eq!(args[4], OsStr::new(&expected));
        }
    }

    #[test]
    fn capture_result_serialization() {
        let result = CaptureResult {
            captured: 1,
            captures: vec![CaptureInfo {
                mode: 1,
                locale: "en-US".to_owned(),
                device: "iPhone 6.9\"".to_owned(),
                output_path: "/tmp/mode-1.png".to_owned(),
            }],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["captured"], 1);
        assert_eq!(json["captures"][0]["mode"], 1);
        assert_eq!(json["captures"][0]["locale"], "en-US");
    }

    const SAMPLE_SIMCTL_JSON: &str = r#"{
        "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                {
                    "name": "iPhone 16 Pro Max",
                    "udid": "AAAA-BBBB-CCCC",
                    "state": "Shutdown",
                    "isAvailable": true
                },
                {
                    "name": "iPhone 16",
                    "udid": "DDDD-EEEE-FFFF",
                    "state": "Booted",
                    "isAvailable": true
                }
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
                {
                    "name": "iPad Pro 13-inch (M4)",
                    "udid": "1111-2222-3333",
                    "state": "Shutdown",
                    "isAvailable": true
                }
            ]
        }
    }"#;

    #[test]
    fn parse_simctl_devices_extracts_all() {
        let result = parse_simctl_devices(SAMPLE_SIMCTL_JSON).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn parse_simctl_devices_sorted_by_name() {
        let result = parse_simctl_devices(SAMPLE_SIMCTL_JSON).unwrap();
        let names: Vec<&str> = result.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["iPad Pro 13-inch (M4)", "iPhone 16", "iPhone 16 Pro Max"]
        );
    }

    #[test]
    fn parse_simctl_devices_runtime_extracted() {
        let result = parse_simctl_devices(SAMPLE_SIMCTL_JSON).unwrap();
        let ipad = result.iter().find(|s| s.name.contains("iPad")).unwrap();
        assert_eq!(ipad.runtime, "iOS.17.5");
    }

    #[test]
    fn parse_simctl_devices_fields() {
        let result = parse_simctl_devices(SAMPLE_SIMCTL_JSON).unwrap();
        let booted = result.iter().find(|s| s.state == "Booted").unwrap();
        assert_eq!(booted.name, "iPhone 16");
        assert_eq!(booted.udid, "DDDD-EEEE-FFFF");
    }

    #[test]
    fn parse_simctl_devices_empty() {
        let json = r#"{"devices": {}}"#;
        let result = parse_simctl_devices(json).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_simctl_devices_invalid_json() {
        let result = parse_simctl_devices("not json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_simctl_devices_missing_devices_key() {
        let result = parse_simctl_devices(r#"{"other": true}"#);
        assert!(result.is_err());
    }

    #[test]
    fn simulator_info_serialization() {
        let info = SimulatorInfo {
            name: "iPhone 16".into(),
            udid: "ABC-123".into(),
            state: "Booted".into(),
            runtime: "iOS.18.0".into(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["name"], "iPhone 16");
        assert_eq!(json["udid"], "ABC-123");
        assert_eq!(json["state"], "Booted");
        assert_eq!(json["runtime"], "iOS.18.0");
    }

    #[test]
    fn parse_simctl_devices_with_missing_fields() {
        // Devices with missing optional fields should still parse
        let json = r#"{
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                    {"name": "iPhone 17", "udid": "AAA", "state": "Shutdown"},
                    {"udid": "BBB", "state": "Booted"}
                ]
            }
        }"#;
        let result = parse_simctl_devices(json).unwrap();
        assert_eq!(result.len(), 2);
        // Missing name defaults to ""
        let no_name = result.iter().find(|s| s.udid == "BBB").unwrap();
        assert_eq!(no_name.name, "");
    }

    #[test]
    fn parse_simctl_devices_multiple_runtimes_sorted() {
        let json = r#"{
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                    {"name": "iPhone 17", "udid": "A", "state": "Booted"}
                ],
                "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
                    {"name": "iPhone 17", "udid": "B", "state": "Shutdown"}
                ]
            }
        }"#;
        let result = parse_simctl_devices(json).unwrap();
        assert_eq!(result.len(), 2);
        // Same name, sorted by runtime
        assert_eq!(result[0].runtime, "iOS.17.5");
        assert_eq!(result[1].runtime, "iOS.18.0");
    }

    #[test]
    fn parse_simctl_devices_non_array_runtime_skipped() {
        let json = r#"{
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-18-0": "not an array",
                "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
                    {"name": "iPad", "udid": "C", "state": "Shutdown"}
                ]
            }
        }"#;
        let result = parse_simctl_devices(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "iPad");
    }

    #[test]
    fn build_capture_command_with_spaces_in_path() {
        let cmd = build_capture_command("/Users/test user/screenshots/mode 1.png");
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args[4],
            OsStr::new("/Users/test user/screenshots/mode 1.png")
        );
    }

    #[test]
    fn build_launch_command_mode_boundaries() {
        // Mode 0 (unusual but valid)
        let cmd = build_launch_command("com.test", 0);
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(args[4], OsStr::new("--screenshot-0"));

        // Mode 255 (u8 max)
        let cmd = build_launch_command("com.test", 255);
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(args[4], OsStr::new("--screenshot-255"));
    }

    #[test]
    fn capture_info_serialization_with_special_chars() {
        let info = CaptureInfo {
            mode: 3,
            locale: "zh-Hans".to_owned(),
            device: "iPad 13\"".to_owned(),
            output_path: "/tmp/appshots/captures/iPad 13\"/zh-Hans/mode-3.png".to_owned(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["mode"], 3);
        assert_eq!(json["locale"], "zh-Hans");
        assert_eq!(json["device"], "iPad 13\"");
    }

    #[test]
    fn capture_result_empty() {
        let result = CaptureResult {
            captured: 0,
            captures: vec![],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["captured"], 0);
        assert!(json["captures"].as_array().unwrap().is_empty());
    }
}
