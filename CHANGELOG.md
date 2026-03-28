# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-03-28

### Fixed
- **CRITICAL: `capture_screenshots` broken on all Xcode versions** — `simctl list windows` never existed as a simctl subcommand; replaced with `simctl io booted screenshot` for clean framebuffer capture; device frames now added during compose via Typst templates
- **Bug: `screenshot_path` never passed to Typst templates** — `build_inputs()` was missing the `screenshot_path` key; templates always got `none`; now correctly set to `"/screenshot.png"` when capture data exists

### Added
- **`warm_simulator` tool** — pre-boots simulator, grants permissions, sets Apple canonical status bar (9:41, full battery/signal), optionally sets appearance
- **`seed_defaults` tool** — seeds UserDefaults via XML plist import; supports String, Int, Float, Bool, Array, Dict, base64 Data
- **`interact_simulator` tool** — scroll or tap via CGEvent mouse drag simulation; use before capture to scroll content into view
- **`plist_builder` service** — pure XML plist generation from JSON
- **Reference template** `examples/template-with-frame.typ` — device frame compositing, RTL, auto-scaling, OKLCH
- **Structured error variants** — replaced generic `SimulatorError`/`CaptureError` with 6 actionable variants
- **Device frame compositing docs** in `design-template` prompt with 3 approaches
- **`defaults import` docs** in `prepare-app` prompt with Date encoding gotchas

### Changed
- `capture_screenshots` uses `simctl io booted screenshot` instead of broken `screencapture -o -l <window_id>`
- `SimulatorNotBooted` maps to `INVALID_PARAMS` instead of `INTERNAL_ERROR`
- SERVER_INSTRUCTIONS expanded with new tools, workflows, composition rules
- `prepare-app` prompt recommends `defaults import` alongside `ScreenshotDataProvider`
- `generate-screenshots` prompt documents compose internals and optional prep steps
- Tool count: 21 → 24

### Dependencies
- Added `core-graphics = "0.24"` for CGEvent simulator interaction

## [0.2.0] - 2026-03-25

### Added
- **MCP Instructions**: comprehensive ~180-line guide for AI clients covering all 21 tools, 3 prompts, pipeline steps, key rules, common workflows, and directory structure
- **Font auto-discovery**: automatically loads `.ttf`/`.otf`/`.ttc`/`.woff2` from `appshots/fonts/`
- **Path containment**: `FsFileStore.with_project_dir()` prevents symlink escape attacks
- **Parallel rendering**: `compose_screenshots` uses `JoinSet` + `Semaphore(4)` for concurrent renders
- **Template caching**: templates read once per mode, shared across locales in batch renders
- **Typst compilation timeout**: 30s limit prevents infinite-loop templates from hanging the server
- **External command timeouts**: 60s for simctl/screencapture, 600s for fastlane deliver
- **Error code granularity**: user errors map to `INVALID_PARAMS`, system errors to `INTERNAL_ERROR`
- P1 tools: `list_simulators`, `get_project_status`, `run_deliver`, `get_caption_coverage`, `review_captions`, `save_template`, `get_template`, `suggest_font`

### Changed
- Bundled font parsing cached via `OnceLock` (parse once, reuse across renders)
- `build_inputs()` extracted as shared function (was duplicated in renderer and validator)
- Granular filters: `None` now means "process all" consistently across all tools
- `compose_screenshots` reads captions from config (matching `save_captions` storage)

## [0.1.0] - 2026-03-25

### Added
- Initial release with full MCP server implementation
- **21 MCP tools**: scan, analyze, plan, captions, design, render, capture, validate, deliver, glossary
- **3 MCP prompts**: `prepare-app`, `design-template`, `generate-screenshots`
- **Typst rendering engine**: embedded compilation with OKLCH color support, RTL/CJK text
- **39 ASO locales** with fallback chains (es-MX->es-ES, fr-CA->fr-FR, etc.)
- **2 required App Store sizes**: iPhone 6.9" (1320x2868), iPad 13" (2064x2752)
- FileStore abstraction with atomic writes, advisory locking, path validation
- MemoryStore for unit testing
- Template resolution chain: mode-specific -> shared -> root
- Keyword matching: Unicode-aware (word boundary for Latin, substring for CJK)
- 333 tests, 93%+ coverage

[0.3.0]: https://github.com/Murzav/appshots-mcp/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Murzav/appshots-mcp/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Murzav/appshots-mcp/releases/tag/v0.1.0
