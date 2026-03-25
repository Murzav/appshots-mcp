use std::path::Path;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::error::AppShotsError;

/// Timeout for fastlane deliver (may upload many screenshots).
const DELIVER_TIMEOUT: Duration = Duration::from_secs(600);

/// Run `fastlane deliver` to upload screenshots.
pub(crate) async fn handle_run_deliver(
    project_dir: &Path,
) -> Result<serde_json::Value, AppShotsError> {
    let output = timeout(DELIVER_TIMEOUT, build_deliver_command(project_dir).output())
        .await
        .map_err(|_| AppShotsError::DeliverError("fastlane deliver timed out after 600s".into()))?
        .map_err(|e| AppShotsError::DeliverError(format!("failed to run fastlane deliver: {e}")))?;

    if !output.status.success() {
        return Err(AppShotsError::DeliverError(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    Ok(serde_json::json!({
        "success": true,
        "output": String::from_utf8_lossy(&output.stdout),
    }))
}

/// Build the fastlane deliver command (exposed for testing).
pub(crate) fn build_deliver_command(project_dir: &Path) -> Command {
    let mut cmd = Command::new("fastlane");
    cmd.arg("deliver")
        .arg("--skip_metadata")
        .arg("--skip_app_rating")
        .current_dir(project_dir);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    #[test]
    fn build_deliver_command_has_correct_args() {
        let cmd = build_deliver_command(Path::new("/project"));
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("fastlane"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("deliver"),
                OsStr::new("--skip_metadata"),
                OsStr::new("--skip_app_rating"),
            ]
        );
    }

    #[test]
    fn build_deliver_command_sets_cwd() {
        let dir = PathBuf::from("/my/project");
        let cmd = build_deliver_command(&dir);
        let cwd = cmd.as_std().get_current_dir().unwrap();
        assert_eq!(cwd, dir.as_path());
    }
}
