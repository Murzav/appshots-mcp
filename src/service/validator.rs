use std::collections::HashMap;
use std::time::Duration;

use typst::diag::Severity;
use typst::layout::PagedDocument;

use crate::service::typst_renderer::{RenderParams, build_inputs};
use crate::service::typst_world::{AppWorld, COMPILE_TIMEOUT};

/// Severity of a validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
}

/// A single validation issue found during layout check.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
}

/// Validate a template by compiling it and checking for warnings/errors.
pub fn validate_layout(template_source: &str, params: &RenderParams) -> Vec<ValidationIssue> {
    let inputs = build_inputs(params);

    let mut files = HashMap::new();
    if let Some(ref data) = params.screenshot_data {
        files.insert("/screenshot.png".to_owned(), data.clone());
    }

    let world = AppWorld::new(template_source, inputs, params.extra_fonts.clone(), files);
    let warned = typst::compile::<PagedDocument>(&world);

    let mut issues = Vec::new();

    // Collect warnings
    for w in &warned.warnings {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Warning,
            message: w.message.to_string(),
        });
    }

    // Collect errors
    if let Err(errors) = &warned.output {
        for e in errors.iter() {
            let severity = match e.severity {
                Severity::Error => IssueSeverity::Error,
                Severity::Warning => IssueSeverity::Warning,
            };
            issues.push(ValidationIssue {
                severity,
                message: e.message.to_string(),
            });
        }
    }

    issues
}

/// Async version of `validate_layout` with a compilation timeout.
///
/// Runs the Typst compilation on a dedicated thread. If compilation
/// exceeds `COMPILE_TIMEOUT`, returns a single error issue.
pub async fn validate_layout_async(
    template_source: &str,
    params: &RenderParams,
) -> Vec<ValidationIssue> {
    validate_layout_with_timeout(template_source, params, COMPILE_TIMEOUT).await
}

async fn validate_layout_with_timeout(
    template_source: &str,
    params: &RenderParams,
    timeout: Duration,
) -> Vec<ValidationIssue> {
    let inputs = build_inputs(params);

    let mut files = HashMap::new();
    if let Some(ref data) = params.screenshot_data {
        files.insert("/screenshot.png".to_owned(), data.clone());
    }

    let world = AppWorld::new(template_source, inputs, params.extra_fonts.clone(), files);

    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let warned = typst::compile::<PagedDocument>(&world);
        let _ = tx.send(warned);
    });

    let warned = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(warned)) => warned,
        Ok(Err(_)) => {
            return vec![ValidationIssue {
                severity: IssueSeverity::Error,
                message: "validation thread panicked".into(),
            }];
        }
        Err(_) => {
            return vec![ValidationIssue {
                severity: IssueSeverity::Error,
                message: format!("compilation timed out after {}s", timeout.as_secs()),
            }];
        }
    };

    let mut issues = Vec::new();
    for w in &warned.warnings {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Warning,
            message: w.message.to_string(),
        });
    }
    if let Err(errors) = &warned.output {
        for e in errors.iter() {
            let severity = match e.severity {
                Severity::Error => IssueSeverity::Error,
                Severity::Warning => IssueSeverity::Warning,
            };
            issues.push(ValidationIssue {
                severity,
                message: e.message.to_string(),
            });
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::device::Device;
    use crate::model::locale::AsoLocale;
    use crate::service::typst_renderer::RenderParams;

    use crate::model::color::OklchColor;

    fn test_params() -> RenderParams {
        RenderParams {
            template_source: String::new(),
            caption_title: "Test".to_owned(),
            caption_subtitle: None,
            keyword: None,
            bg_colors: vec![],
            device: Device::Iphone6_9,
            locale: AsoLocale::EnUs,
            screenshot_data: None,
            extra_fonts: vec![],
        }
    }

    fn full_params() -> RenderParams {
        RenderParams {
            template_source: String::new(),
            caption_title: "Track Your Glucose".to_owned(),
            caption_subtitle: Some("Monitor daily trends".to_owned()),
            keyword: Some("glucose tracker".to_owned()),
            bg_colors: vec![
                OklchColor {
                    l: 50.0,
                    c: 0.15,
                    h: 240.0,
                    alpha: 1.0,
                },
                OklchColor {
                    l: 30.0,
                    c: 0.1,
                    h: 270.0,
                    alpha: 0.8,
                },
            ],
            device: Device::Ipad13,
            locale: AsoLocale::ArSa,
            screenshot_data: Some(vec![0x89, 0x50, 0x4E, 0x47]),
            extra_fonts: vec![],
        }
    }

    #[test]
    fn valid_template_no_issues() {
        let params = test_params();
        let issues = validate_layout(
            r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
Hello World"#,
            &params,
        );
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn template_with_error_produces_error_issue() {
        let params = test_params();
        let issues = validate_layout("#let x = ", &params);
        assert!(!issues.is_empty(), "expected at least one issue");
        assert!(
            issues.iter().any(|i| i.severity == IssueSeverity::Error),
            "expected an Error severity issue, got: {issues:?}"
        );
    }

    #[test]
    fn validate_with_full_params_exercises_all_inputs() {
        let params = full_params();
        let template = r#"
#set page(width: 688pt, height: 917pt, margin: 20pt)
#let title = sys.inputs.at("caption_title")
#let subtitle = sys.inputs.at("caption_subtitle", default: "")
#let kw = sys.inputs.at("keyword", default: "")
#let bg = sys.inputs.at("bg_color", default: "white")
#let grad = sys.inputs.at("bg_gradient", default: "")
#let dw = sys.inputs.at("device_width")
#let dh = sys.inputs.at("device_height")
#let loc = sys.inputs.at("locale")
#let dir = sys.inputs.at("text_direction")
#title \ #subtitle \ #kw \ #bg \ #dw × #dh \ #loc (#dir)
"#;
        let issues = validate_layout(template, &params);
        let errors: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn validate_with_rtl_locale() {
        let params = full_params(); // ArSa locale
        let template = r#"
#set page(width: 688pt, height: 917pt, margin: 20pt)
#let dir = sys.inputs.at("text_direction")
Direction: #dir
"#;
        let issues = validate_layout(template, &params);
        let errors: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn validate_error_contains_message() {
        let params = test_params();
        let issues = validate_layout("#unknown_func()", &params);
        assert!(!issues.is_empty());
        let error = &issues[0];
        assert_eq!(error.severity, IssueSeverity::Error);
        assert!(
            !error.message.is_empty(),
            "error message should not be empty"
        );
    }

    #[test]
    fn validate_no_warnings_on_clean_template() {
        let params = test_params();
        let issues = validate_layout(
            r#"#set page(width: 440pt, height: 956pt, margin: 0pt)
Clean template"#,
            &params,
        );
        let warnings: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .collect();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn issue_severity_equality() {
        assert_eq!(IssueSeverity::Error, IssueSeverity::Error);
        assert_eq!(IssueSeverity::Warning, IssueSeverity::Warning);
        assert_ne!(IssueSeverity::Error, IssueSeverity::Warning);
    }
}
