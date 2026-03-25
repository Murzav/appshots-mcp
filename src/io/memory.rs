use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::AppShotsError;
use crate::io::FileStore;

/// In-memory `FileStore` for unit tests.
/// Tracks file content and modification times for realistic caching behavior.
pub struct MemoryStore {
    files: Mutex<HashMap<PathBuf, (Vec<u8>, SystemTime)>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FileStore for MemoryStore {
    fn read(&self, path: &Path) -> Result<String, AppShotsError> {
        let files = self
            .files
            .lock()
            .map_err(|e| AppShotsError::InvalidFormat(format!("lock poisoned: {e}")))?;
        let (bytes, _) = files.get(path).ok_or_else(|| AppShotsError::FileNotFound {
            path: path.to_path_buf(),
        })?;
        String::from_utf8(bytes.clone())
            .map_err(|e| AppShotsError::InvalidFormat(format!("invalid UTF-8: {e}")))
    }

    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, AppShotsError> {
        let files = self
            .files
            .lock()
            .map_err(|e| AppShotsError::InvalidFormat(format!("lock poisoned: {e}")))?;
        files
            .get(path)
            .map(|(bytes, _)| bytes.clone())
            .ok_or_else(|| AppShotsError::FileNotFound {
                path: path.to_path_buf(),
            })
    }

    fn write(&self, path: &Path, content: &str) -> Result<(), AppShotsError> {
        let mut files = self
            .files
            .lock()
            .map_err(|e| AppShotsError::InvalidFormat(format!("lock poisoned: {e}")))?;
        files.insert(
            path.to_path_buf(),
            (content.as_bytes().to_vec(), SystemTime::now()),
        );
        Ok(())
    }

    fn write_bytes(&self, path: &Path, content: &[u8]) -> Result<(), AppShotsError> {
        let mut files = self
            .files
            .lock()
            .map_err(|e| AppShotsError::InvalidFormat(format!("lock poisoned: {e}")))?;
        files.insert(path.to_path_buf(), (content.to_vec(), SystemTime::now()));
        Ok(())
    }

    fn modified_time(&self, path: &Path) -> Result<SystemTime, AppShotsError> {
        let files = self
            .files
            .lock()
            .map_err(|e| AppShotsError::InvalidFormat(format!("lock poisoned: {e}")))?;
        files
            .get(path)
            .map(|(_, mtime)| *mtime)
            .ok_or_else(|| AppShotsError::FileNotFound {
                path: path.to_path_buf(),
            })
    }

    fn exists(&self, path: &Path) -> bool {
        self.files
            .lock()
            .map(|files| {
                // Exact file match
                if files.contains_key(path) {
                    return true;
                }
                // Directory-like match: any key has this path as prefix
                files.keys().any(|k| k.starts_with(path))
            })
            .unwrap_or(false)
    }

    fn create_parent_dirs(&self, _path: &Path) -> Result<(), AppShotsError> {
        Ok(()) // no-op for in-memory store
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, AppShotsError> {
        let files = self
            .files
            .lock()
            .map_err(|e| AppShotsError::InvalidFormat(format!("lock poisoned: {e}")))?;

        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for k in files.keys() {
            if let Ok(rel) = k.strip_prefix(path) {
                // Get the first component (immediate child — file or virtual dir)
                if let Some(first) = rel.components().next() {
                    let child = path.join(first);
                    if seen.insert(child.clone()) {
                        result.push(child);
                    }
                }
            }
        }

        if result.is_empty() {
            return Err(AppShotsError::FileNotFound {
                path: path.to_path_buf(),
            });
        }
        result.sort();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_roundtrip() {
        let store = MemoryStore::new();
        let path = Path::new("/test/hello.txt");
        store.write(path, "hello world").expect("write");
        let content = store.read(path).expect("read");
        assert_eq!(content, "hello world");
    }

    #[test]
    fn file_not_found() {
        let store = MemoryStore::new();
        let err = store.read(Path::new("/missing")).expect_err("should fail");
        assert!(matches!(err, AppShotsError::FileNotFound { .. }));
    }

    #[test]
    fn exists_works() {
        let store = MemoryStore::new();
        let path = Path::new("/test/exists.txt");
        assert!(!store.exists(path));
        store.write(path, "data").expect("write");
        assert!(store.exists(path));
    }

    #[test]
    fn list_dir_empty_and_populated() {
        let store = MemoryStore::new();
        let dir = Path::new("/mydir");

        // Not found when nothing exists
        let err = store.list_dir(dir).expect_err("should fail");
        assert!(matches!(err, AppShotsError::FileNotFound { .. }));

        // Add files
        store.write(Path::new("/mydir/b.txt"), "b").expect("write");
        store.write(Path::new("/mydir/a.txt"), "a").expect("write");
        // File in subdirectory should not appear
        store
            .write(Path::new("/mydir/sub/c.txt"), "c")
            .expect("write");

        let entries = store.list_dir(dir).expect("list_dir");
        let names: Vec<_> = entries
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        // Returns immediate files and virtual subdirectories
        assert_eq!(names, vec!["a.txt", "b.txt", "sub"]);
    }

    #[test]
    fn write_bytes_roundtrip() {
        let store = MemoryStore::new();
        let path = Path::new("/test/data.bin");
        let data = vec![0u8, 1, 2, 255, 128];
        store.write_bytes(path, &data).expect("write_bytes");
        let result = store.read_bytes(path).expect("read_bytes");
        assert_eq!(result, data);
    }
}
