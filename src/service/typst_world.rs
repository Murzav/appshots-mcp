use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use typst::foundations::{Bytes, Datetime, Dict};
use typst::layout::PagedDocument;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use std::time::Duration;

use crate::error::AppShotsError;

/// Default timeout for Typst template compilation.
pub(crate) const COMPILE_TIMEOUT: Duration = Duration::from_secs(30);

/// Cached bundled fonts — parsed once, reused across all renders.
fn bundled_fonts() -> &'static [Font] {
    static FONTS: OnceLock<Vec<Font>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data);
            for font in Font::iter(bytes) {
                fonts.push(font);
            }
        }
        fonts
    })
}

/// Minimal Typst World for rendering screenshot templates.
///
/// All sources and files are in-memory — no filesystem access.
pub(crate) struct AppWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main_id: FileId,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
}

impl AppWorld {
    /// Create a new world for compiling a single template.
    ///
    /// - `template_source`: Typst source code for the main template
    /// - `inputs`: key-value pairs accessible via `sys.inputs` in Typst
    /// - `extra_fonts`: additional font file bytes (e.g. custom project fonts)
    /// - `files`: path → content map for binary files (images, etc.)
    pub(crate) fn new(
        template_source: &str,
        inputs: Dict,
        extra_fonts: Vec<Vec<u8>>,
        files: HashMap<String, Vec<u8>>,
    ) -> Self {
        // Start with cached bundled fonts (parsed once, shared across renders)
        let mut fonts: Vec<Font> = bundled_fonts().to_vec();

        // Add extra fonts
        for font_data in extra_fonts {
            let bytes = Bytes::new(font_data);
            for font in Font::iter(bytes) {
                fonts.push(font);
            }
        }

        // Build font book
        let book = LazyHash::new(FontBook::from_fonts(&fonts));

        // Build library with sys.inputs
        let library = LazyHash::new(Library::builder().with_inputs(inputs).build());

        // Create main source file
        let main_id = FileId::new(None, VirtualPath::new(Path::new("/main.typ")));
        let source = Source::new(main_id, template_source.to_owned());
        let mut sources = HashMap::with_capacity(1);
        sources.insert(main_id, source);

        // Convert file paths to FileId → Bytes
        let files = files
            .into_iter()
            .map(|(path, data)| {
                let id = FileId::new(None, VirtualPath::new(Path::new(&path)));
                (id, Bytes::new(data))
            })
            .collect();

        Self {
            library,
            book,
            fonts,
            main_id,
            sources,
            files,
        }
    }
}

impl World for AppWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> typst::diag::FileResult<Source> {
        self.sources
            .get(&id)
            .cloned()
            .ok_or(typst::diag::FileError::NotFound(
                id.vpath().as_rootless_path().into(),
            ))
    }

    fn file(&self, id: FileId) -> typst::diag::FileResult<Bytes> {
        self.files
            .get(&id)
            .cloned()
            .ok_or(typst::diag::FileError::NotFound(
                id.vpath().as_rootless_path().into(),
            ))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        Datetime::from_ymd(2026, 1, 1)
    }
}

/// Compile a template source into a paged document.
pub(crate) fn compile_template(
    world: &AppWorld,
) -> Result<(PagedDocument, Vec<String>), AppShotsError> {
    let warned = typst::compile::<PagedDocument>(world);
    let warnings: Vec<String> = warned
        .warnings
        .iter()
        .map(|w| w.message.to_string())
        .collect();

    match warned.output {
        Ok(doc) => Ok((doc, warnings)),
        Err(errors) => {
            let messages: Vec<String> = errors.iter().map(|e| e.message.to_string()).collect();
            Err(AppShotsError::TemplateCompileError(messages.join("; ")))
        }
    }
}

