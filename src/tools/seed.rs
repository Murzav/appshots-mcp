use std::path::Path;
use std::time::Duration;

use indexmap::IndexMap;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::service::plist_builder;

/// Timeout for simctl spawn command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the simctl command to import a plist into an app's UserDefaults.
pub(crate) fn build_seed_command(bundle_id: &str, plist_path: &str) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args([
        "simctl", "spawn", "booted", "defaults", "import", bundle_id, plist_path,
    ]);
    cmd
}

/// Seed UserDefaults in the booted simulator via plist import.
///
/// 1. Builds XML plist from the provided key-value data
/// 2. Writes it to `appshots/.seed-defaults.plist`
/// 3. Runs `simctl spawn booted defaults import <bundle_id> <path>`
/// 4. Returns summary JSON
pub(crate) async fn handle_seed_defaults(
    store: &dyn FileStore,
    project_dir: &Path,
    bundle_id: &str,
    data: IndexMap<String, serde_json::Value>,
) -> Result<serde_json::Value, AppShotsError> {
    let seeded_keys = data.len();
    let plist_content = plist_builder::build_xml_plist(&data)?;

    let plist_path = project_dir.join("appshots/.seed-defaults.plist");
    store.create_parent_dirs(&plist_path)?;
    store.write(&plist_path, &plist_content)?;

    let plist_path_str = plist_path.to_string_lossy().to_string();
    let mut cmd = build_seed_command(bundle_id, &plist_path_str);

    let output = timeout(COMMAND_TIMEOUT, cmd.output())
        .await
        .map_err(|_| AppShotsError::SimctlTimeout {
            command: "spawn defaults import",
            timeout_secs: COMMAND_TIMEOUT.as_secs(),
        })?
        .map_err(|e| AppShotsError::SimctlFailed {
            command: "spawn defaults import",
            detail: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppShotsError::SimctlFailed {
            command: "spawn defaults import",
            detail: stderr.into_owned(),
        });
    }

    Ok(serde_json::json!({
        "seeded_keys": seeded_keys,
        "bundle_id": bundle_id,
        "plist_path": plist_path_str,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn build_seed_command_has_correct_args() {
        let cmd = build_seed_command("com.example.app", "/tmp/defaults.plist");
        let prog = cmd.as_std().get_program();
        assert_eq!(prog, OsStr::new("xcrun"));

        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("simctl"),
                OsStr::new("spawn"),
                OsStr::new("booted"),
                OsStr::new("defaults"),
                OsStr::new("import"),
                OsStr::new("com.example.app"),
                OsStr::new("/tmp/defaults.plist"),
            ]
        );
    }

    #[test]
    fn build_seed_command_different_bundle_id() {
        let cmd = build_seed_command("org.test.myapp", "/path/to/file.plist");
        let args: Vec<&OsStr> = cmd.as_std().get_args().collect();
        assert_eq!(args[5], OsStr::new("org.test.myapp"));
        assert_eq!(args[6], OsStr::new("/path/to/file.plist"));
    }

    #[tokio::test]
    async fn handle_seed_defaults_writes_plist_to_correct_path() {
        use crate::io::memory::MemoryStore;
        use std::path::PathBuf;

        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        let mut data = IndexMap::new();
        data.insert("streak".to_owned(), serde_json::json!(7));
        data.insert("name".to_owned(), serde_json::json!("test"));
        data.insert("isPro".to_owned(), serde_json::json!(true));

        // Will fail at simctl command (no simulator), but plist should be written first
        let result = handle_seed_defaults(&store, &project_dir, "com.test.app", data).await;

        // Command fails (no simulator), but file should have been written
        assert!(result.is_err());

        // Verify the plist file was written
        let plist_path = PathBuf::from("/project/appshots/.seed-defaults.plist");
        let content = store
            .read(&plist_path)
            .expect("plist should be written before command runs");
        assert!(content.contains("<?xml version=\"1.0\""));
        assert!(content.contains("<key>streak</key>"));
        assert!(content.contains("<integer>7</integer>"));
        assert!(content.contains("<key>name</key>"));
        assert!(content.contains("<string>test</string>"));
        assert!(content.contains("<key>isPro</key>"));
        assert!(content.contains("<true/>"));
    }

    #[tokio::test]
    async fn handle_seed_defaults_error_is_simctl_failed() {
        use crate::io::memory::MemoryStore;
        use std::path::PathBuf;

        let store = MemoryStore::new();
        let mut data = IndexMap::new();
        data.insert("key".to_owned(), serde_json::json!("val"));

        let err = handle_seed_defaults(&store, &PathBuf::from("/p"), "com.app", data)
            .await
            .unwrap_err();
        // Should be SimctlFailed or SimctlTimeout (command execution failure)
        let msg = err.to_string();
        assert!(
            msg.contains("simctl") || msg.contains("timed out"),
            "expected simctl error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn handle_seed_defaults_empty_data_writes_empty_plist() {
        use crate::io::memory::MemoryStore;
        use std::path::PathBuf;

        let store = MemoryStore::new();
        let data = IndexMap::new();

        let _ = handle_seed_defaults(&store, &PathBuf::from("/p"), "com.app", data).await;

        let plist_path = PathBuf::from("/p/appshots/.seed-defaults.plist");
        let content = store.read(&plist_path).expect("plist should be written");
        assert!(content.contains("<dict>"));
        assert!(content.contains("</dict>"));
        // No keys inside dict
        assert!(!content.contains("<key>"));
    }

    #[tokio::test]
    async fn handle_seed_defaults_complex_nested_data() {
        use crate::io::memory::MemoryStore;
        use std::path::PathBuf;

        let store = MemoryStore::new();
        let mut data = IndexMap::new();
        data.insert(
            "records".to_owned(),
            serde_json::json!([{"id": "abc", "count": 42}]),
        );
        data.insert("score".to_owned(), serde_json::json!(3.14));
        data.insert("skip_me".to_owned(), serde_json::json!(null));

        let _ = handle_seed_defaults(&store, &PathBuf::from("/p"), "com.app", data).await;

        let content = store
            .read(&PathBuf::from("/p/appshots/.seed-defaults.plist"))
            .unwrap();
        assert!(content.contains("<array>"));
        assert!(content.contains("<key>id</key>"));
        assert!(content.contains("<string>abc</string>"));
        assert!(content.contains("<integer>42</integer>"));
        assert!(content.contains("<real>3.14</real>"));
        // null should be skipped
        assert!(!content.contains("skip_me"));
    }
}
