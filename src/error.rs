use std::path::PathBuf;

/// All errors produced by the appshots-mcp crate.
#[derive(Debug, thiserror::Error)]
pub enum AppShotsError {
    #[error("file not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("invalid path `{path}`: {reason}")]
    InvalidPath { path: PathBuf, reason: String },

    #[error("invalid format: {0}")]
    InvalidFormat(String),

    #[error("JSON parse error: {0}")]
    JsonParse(String),

    #[error("config not found: {path}")]
    ConfigNotFound { path: PathBuf },

    #[error("template not found: {path}")]
    TemplateNotFound { path: PathBuf },

    #[error("template compile error: {0}")]
    TemplateCompileError(String),

    #[error("capture error: {0}")]
    CaptureError(String),

    #[error("simulator error: {0}")]
    SimulatorError(String),

    #[error("locale not found: {0}")]
    LocaleNotFound(String),

    #[error("no active project")]
    NoActiveProject,

    #[error("file locked: {path}")]
    FileLocked { path: PathBuf },

    #[error("file too large: {size_mb}MB (max {max_mb}MB)")]
    FileTooLarge { size_mb: u64, max_mb: u64 },

    #[error("invalid color: {0}")]
    InvalidColor(String),

    #[error("render error: {0}")]
    RenderError(String),

    #[error("deliver error: {0}")]
    DeliverError(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AppShotsError>;

impl From<crate::model::locale::ParseLocaleError> for AppShotsError {
    fn from(e: crate::model::locale::ParseLocaleError) -> Self {
        Self::LocaleNotFound(e.to_string())
    }
}

impl From<AppShotsError> for rmcp::model::ErrorData {
    fn from(e: AppShotsError) -> Self {
        rmcp::model::ErrorData {
            code: rmcp::model::ErrorCode::INTERNAL_ERROR,
            message: e.to_string().into(),
            data: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_file_not_found() {
        let err = AppShotsError::FileNotFound {
            path: PathBuf::from("/tmp/missing.json"),
        };
        assert_eq!(err.to_string(), "file not found: /tmp/missing.json");
    }

    #[test]
    fn display_invalid_path() {
        let err = AppShotsError::InvalidPath {
            path: PathBuf::from("/bad"),
            reason: "not absolute".into(),
        };
        assert_eq!(err.to_string(), "invalid path `/bad`: not absolute");
    }

    #[test]
    fn display_file_too_large() {
        let err = AppShotsError::FileTooLarge {
            size_mb: 150,
            max_mb: 100,
        };
        assert_eq!(err.to_string(), "file too large: 150MB (max 100MB)");
    }

    #[test]
    fn display_no_active_project() {
        let err = AppShotsError::NoActiveProject;
        assert_eq!(err.to_string(), "no active project");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = AppShotsError::from(io_err);
        assert!(matches!(err, AppShotsError::Io(_)));
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn into_error_data() {
        let err = AppShotsError::InvalidColor("bad oklch".into());
        let data: rmcp::model::ErrorData = err.into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert_eq!(data.message.as_ref(), "invalid color: bad oklch");
    }

    #[test]
    fn from_serde_error() {
        let serde_err =
            serde_json::from_str::<serde_json::Value>("{{bad}}").expect_err("should fail");
        let err = AppShotsError::from(serde_err);
        assert!(matches!(err, AppShotsError::Serde(_)));
    }
}