/// Compile a template with a timeout, running on a dedicated thread.
///
/// `AppWorld` is `Send`, so we move it into a spawned thread.
/// If compilation exceeds `timeout`, the caller gets an error
/// (the orphaned thread will still run to completion, but the result is discarded).
pub(crate) async fn compile_template_with_timeout(
    world: AppWorld,
    timeout: Duration,
) -> Result<(PagedDocument, Vec<String>), AppShotsError> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    std::thread::spawn(move || {
        let result = compile_template(&world);
        let _ = tx.send(result);
    });

    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(AppShotsError::TemplateCompileError(
            "compilation thread panicked".into(),
        )),
        Err(_) => Err(AppShotsError::TemplateCompileError(format!(
            "compilation timed out after {}s",
            timeout.as_secs()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use typst::foundations::{IntoValue, Str};

    use super::*;

    fn empty_inputs() -> Dict {
        Dict::new()
    }

    #[test]
    fn world_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AppWorld>();
    }

    #[test]
    fn source_returns_template() {
        let world = AppWorld::new("Hello, world!", empty_inputs(), vec![], HashMap::new());
        let source = world.source(world.main());
        assert!(source.is_ok());
        assert_eq!(
            source.as_ref().map(|s| s.text()).ok(),
            Some("Hello, world!")
        );
    }

    #[test]
    fn source_not_found_for_unknown_id() {
        let world = AppWorld::new("test", empty_inputs(), vec![], HashMap::new());
        let unknown_id = FileId::new(None, VirtualPath::new(Path::new("/unknown.typ")));
        assert!(world.source(unknown_id).is_err());
    }

    #[test]
    fn font_returns_bundled_fonts() {
        let world = AppWorld::new("test", empty_inputs(), vec![], HashMap::new());
        // typst_assets bundles several fonts — at minimum index 0 should exist
        assert!(world.font(0).is_some());
    }

    #[test]
    fn file_returns_mapped_content() {
        let mut files = HashMap::new();
        files.insert("/screenshot.png".to_owned(), vec![0x89, 0x50, 0x4E, 0x47]);
        let world = AppWorld::new("test", empty_inputs(), vec![], files);

        let file_id = FileId::new(None, VirtualPath::new(Path::new("/screenshot.png")));
        let result = world.file(file_id);
        assert!(result.is_ok());
        assert_eq!(result.as_ref().map(|b| b.len()).ok(), Some(4));
    }

    #[test]
    fn file_not_found_for_unknown_path() {
        let world = AppWorld::new("test", empty_inputs(), vec![], HashMap::new());
        let unknown_id = FileId::new(None, VirtualPath::new(Path::new("/missing.png")));
        assert!(world.file(unknown_id).is_err());
    }

    #[test]
    fn compile_simple_template() {
        let world = AppWorld::new("Hello, world!", empty_inputs(), vec![], HashMap::new());
        let result = compile_template(&world);
        assert!(result.is_ok());
        let (doc, _warnings) = result.as_ref().ok().map(|(d, w)| (d, w)).unwrap();
        assert!(!doc.pages.is_empty());
    }

    #[test]
    fn compile_with_inputs() {
        let mut inputs = Dict::new();
        inputs.insert("title".into(), Str::from("Test Title").into_value());

        let source = r#"#sys.inputs.title"#;
        let world = AppWorld::new(source, inputs, vec![], HashMap::new());
        let result = compile_template(&world);
        assert!(result.is_ok());
    }

    #[test]
    fn compile_error_returns_template_compile_error() {
        let world = AppWorld::new("#invalid-syntax(", empty_inputs(), vec![], HashMap::new());
        let result = compile_template(&world);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, AppShotsError::TemplateCompileError(_)));
    }

    #[tokio::test]
    async fn compile_with_timeout_succeeds() {
        let world = AppWorld::new("Hello, world!", empty_inputs(), vec![], HashMap::new());
        let result = compile_template_with_timeout(world, Duration::from_secs(10)).await;
        assert!(result.is_ok());
        let (doc, _warnings) = result.unwrap();
        assert!(!doc.pages.is_empty());
    }

    #[tokio::test]
    async fn compile_with_timeout_returns_error_on_invalid_template() {
        let world = AppWorld::new("#invalid-syntax(", empty_inputs(), vec![], HashMap::new());
        let result = compile_template_with_timeout(world, Duration::from_secs(10)).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppShotsError::TemplateCompileError(_)
        ));
    }
}
