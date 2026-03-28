use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::error::AppShotsError;

/// Timeout for each simctl sub-command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the command to boot a simulator by UDID.
pub(crate) fn build_boot_command(udid: &str) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args(["simctl", "boot", udid]);
    cmd
}

/// Build the command to grant all permissions to an app.
pub(crate) fn build_grant_command(bundle_id: &str) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args(["simctl", "privacy", "booted", "grant", "all", bundle_id]);
    cmd
}

/// Build the command to set the status bar to Apple canonical values.
pub(crate) fn build_status_bar_command() -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args([
        "simctl",
        "status_bar",
        "booted",
        "override",
        "--time",
        "9:41",
        "--batteryState",
        "charged",
        "--batteryLevel",
        "100",
        "--wifiBars",
        "3",
        "--cellularBars",
        "4",
        "--cellularMode",
        "active",
        "--dataNetwork",
        "wifi",
    ]);
    cmd
}

/// Build the command to set simulator appearance (light/dark).
pub(crate) fn build_appearance_command(appearance: &str) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args(["simctl", "ui", "booted", "appearance", appearance]);
    cmd
}

/// Pre-warm a simulator: boot, grant permissions, set status bar, set appearance.
///
/// Steps:
/// 1. Boot simulator (ignore "already booted" errors)
/// 2. Grant all permissions if `bundle_id` provided
/// 3. Set status bar to Apple canonical (9:41, full battery/signal)
/// 4. Set appearance if provided
pub(crate) async fn handle_warm_simulator(
    udid: &str,
    bundle_id: Option<&str>,
    appearance: Option<&str>,
) -> Result<serde_json::Value, AppShotsError> {
    let mut steps_completed: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // 1. Boot
    let mut boot_cmd = build_boot_command(udid);
    let boot_output = timeout(COMMAND_TIMEOUT, boot_cmd.output())
        .await
        .map_err(|_| AppShotsError::SimctlTimeout {
            command: "boot",
            timeout_secs: COMMAND_TIMEOUT.as_secs(),
        })?
        .map_err(|e| AppShotsError::SimctlFailed {
            command: "boot",
            detail: e.to_string(),
        })?;

    if boot_output.status.success() {
        steps_completed.push("boot".into());
    } else {
        let stderr = String::from_utf8_lossy(&boot_output.stderr);
        // "Unable to boot device in current state: Booted" — not an error
        if stderr.contains("Booted") || stderr.contains("already booted") {
            steps_completed.push("boot (already booted)".into());
        } else {
            return Err(AppShotsError::SimctlFailed {
                command: "boot",
                detail: stderr.into_owned(),
            });
        }
    }

    // 2. Grant permissions
    if let Some(bid) = bundle_id {
        let mut grant_cmd = build_grant_command(bid);
        let grant_output = timeout(COMMAND_TIMEOUT, grant_cmd.output())
            .await
            .map_err(|_| AppShotsError::SimctlTimeout {
                command: "privacy grant",
                timeout_secs: COMMAND_TIMEOUT.as_secs(),
            })?
            .map_err(|e| AppShotsError::SimctlFailed {
                command: "privacy grant",
                detail: e.to_string(),
            })?;

        if grant_output.status.success() {
            steps_completed.push(format!("grant all permissions ({bid})"));
        } else {
            let stderr = String::from_utf8_lossy(&grant_output.stderr);
            warnings.push(format!("grant permissions warning: {stderr}"));
        }
    }

    // 3. Status bar
    let mut bar_cmd = build_status_bar_command();
    let bar_output = timeout(COMMAND_TIMEOUT, bar_cmd.output())
        .await
        .map_err(|_| AppShotsError::SimctlTimeout {
            command: "status_bar override",
            timeout_secs: COMMAND_TIMEOUT.as_secs(),
        })?
        .map_err(|e| AppShotsError::SimctlFailed {
            command: "status_bar override",
            detail: e.to_string(),
        })?;

    if bar_output.status.success() {
        steps_completed.push("status bar (9:41, full battery/signal)".into());
    } else {
        let stderr = String::from_utf8_lossy(&bar_output.stderr);
        warnings.push(format!("status bar warning: {stderr}"));
    }

    // 4. Appearance
    if let Some(mode) = appearance {
        let mut app_cmd = build_appearance_command(mode);
        let app_output = timeout(COMMAND_TIMEOUT, app_cmd.output())
            .await
            .map_err(|_| AppShotsError::SimctlTimeout {
                command: "ui appearance",
                timeout_secs: COMMAND_TIMEOUT.as_secs(),
            })?
            .map_err(|e| AppShotsError::SimctlFailed {
                command: "ui appearance",
                detail: e.to_string(),
            })?;

        if app_output.status.success() {
            steps_completed.push(format!("appearance ({mode})"));
        } else {
            let stderr = String::from_utf8_lossy(&app_output.stderr);
            warnings.push(format!("appearance warning: {stderr}"));
        }
    }

    Ok(serde_json::json!({
        "steps_completed": steps_completed,
        "warnings": warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn build_boot_command_has_correct_args() {
        let cmd = build_boot_command("AAAA-BBBB-CCCC");
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("xcrun"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("boot"),
                OsStr::new("AAAA-BBBB-CCCC"),
            ]
        );
    }

    #[test]
    fn build_grant_command_has_correct_args() {
        let cmd = build_grant_command("com.example.app");
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("xcrun"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("privacy"),
                OsStr::new("booted"),
                OsStr::new("grant"),
                OsStr::new("all"),
                OsStr::new("com.example.app"),
            ]
        );
    }

    #[test]
    fn build_status_bar_command_has_correct_args() {
        let cmd = build_status_bar_command();
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("xcrun"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("status_bar"),
                OsStr::new("booted"),
                OsStr::new("override"),
                OsStr::new("--time"),
                OsStr::new("9:41"),
                OsStr::new("--batteryState"),
                OsStr::new("charged"),
                OsStr::new("--batteryLevel"),
                OsStr::new("100"),
                OsStr::new("--wifiBars"),
                OsStr::new("3"),
                OsStr::new("--cellularBars"),
                OsStr::new("4"),
                OsStr::new("--cellularMode"),
                OsStr::new("active"),
                OsStr::new("--dataNetwork"),
                OsStr::new("wifi"),
            ]
        );
    }

    #[test]
    fn build_appearance_command_light() {
        let cmd = build_appearance_command("light");
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("ui"),
                OsStr::new("booted"),
                OsStr::new("appearance"),
                OsStr::new("light"),
            ]
        );
    }

    #[test]
    fn build_appearance_command_dark() {
        let cmd = build_appearance_command("dark");
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(args[4], OsStr::new("dark"));
    }

    #[test]
    fn build_boot_command_preserves_udid_format() {
        // UDIDs can have various formats — must be passed through unchanged
        let udid = "4A5B6C7D-8E9F-0A1B-2C3D-4E5F6A7B8C9D";
        let cmd = build_boot_command(udid);
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(args[2], OsStr::new(udid));
    }

    #[test]
    fn build_grant_command_with_reverse_dns_bundle() {
        let cmd = build_grant_command("com.company.product.module");
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(args[5], OsStr::new("com.company.product.module"));
    }

    #[test]
    fn build_status_bar_command_has_canonical_time() {
        let cmd = build_status_bar_command();
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        // Apple canonical time is 9:41
        let time_idx = args
            .iter()
            .position(|a| *a == OsStr::new("--time"))
            .unwrap();
        assert_eq!(args[time_idx + 1], OsStr::new("9:41"));
    }

    #[test]
    fn build_status_bar_command_has_full_battery() {
        let cmd = build_status_bar_command();
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        let level_idx = args
            .iter()
            .position(|a| *a == OsStr::new("--batteryLevel"))
            .unwrap();
        assert_eq!(args[level_idx + 1], OsStr::new("100"));
    }

    #[tokio::test]
    async fn handle_warm_simulator_fails_with_simctl_error() {
        // Without a real simulator, boot will fail
        let result = handle_warm_simulator("FAKE-UDID-1234", None, None).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("simctl") || msg.contains("timed out"),
            "expected simctl error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn handle_warm_simulator_with_all_params_fails_at_boot() {
        let result = handle_warm_simulator("FAKE-UDID", Some("com.test.app"), Some("dark")).await;
        // Should fail at boot step (first command)
        assert!(result.is_err());
    }
}
