# appshots-mcp

[![CI](https://github.com/Murzav/appshots-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/Murzav/appshots-mcp/actions/workflows/ci.yml)
![Coverage](https://img.shields.io/endpoint?url=https://gist.githubusercontent.com/Murzav/077a4369198624c7b57c1fadfa868d65/raw/coverage.json)
[![Crates.io](https://img.shields.io/crates/v/appshots-mcp)](https://crates.io/crates/appshots-mcp)
[![Downloads](https://img.shields.io/crates/d/appshots-mcp)](https://crates.io/crates/appshots-mcp)
[![MSRV](https://img.shields.io/badge/MSRV-1.90-blue)](https://blog.rust-lang.org/)
[![License](https://img.shields.io/crates/l/appshots-mcp)](LICENSE-MIT)
[![MCP](https://img.shields.io/badge/MCP-compatible-green)](https://modelcontextprotocol.io)

MCP server for generating ASO-optimized App Store screenshots. Generates up to **780 final images** per app (39 locales x 5-10 screenshots x 1-2 devices).

AI logic lives in Claude Code; the server provides tools, rendering, and validation.

## Why appshots-mcp?

| Current solution | Problem |
|-----------------|---------|
| Manual Figma | Days of work, repeat on every update |
| AppScreens / Screenshots.pro | Paid, vendor lock-in |
| fastlane frameit + ImageMagick | Flat PNG frames, ~10% layout accuracy, RTL/emoji breaks |

**Core problem:** screenshot captions are disconnected from ASO keywords. Captions must reinforce keyword coverage, not be random marketing text. appshots-mcp solves this by integrating ASO keyword analysis directly into the screenshot generation pipeline.

## Features

- **24 MCP tools** for the complete screenshot pipeline
- **3 MCP prompts** guiding app preparation, template design, and batch generation
- **Simulator state management** — pre-warm, seed UserDefaults, scroll/tap before capture
- **Clean capture + Typst compositing** — framebuffer screenshots with device frames added in templates
- **Typst rendering** — native RTL/CJK support, exact layout, sub-second renders
- **OKLCH color space** exclusively — perceptually uniform, no hex/RGB
- **39 ASO locales** with fallback chains
- **Granular regeneration** — fix one screenshot without re-rendering all 780
- **Keyword-aware captions** — AI incorporates ASO keywords into every locale's captions
- **Parallel rendering** with configurable concurrency (4 concurrent by default)
- **Auto font discovery** from `appshots/fonts/` directory
- **Compilation timeout** (30s) prevents infinite-loop templates
- **Atomic file writes** with advisory locking
- **Path containment** prevents symlink escape attacks

## Quick Start

### Installation

**Homebrew (macOS/Linux):**
```bash
brew install Murzav/tap/appshots-mcp
```

**Cargo:**
```bash
cargo install appshots-mcp
```

**cargo-binstall (prebuilt binary):**
```bash
cargo binstall appshots-mcp
```

### Configuration

**Claude Code:**

```bash
claude mcp add appshots-mcp -- appshots-mcp --project-dir /path/to/your/app
```

**Generic MCP client (stdio):**

```json
{
  "mcpServers": {
    "appshots-mcp": {
      "command": "appshots-mcp",
      "args": ["--project-dir", "/path/to/your/app"]
    }
  }
}
```

### CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `--project-dir` | `.` | Path to the app project root |
| `--glossary-path` | `glossary.json` | Path to shared glossary file |
| `--config-path` | `appshots.json` | Path to screenshot config |

## Usage

### Typical Workflow

1. **Prepare your app** — use the `prepare-app` prompt to create `ScreenshotMode` enum in Swift
2. **Design templates** — use the `design-template` prompt to create Typst templates with `preview_design`
3. **Generate all screenshots** — use the `generate-screenshots` prompt for the full pipeline:

```
scan_project → analyze_keywords → plan_screens → save_captions (en-US)
→ translate captions (38 locales) → validate_layout
→ warm_simulator → seed_defaults → capture_screenshots
→ compose_screenshots → run_deliver
```

### Tools

#### Capture & Setup

| Tool | Description |
|------|-------------|
| `list_simulators` | List available iOS simulators (UDID, runtime, state) |
| `warm_simulator` | Pre-boot simulator, grant permissions, set Apple canonical status bar (9:41) |
| `seed_defaults` | Seed UserDefaults via plist import (run after install, before launch) |
| `interact_simulator` | Scroll or tap in iOS Simulator via CGEvent mouse drag simulation |
| `capture_screenshots` | Capture clean screenshots from simulator framebuffer (no device bezels) |

#### Discovery & Analysis

| Tool | Description |
|------|-------------|
| `scan_project` | Parse `fastlane/metadata/` across all 39 locales |
| `analyze_keywords` | Find keyword coverage gaps for a locale |
| `get_project_status` | Check config, templates, captions, captures readiness |

#### Strategy

| Tool | Description |
|------|-------------|
| `plan_screens` | Save mode -> keyword -> messaging mapping |
| `get_plans` | Retrieve current screen plans |
| `save_captions` | Save captions for a locale (upsert by mode) |
| `get_captions` | Get captions with locale/mode filters |
| `get_locale_keywords` | Read keywords.txt for a locale |
| `get_caption_coverage` | Coverage matrix: locale x mode |
| `review_captions` | Keyword coverage analysis per caption |

#### Design & Rendering

| Tool | Description |
|------|-------------|
| `save_template` | Save Typst template source |
| `get_template` | Read template with resolution chain |
| `preview_design` | Render a single design preview |
| `validate_layout` | Check all templates for errors/warnings |
| `suggest_font` | Suggest system font for a locale's script |
| `compose_screenshots` | Render final PNGs via Typst |

#### Pipeline & Glossary

| Tool | Description |
|------|-------------|
| `run_deliver` | Run `fastlane deliver` to upload screenshots |
| `get_glossary` | Get glossary entries (shared with xcstrings-mcp) |
| `update_glossary` | Update glossary entries |

### Prompts

| Prompt | Description |
|--------|-------------|
| `prepare-app` | Guide: create ScreenshotMode enum + ScreenshotDataProvider in Swift; documents `defaults import` for seeding mock data |
| `design-template` | Guide: design Typst template with OKLCH colors, auto-scaling text, RTL support; includes device frame compositing approaches |
| `generate-screenshots` | Guide: full 10-step pipeline from scan to deliver; covers warm/seed/interact prep steps and compose internals |

### Granular Regeneration

All rendering tools accept optional `modes` and `locales` filters. Omitting = process all.

```
"Fix screenshot 3"         → compose_screenshots(modes: [3])
"Fix German text on #5"    → compose_screenshots(modes: [5], locales: ["de-DE"])
"Re-capture stats screen"  → capture_screenshots(modes: [4])
```

## Key Rules

- **OKLCH Only**: All colors use `oklch(L%, C, Hdeg)`. No hex, RGB, or HSL.
- **Template Resolution**: `templates/template-{mode}.typ` -> `templates/template.typ` -> `template.typ`
- **Locale Fallback**: es-MX->es-ES, fr-CA->fr-FR, en-AU/CA/GB->en-US, pt-PT->pt-BR, zh-Hant->zh-Hans
- **Required Sizes**: iPhone 6.9" (1320x2868), iPad 13" (2064x2752). Max 10 per locale.

## How Screenshot Modes Work

Each app screen is assigned a numeric **mode** (1-10). The app's `ScreenshotMode` Swift enum maps modes to specific UI states:

1. The app is launched with `xcrun simctl launch --screenshot-N <udid> <bundle_id>` where N is the mode number
2. `ScreenshotDataProvider` in the app checks `ProcessInfo.processInfo.arguments` for `--screenshot-N` and configures mock data accordingly
3. For complex state (e.g., streak counts, pro status), use `seed_defaults` to write UserDefaults **before** launching the app
4. `capture_screenshots` captures a clean framebuffer image per mode — no device bezels
5. `compose_screenshots` renders the final PNG via Typst, overlaying captions and optionally adding device frames

## Project Directory Structure

```
project-root/
├── fastlane/
│   ├── metadata/{locale}/    ← keywords.txt, name.txt, subtitle.txt
│   └── screenshots/{locale}/ ← final output
├── appshots.json             ← project config (plan, captions, template)
├── appshots/
│   ├── template.typ          ← single template
│   ├── templates/            ← per-screen templates
│   ├── fonts/                ← custom fonts (.ttf, .otf, .woff2)
│   ├── captures/             ← simulator captures (clean framebuffer)
│   ├── previews/             ← design iteration previews
│   └── .seed-defaults.plist  ← temporary plist for defaults import
├── examples/                 ← reference Typst templates
└── glossary.json             ← shared with xcstrings-mcp
```

## Device Frame Compositing

`capture_screenshots` produces clean framebuffer images without device bezels. Device frames are added during `compose_screenshots` via Typst templates. Three approaches:

1. **Raw screenshot** — no frame, just the capture with captions overlay
2. **Rounded card** — Typst `rect(radius: ...)` with `clip: true` to simulate device corners
3. **PNG overlay** — place a transparent device frame PNG on top of the screenshot (pixel-perfect Apple bezels)

See `examples/template-with-frame.typ` for a reference template demonstrating approach #2 with RTL support, auto-scaling text, and OKLCH colors.

## Architecture

```
main.rs     → CLI (clap), tokio current_thread, stdio transport
server.rs   → #[tool_router] (24 tools) + #[prompt_router] (3 prompts)
tools/      → capture, scan, analyze, plan, captions, design, render,
              deliver, validate, glossary — all I/O via FileStore trait
service/    → metadata_parser, locale, keyword_matcher, font_resolver,
              template_resolver, typst_renderer, typst_world, validator,
              config_parser — pure functions, NO I/O
model/      → ProjectConfig, Caption, OklchColor, AsoLocale, Device,
              TemplateConfig — data types with serde + JsonSchema
io/         → FileStore trait + FsFileStore (atomic writes, flock)
error.rs    → AppShotsError enum (thiserror)
prompts.rs  → prompt content generators
```

**Layer rule:** `server -> tools -> service -> model`. Services have NO I/O.

## Performance

| Operation | Target |
|-----------|--------|
| `scan_project` (39 locales) | < 50ms |
| `compose_screenshots` (1) | < 100ms |
| `compose_screenshots` (all, parallel) | < 20s |
| `capture_screenshots` (1x1) | ~3s |

## Related

- [MCP Protocol](https://modelcontextprotocol.io) — Model Context Protocol specification
- [xcstrings-mcp](https://github.com/Murzav/xcstrings-mcp) — Sister project for .xcstrings localization
- [Typst](https://typst.app) — The typesetting system used for rendering
- [fastlane](https://fastlane.tools) — App Store automation

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! Please open an issue or submit a PR.
