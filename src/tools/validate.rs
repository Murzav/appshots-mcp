use std::path::Path;
use std::str::FromStr;

use serde::Serialize;

use crate::error::AppShotsError;
use crate::io::FileStore;
use crate::model::device;
use crate::model::locale::AsoLocale;
use crate::service::template_resolver;
use crate::service::typst_renderer::RenderParams;
use crate::service::typst_world;
use crate::service::validator::{self, IssueSeverity};

#[derive(Debug, Serialize)]
pub struct ValidateResult {
    pub total_checks: usize,
    pub issues: Vec<ValidateIssueInfo>,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateIssueInfo {
    pub mode: u8,
    pub locale: String,
    pub device: String,
    pub severity: String,
    pub message: String,
}

/// Validate layout for specified modes/locales/devices.
pub(crate) async fn handle_validate_layout(
    store: &dyn FileStore,
    project_dir: &Path,
    modes: Option<&[u8]>,
    locales: Option<&[String]>,
) -> Result<ValidateResult, AppShotsError> {
    let appshots_dir = project_dir.join("appshots");
    let base_dir = appshots_dir
        .to_str()
        .ok_or_else(|| AppShotsError::InvalidPath {
            path: appshots_dir.clone(),
            reason: "non-UTF-8 path".into(),
        })?;

    // Determine which locales to validate (None = all 39 ASO locales)
    let target_locales: Vec<AsoLocale> = match locales {
        Some(codes) => codes
            .iter()
            .map(|c| AsoLocale::from_str(c))
            .collect::<Result<Vec<_>, _>>()?,
        None => crate::model::locale::ALL.to_vec(),
    };

    // Determine which modes to validate (None = all modes 1..=10)
    let target_modes: Vec<u8> = match modes {
        Some(m) => m.to_vec(),
        None => (1..=10).collect(),
    };

    let devices = device::REQUIRED;

    // Load project fonts once before the validation loop
    let project_fonts = typst_world::load_project_fonts(store, project_dir);

    let mut all_issues = Vec::new();
    let mut total_checks: usize = 0;

    for &mode in &target_modes {
        // Resolve template for this mode
        let template_path = template_resolver::resolve_template(base_dir, mode, |path| {
            store.exists(Path::new(path))
        })?;
        let template_source = store.read(&template_path.resolved)?;

        for &locale in &target_locales {
            for &dev in devices {
                total_checks += 1;

                let params = RenderParams {
                    template_source: template_source.clone(),
                    caption_title: "Validation Check".to_owned(),
                    caption_subtitle: Some("Subtitle Check".to_owned()),
                    keyword: None,
                    bg_colors: vec![],
                    device: dev,
                    locale,
                    screenshot_data: None,
                    extra_fonts: project_fonts.clone(),
                };

                let issues = validator::validate_layout_async(&template_source, &params).await;
                for issue in issues {
                    all_issues.push(ValidateIssueInfo {
                        mode,
                        locale: locale.code().to_owned(),
                        device: dev.display_name().to_owned(),
                        severity: match issue.severity {
                            IssueSeverity::Error => "error".to_owned(),
                            IssueSeverity::Warning => "warning".to_owned(),
                        },
                        message: issue.message,
                    });
                }
            }
        }
    }

    let passed = !all_issues.iter().any(|i| i.severity == "error");

    Ok(ValidateResult {
        total_checks,
        issues: all_issues,
        passed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;
    use std::path::PathBuf;

    const VALID_TEMPLATE: &str = r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
Hello World"#;

    const INVALID_TEMPLATE: &str = "#let x = ";

    fn setup_store(store: &MemoryStore, project_dir: &Path, template: &str) {
        let template_path = project_dir.join("appshots/template.typ");
        store.write(&template_path, template).unwrap();
    }

    #[tokio::test]
    async fn validate_valid_template_passes() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store(&store, &project_dir, VALID_TEMPLATE);

        let result = handle_validate_layout(
            &store,
            &project_dir,
            Some(&[1]),
            Some(&["en-US".to_owned()]),
        )
        .await
        .unwrap();

        assert!(result.passed);
        assert!(result.total_checks > 0);
        let errors: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.severity == "error")
            .collect();
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn validate_invalid_template_fails() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store(&store, &project_dir, INVALID_TEMPLATE);

        let result = handle_validate_layout(
            &store,
            &project_dir,
            Some(&[1]),
            Some(&["en-US".to_owned()]),
        )
        .await
        .unwrap();

        assert!(!result.passed);
        assert!(!result.issues.is_empty());
        assert!(result.issues.iter().any(|i| i.severity == "error"));
    }

    #[tokio::test]
    async fn validate_multiple_devices_checked() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store(&store, &project_dir, VALID_TEMPLATE);

        let result = handle_validate_layout(
            &store,
            &project_dir,
            Some(&[1]),
            Some(&["en-US".to_owned()]),
        )
        .await
        .unwrap();

        // 1 mode × 1 locale × 2 devices = 2 checks
        assert_eq!(result.total_checks, 2);
    }

    #[tokio::test]
    async fn validate_multiple_modes_and_locales() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store(&store, &project_dir, VALID_TEMPLATE);

        let result = handle_validate_layout(
            &store,
            &project_dir,
            Some(&[1, 1]), // same mode twice (uses shared template)
            Some(&["en-US".to_owned(), "fr-FR".to_owned()]),
        )
        .await
        .unwrap();

        // 2 modes × 2 locales × 2 devices = 8 checks
        assert_eq!(result.total_checks, 8);
        assert!(result.passed);
    }

    #[tokio::test]
    async fn validate_template_not_found() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        // No template

        let result = handle_validate_layout(
            &store,
            &project_dir,
            Some(&[1]),
            Some(&["en-US".to_owned()]),
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppShotsError::TemplateNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn validate_issue_info_fields() {
        let store = MemoryStore::new();
        let project_dir = PathBuf::from("/project");
        setup_store(&store, &project_dir, INVALID_TEMPLATE);

        let result = handle_validate_layout(
            &store,
            &project_dir,
            Some(&[3]),
            Some(&["fr-FR".to_owned()]),
        )
        .await
        .unwrap();

        let issue = &result.issues[0];
        assert_eq!(issue.mode, 3);
        assert_eq!(issue.locale, "fr-FR");
        assert!(!issue.device.is_empty());
        assert!(!issue.message.is_empty());
    }
}
