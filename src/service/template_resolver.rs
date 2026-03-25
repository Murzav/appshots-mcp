use std::path::PathBuf;

use crate::error::AppShotsError;
use crate::model::template::{ResolutionSource, TemplatePath};

/// Resolve template file for a given mode.
///
/// Uses a closure for existence check to stay pure (no I/O).
/// Resolution chain:
/// 1. `{base}/templates/template-{mode}.typ`
/// 2. `{base}/templates/template.typ`
/// 3. `{base}/template.typ`
pub fn resolve_template(
    base_dir: &str,
    mode: u8,
    exists_fn: impl Fn(&str) -> bool,
) -> Result<TemplatePath, AppShotsError> {
    // 1. Mode-specific template
    let mode_specific = format!("{base_dir}/templates/template-{mode}.typ");
    if exists_fn(&mode_specific) {
        return Ok(TemplatePath {
            resolved: PathBuf::from(mode_specific),
            source: ResolutionSource::ModeSpecific { mode },
        });
    }

    // 2. Shared template in templates/
    let shared = format!("{base_dir}/templates/template.typ");
    if exists_fn(&shared) {
        return Ok(TemplatePath {
            resolved: PathBuf::from(shared),
            source: ResolutionSource::SharedFallback,
        });
    }

    // 3. Root template
    let root = format!("{base_dir}/template.typ");
    if exists_fn(&root) {
        return Ok(TemplatePath {
            resolved: PathBuf::from(root),
            source: ResolutionSource::RootFallback,
        });
    }

    Err(AppShotsError::TemplateNotFound {
        path: PathBuf::from(format!(
            "{base_dir}/templates/template-{mode}.typ (and fallbacks)"
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_mode_specific() {
        let result = resolve_template("/app/appshots", 3, |path| {
            path == "/app/appshots/templates/template-3.typ"
        })
        .unwrap();
        assert_eq!(
            result.resolved,
            PathBuf::from("/app/appshots/templates/template-3.typ")
        );
        assert_eq!(result.source, ResolutionSource::ModeSpecific { mode: 3 });
    }

    #[test]
    fn resolve_shared_fallback() {
        let result = resolve_template("/app/appshots", 3, |path| {
            path == "/app/appshots/templates/template.typ"
        })
        .unwrap();
        assert_eq!(
            result.resolved,
            PathBuf::from("/app/appshots/templates/template.typ")
        );
        assert_eq!(result.source, ResolutionSource::SharedFallback);
    }

    #[test]
    fn resolve_root_fallback() {
        let result = resolve_template("/app/appshots", 1, |path| {
            path == "/app/appshots/template.typ"
        })
        .unwrap();
        assert_eq!(result.resolved, PathBuf::from("/app/appshots/template.typ"));
        assert_eq!(result.source, ResolutionSource::RootFallback);
    }

    #[test]
    fn resolve_prefers_mode_specific_over_shared() {
        let result = resolve_template("/app/appshots", 2, |path| {
            path == "/app/appshots/templates/template-2.typ"
                || path == "/app/appshots/templates/template.typ"
                || path == "/app/appshots/template.typ"
        })
        .unwrap();
        assert_eq!(result.source, ResolutionSource::ModeSpecific { mode: 2 });
    }

    #[test]
    fn resolve_prefers_shared_over_root() {
        let result = resolve_template("/app/appshots", 5, |path| {
            path == "/app/appshots/templates/template.typ" || path == "/app/appshots/template.typ"
        })
        .unwrap();
        assert_eq!(result.source, ResolutionSource::SharedFallback);
    }

    #[test]
    fn resolve_none_found() {
        let result = resolve_template("/app/appshots", 1, |_| false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppShotsError::TemplateNotFound { .. }));
    }
}
