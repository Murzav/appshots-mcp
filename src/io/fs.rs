use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::AppShotsError;
use crate::io::FileStore;

const DEFAULT_MAX_FILE_SIZE_MB: u64 = 200;
const TEMP_PREFIX: &str = ".appshots-mcp-";
const TEMP_SUFFIX: &str = ".tmp";

/// Filesystem-backed `FileStore` for production use.
pub struct FsFileStore {
    max_file_size: u64,
    project_dir: Option<PathBuf>,
}

impl FsFileStore {
    pub fn new() -> Self {
        let max_mb = std::env::var("APPSHOTS_MAX_FILE_SIZE_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_FILE_SIZE_MB);

        let store = Self {
            max_file_size: max_mb * 1024 * 1024,
            project_dir: None,
        };
        store.cleanup_orphan_temps();
        store
    }

    /// Set the project directory for path containment checks.
    /// All file operations will be restricted to paths within this directory.
    pub fn with_project_dir(mut self, dir: PathBuf) -> Self {
        self.project_dir = dir.canonicalize().ok().or(Some(dir));
        self
    }

    /// Verify that a canonical path is within the project directory.
    fn check_containment(&self, canonical: &Path) -> Result<(), AppShotsError> {
        if let Some(ref root) = self.project_dir
            && !canonical.starts_with(root)
        {
            return Err(AppShotsError::InvalidPath {
                path: canonical.to_path_buf(),
                reason: "path escapes project directory".into(),
            });
        }
        Ok(())
    }

    /// Remove leftover temp files from a previous crash.
    fn cleanup_orphan_temps(&self) {
        let current_dir = match std::env::current_dir() {
            Ok(d) => d,
            Err(_) => return,
        };
        let entries = match fs::read_dir(&current_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Validate that a path contains no `..` components.
    fn validate_path(path: &Path) -> Result<(), AppShotsError> {
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                return Err(AppShotsError::InvalidPath {
                    path: path.to_path_buf(),
                    reason: "path contains `..` component".into(),
                });
            }
        }
        Ok(())
    }

    /// Canonicalize a path. For non-existing files, canonicalize the parent and
    /// append the file name.
    fn canonicalize(path: &Path) -> Result<PathBuf, AppShotsError> {
        if path.exists() {
            path.canonicalize().map_err(AppShotsError::Io)
        } else {
            let parent = path.parent().ok_or_else(|| AppShotsError::InvalidPath {
                path: path.to_path_buf(),
                reason: "no parent directory".into(),
            })?;
            let file_name = path.file_name().ok_or_else(|| AppShotsError::InvalidPath {
                path: path.to_path_buf(),
                reason: "no file name".into(),
            })?;
            let canonical_parent =
                parent
                    .canonicalize()
                    .map_err(|_| AppShotsError::FileNotFound {
                        path: parent.to_path_buf(),
                    })?;
            Ok(canonical_parent.join(file_name))
        }
    }

    /// Check file size against the configured limit.
    fn check_size(&self, metadata: &fs::Metadata, path: &Path) -> Result<(), AppShotsError> {
        let size = metadata.len();
        if size > self.max_file_size {
            return Err(AppShotsError::FileTooLarge {
                size_mb: size / (1024 * 1024),
                max_mb: self.max_file_size / (1024 * 1024),
            });
        }
        let _ = path; // used only for context in future error variants
        Ok(())
    }

    /// Strip UTF-8 BOM if present.
    fn strip_bom(s: String) -> String {
        s.strip_prefix('\u{feff}')
            .map(|stripped| stripped.to_owned())
            .unwrap_or(s)
    }

    /// Generate a temp file path in the same directory as `path`.
    fn temp_path(path: &Path) -> PathBuf {
        let pid = std::process::id();
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let parent = path.parent().unwrap_or(path);
        parent.join(format!("{TEMP_PREFIX}{pid}-{ts}{TEMP_SUFFIX}"))
    }

    /// Attempt advisory flock on an existing file. Returns error only on
    /// `EWOULDBLOCK`; other errors are logged and ignored.
    fn try_lock(path: &Path) -> Result<(), AppShotsError> {
        use std::os::unix::io::AsRawFd;

        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Ok(()), // file doesn't exist yet — no lock needed
        };
        let fd = file.as_raw_fd();
        // SAFETY: `fd` is a valid file descriptor obtained from `File::open`.
        // `libc::flock` with `LOCK_EX | LOCK_NB` is safe on a valid fd.
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Err(AppShotsError::FileLocked {
                    path: path.to_path_buf(),
                });
            }
            tracing::warn!("flock on {}: {err}", path.display());
        }
        Ok(())
    }

    /// Atomic write: temp file → write → sync → rename. Clean up temp on
    /// failure.
    fn atomic_write(path: &Path, content: &[u8]) -> Result<(), AppShotsError> {
        let tmp = Self::temp_path(path);
        let result = (|| -> Result<(), AppShotsError> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(content)?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

impl Default for FsFileStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FileStore for FsFileStore {
    fn read(&self, path: &Path) -> Result<String, AppShotsError> {
        Self::validate_path(path)?;
        let canonical = Self::canonicalize(path)?;
        self.check_containment(&canonical)?;
        let metadata = fs::metadata(&canonical).map_err(|_| AppShotsError::FileNotFound {
            path: path.to_path_buf(),
        })?;
        self.check_size(&metadata, &canonical)?;
        let content = fs::read_to_string(&canonical).map_err(|_| AppShotsError::FileNotFound {
            path: path.to_path_buf(),
        })?;
        Ok(Self::strip_bom(content))
    }

    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, AppShotsError> {
        Self::validate_path(path)?;
        let canonical = Self::canonicalize(path)?;
        self.check_containment(&canonical)?;
        let metadata = fs::metadata(&canonical).map_err(|_| AppShotsError::FileNotFound {
            path: path.to_path_buf(),
        })?;
        self.check_size(&metadata, &canonical)?;
        fs::read(&canonical).map_err(|_| AppShotsError::FileNotFound {
            path: path.to_path_buf(),
        })
    }

    fn write(&self, path: &Path, content: &str) -> Result<(), AppShotsError> {
        Self::validate_path(path)?;
        let canonical = Self::canonicalize(path)?;
        self.check_containment(&canonical)?;
        Self::try_lock(&canonical)?;
        Self::atomic_write(&canonical, content.as_bytes())
    }

    fn write_bytes(&self, path: &Path, content: &[u8]) -> Result<(), AppShotsError> {
        Self::validate_path(path)?;
        let canonical = Self::canonicalize(path)?;
        self.check_containment(&canonical)?;
        Self::try_lock(&canonical)?;
        Self::atomic_write(&canonical, content)
    }

    fn modified_time(&self, path: &Path) -> Result<SystemTime, AppShotsError> {
        Self::validate_path(path)?;
        let canonical = Self::canonicalize(path)?;
        self.check_containment(&canonical)?;
        let metadata = fs::metadata(&canonical).map_err(|_| AppShotsError::FileNotFound {
            path: path.to_path_buf(),
        })?;
        metadata.modified().map_err(AppShotsError::Io)
    }

    fn exists(&self, path: &Path) -> bool {
        Self::validate_path(path).is_ok()
            && Self::canonicalize(path)
                .map(|c| self.check_containment(&c).is_ok())
                .unwrap_or(false)
            && path.exists()
    }

    fn create_parent_dirs(&self, path: &Path) -> Result<(), AppShotsError> {
        Self::validate_path(path)?;
        if let Some(parent) = path.parent() {
            // Check containment if project_dir is set — use the path as-is
            // since parent dirs may not exist yet (can't canonicalize)
            if let Some(ref root) = self.project_dir {
                // For non-existing paths, check if the absolute form starts with root
                let abs = if parent.is_absolute() {
                    parent.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map_err(AppShotsError::Io)?
                        .join(parent)
                };
                // Normalize by checking the longest existing ancestor
                let mut check = abs.as_path();
                loop {
                    if check.exists() {
                        let canonical = check.canonicalize().map_err(AppShotsError::Io)?;
                        if !canonical.starts_with(root) {
                            return Err(AppShotsError::InvalidPath {
                                path: path.to_path_buf(),
                                reason: "path escapes project directory".into(),
                            });
                        }
                        break;
                    }
                    match check.parent() {
                        Some(p) => check = p,
                        None => break,
                    }
                }
            }
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, AppShotsError> {
        Self::validate_path(path)?;
        let canonical = Self::canonicalize(path)?;
        self.check_containment(&canonical)?;
        let entries = fs::read_dir(&canonical).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppShotsError::FileNotFound {
                    path: path.to_path_buf(),
                }
            } else {
                AppShotsError::Io(e)
            }
        })?;
        let mut result: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry?;
            result.push(entry.path());
        }
        result.sort();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> FsFileStore {
        FsFileStore {
            max_file_size: 1024 * 1024, // 1 MB for tests
            project_dir: None,
        }
    }

    #[test]
    fn read_write_text_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("hello.txt");
        let s = store();
        s.write(&path, "hello world").expect("write");
        let content = s.read(&path).expect("read");
        assert_eq!(content, "hello world");
    }

    #[test]
    fn read_write_bytes_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("data.bin");
        let s = store();
        let data = vec![0u8, 1, 2, 255, 128];
        s.write_bytes(&path, &data).expect("write_bytes");
        let result = s.read_bytes(&path).expect("read_bytes");
        assert_eq!(result, data);
    }

    #[test]
    fn bom_stripping() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("bom.txt");
        // Write raw bytes with BOM
        std::fs::write(&path, b"\xef\xbb\xbfhello").expect("raw write");
        let s = store();
        let content = s.read(&path).expect("read");
        assert_eq!(content, "hello");
    }

    #[test]
    fn file_too_large() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("big.bin");
        let s = FsFileStore {
            max_file_size: 10, // 10 bytes
            project_dir: None,
        };
        std::fs::write(&path, "this is way too large").expect("raw write");
        let err = s.read(&path).expect_err("should fail");
        assert!(matches!(err, AppShotsError::FileTooLarge { .. }));
    }

    #[test]
    fn path_traversal_rejected() {
        let s = store();
        let path = Path::new("/tmp/../etc/passwd");
        let err = s.read(path).expect_err("should fail");
        assert!(matches!(err, AppShotsError::InvalidPath { .. }));
    }

    #[test]
    fn file_not_found() {
        let s = store();
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nonexistent.txt");
        let err = s.read(&path).expect_err("should fail");
        assert!(matches!(err, AppShotsError::FileNotFound { .. }));
    }

    #[test]
    fn atomic_write_no_orphans() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("atomic.txt");
        let s = store();
        s.write(&path, "content").expect("write");
        // No temp files should remain
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX)
            })
            .collect();
        assert!(entries.is_empty(), "orphan temp files found: {entries:?}");
    }

    #[test]
    fn modified_time_returns_recent() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("ts.txt");
        let s = store();
        s.write(&path, "time check").expect("write");
        let mtime = s.modified_time(&path).expect("modified_time");
        let elapsed = SystemTime::now().duration_since(mtime).unwrap_or_default();
        assert!(
            elapsed.as_secs() < 5,
            "mtime should be within last 5 seconds, was {elapsed:?} ago"
        );
    }

    #[test]
    fn exists_works() {
        let dir = TempDir::new().expect("tempdir");
        let s = store();
        let present = dir.path().join("present.txt");
        let absent = dir.path().join("absent.txt");
        s.write(&present, "here").expect("write");
        assert!(s.exists(&present));
        assert!(!s.exists(&absent));
    }

    #[test]
    fn create_parent_dirs_works() {
        let dir = TempDir::new().expect("tempdir");
        let s = store();
        let deep = dir.path().join("a").join("b").join("c").join("file.txt");
        s.create_parent_dirs(&deep).expect("create_parent_dirs");
        assert!(dir.path().join("a").join("b").join("c").exists());
    }

    #[test]
    fn list_dir_with_files() {
        let dir = TempDir::new().expect("tempdir");
        let s = store();
        s.write(&dir.path().join("b.txt"), "b").expect("write");
        s.write(&dir.path().join("a.txt"), "a").expect("write");
        let entries = s.list_dir(dir.path()).expect("list_dir");
        let names: Vec<_> = entries
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn list_dir_nonexisting() {
        let dir = TempDir::new().expect("tempdir");
        let s = store();
        let missing = dir.path().join("nope");
        let err = s.list_dir(&missing).expect_err("should fail");
        assert!(matches!(err, AppShotsError::FileNotFound { .. }));
    }

    #[test]
    fn containment_blocks_symlink_escape() {
        let project = TempDir::new().expect("project tempdir");
        let outside = TempDir::new().expect("outside tempdir");

        // Create a file outside the project
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, "secret data").expect("write outside");

        // Create a symlink inside the project pointing outside
        let link_path = project.path().join("escape.txt");
        std::os::unix::fs::symlink(&outside_file, &link_path).expect("symlink");

        let s = FsFileStore::new().with_project_dir(project.path().to_path_buf());

        let err = s
            .read(&link_path)
            .expect_err("should fail containment check");
        assert!(matches!(err, AppShotsError::InvalidPath { .. }));
    }

    #[test]
    fn containment_allows_normal_paths() {
        let project = TempDir::new().expect("tempdir");
        let s = FsFileStore::new().with_project_dir(project.path().to_path_buf());

        let path = project.path().join("hello.txt");
        s.write(&path, "hello").expect("write should succeed");
        let content = s.read(&path).expect("read should succeed");
        assert_eq!(content, "hello");
    }

    #[test]
    fn containment_none_allows_any_path() {
        // Default (no project_dir) should work as before
        let dir = TempDir::new().expect("tempdir");
        let s = store();
        let path = dir.path().join("file.txt");
        s.write(&path, "data").expect("write");
        assert_eq!(s.read(&path).expect("read"), "data");
    }

    #[test]
    fn flock_blocks_concurrent_write() {
        use std::os::unix::io::AsRawFd;

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("locked.txt");
        let s = store();
        s.write(&path, "initial").expect("write");

        // Hold an exclusive lock on the file
        let file = std::fs::File::open(&path).expect("open");
        let fd = file.as_raw_fd();
        // SAFETY: `fd` is a valid file descriptor from `File::open`.
        unsafe { libc::flock(fd, libc::LOCK_EX) };

        let err = s.write(&path, "blocked").expect_err("should be locked");
        assert!(matches!(err, AppShotsError::FileLocked { .. }));

        // Release lock
        // SAFETY: `fd` is still a valid file descriptor.
        unsafe { libc::flock(fd, libc::LOCK_UN) };
    }
}
