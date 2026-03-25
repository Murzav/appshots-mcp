pub mod fs;
pub mod memory;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::AppShotsError;

/// Abstract file store. All I/O goes through this trait.
/// Production: `FsFileStore`. Tests: `MemoryStore`.
pub trait FileStore: Send + Sync {
    fn read(&self, path: &Path) -> Result<String, AppShotsError>;
    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, AppShotsError>;
    fn write(&self, path: &Path, content: &str) -> Result<(), AppShotsError>;
    fn write_bytes(&self, path: &Path, content: &[u8]) -> Result<(), AppShotsError>;
    fn modified_time(&self, path: &Path) -> Result<SystemTime, AppShotsError>;
    fn exists(&self, path: &Path) -> bool;
    fn create_parent_dirs(&self, path: &Path) -> Result<(), AppShotsError>;
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, AppShotsError>;
}
