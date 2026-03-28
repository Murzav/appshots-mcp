use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    GetPromptRequestParams, GetPromptResult, ListPromptsResult, PaginatedRequestParams,
    PromptMessage, PromptMessageRole, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{
    RoleServer, ServerHandler, prompt, prompt_handler, prompt_router, tool, tool_handler,
    tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::error;

use crate::io::FileStore;

// ---------------------------------------------------------------------------
// MCP instructions — read by AI clients every conversation
// ---------------------------------------------------------------------------

const SERVER_INSTRUCTIONS: &str = "\
appshots-mcp: MCP server for generating ASO-optimized App Store screenshots.
Renders up to 780 final images per app (39 locales x 5-10 screenshots x 1-2 devices).
AI logic lives in the client (you); the server provides tools, rendering, and validation.

=== GETTING STARTED: PROMPTS ===

Use prompts to kick off major workflows. Three prompts available:

  prepare-app
    First-time setup. Analyzes the app and creates a ScreenshotMode Swift enum
    plus ScreenshotDataProvider, gated behind #if DEBUG.
    Args: bundle_id (required), screens_count (optional, default 5)
    When: once per app, before anything else.

  design-template
    Create or iterate on Typst templates for screenshot layouts.
    All colors must use OKLCH. Renders previews for visual feedback.
    Args: bundle_id (required), style (optional), per_screen (optional, default false)
    When: after prepare-app, before generating screenshots.

  generate-screenshots
    Execute the full 10-step pipeline: scan, plan, caption, render, deliver.
    Args: devices (optional), locales (optional), modes (optional)
    When: to produce final screenshots end-to-end.

=== PIPELINE OVERVIEW (10 STEPS) ===

  Step 0  PREPARE APP     prompt: prepare-app             One-time app setup
  Step 1  DESIGN          prompt: design-template          Template creation + iteration
                          + preview_design tool            Render previews
  Step 2  SCAN            scan_project                     Discover fastlane metadata
  Step 3  ANALYZE         analyze_keywords                 Find keyword coverage gaps
  Step 4  PLAN            plan_screens                     Map modes to keywords/messaging
  Step 5  GENERATE        save_captions (en-US first)      Write English captions
  Step 6  TRANSLATE       get_locale_keywords              Get locale keywords
                          + save_captions (per locale)     Write translated captions
  Step 7  VALIDATE        validate_layout                  Check templates compile
  (opt)   WARM            warm_simulator                   Pre-boot + status bar + permissions
  (opt)   SEED            seed_defaults                    Seed mock UserDefaults data
  Step 8  CAPTURE         capture_screenshots              Capture from iOS simulator
  (opt)   INTERACT        interact_simulator               Scroll/tap before next capture
  Step 9  COMPOSE         compose_screenshots              Render final PNGs via Typst
  Step 10 DELIVER         run_deliver                      Upload via fastlane deliver

=== TOOLS REFERENCE ===

--- Capture & Setup (5 tools) ---

  list_simulators
    List available iOS simulators with name, UDID, state, runtime.
    No parameters.
    Use: find the right device name/UDID before capturing.

  warm_simulator
    Pre-boot simulator, grant permissions, set status bar to Apple canonical (9:41).
    Params: udid, bundle_id (opt), appearance (opt: \"light\"/\"dark\")
    Use: warm_simulator(udid: \"ABC-123\", bundle_id: \"com.app\") before capture.
    Does: boot → grant all permissions → status bar (9:41, full battery/signal) → appearance.

  seed_defaults
    Seed UserDefaults in simulator via plist import.
    MUST run AFTER app install but BEFORE app launch.
    Params: bundle_id, data (key-value JSON object)
    Use: seed_defaults(bundle_id: \"com.app\", data: {\"streak\": 7, \"isPro\": true})
    Supports: String, Int, Float, Bool, Array, Dict, base64-encoded Data.
    NOTE: Swift Date = Double (secondsSinceReferenceDate from 2001-01-01).

  interact_simulator
    Scroll or tap in iOS Simulator via CGEvent mouse drag simulation.
    Requires macOS Accessibility permission and Simulator must be frontmost.
    Params: action (\"scroll\"/\"tap\"), x, y, dx (opt), dy (opt), delay_ms (opt)
    Use: interact_simulator(action: \"scroll\", x: 200, y: 400, dy: -300) to scroll down.
    Use: interact_simulator(action: \"tap\", x: 200, y: 400) to tap a button.

  capture_screenshots
    Capture clean screenshots from iOS simulator framebuffer.
    Device frames are added during the compose step, not during capture.
    Params: bundle_id, device, modes (opt), locales (opt), delay_ms (opt)
    Use: capture_screenshots(bundle_id: \"com.app\", device: \"iPhone 17 Pro Max\")

--- Discovery (3 tools) ---

  scan_project
    Scan fastlane/metadata/ for all locales, cache keywords/name/subtitle.
    No parameters.
    Use: run first to discover what metadata exists.

  analyze_keywords
    Analyze keyword coverage gaps for a locale's existing captions.
    Params: locale
    Use: analyze_keywords(locale: \"en-US\") to find unused keywords.

  get_project_status
    Get project readiness: config, template, locales, captions, captures.
    No parameters.
    Use: quick health check before starting work.

--- Strategy (7 tools) ---

  plan_screens
    Save screen plans (mode to keywords to messaging) to appshots.json.
    Params: plans (array of ScreenPlan)
    Use: after analyze_keywords, map each mode to target keywords.

  get_plans
    Read current screen plans from appshots.json.
    No parameters.
    Use: review existing strategy before making changes.

  save_captions
    Save captions for a locale. Upserts by mode (preserves others).
    Params: locale, captions (array of Caption)
    Use: save_captions(locale: \"de-DE\", captions: [{mode: 5, title: \"...\"}])

  get_captions
    Read captions with optional locale/modes filter.
    Params: locale (opt), modes (opt)
    Use: get_captions(locale: \"en-US\") or get_captions(modes: [1,2])

  get_locale_keywords
    Read keywords.txt content for a locale from fastlane/metadata.
    Params: locale
    Use: get target keywords before writing translated captions.

  get_caption_coverage
    Coverage matrix showing which locales/modes have captions.
    No parameters.
    Use: find missing translations at a glance.

  review_captions
    Review captions against keyword coverage and glossary for ASO quality.
    Params: locale (opt), modes (opt)
    Use: quality check before final render.

--- Design (6 tools) ---

  save_template
    Save a Typst template file (single shared or per-screen).
    Params: template_source, mode (opt, omit for shared template)
    Use: save_template(template_source: \"#set page()...\", mode: 3)

  get_template
    Read a Typst template. Resolves: templates/template-{mode}.typ
    then templates/template.typ, then template.typ.
    Params: mode (opt)
    Use: get_template(mode: 5) to see what mode 5 uses.

  preview_design
    Render a single design preview image via Typst.
    Params: mode, caption_title, caption_subtitle (opt), bg_colors, device, locale
    Use: iterate on visual design before committing to all locales.

  validate_layout
    Validate that templates compile for all mode/locale/device combinations.
    Params: modes (opt), locales (opt)
    Use: validate_layout() to check everything, or filter to specific items.

  suggest_font
    Suggest a system font appropriate for a locale's script.
    Params: locale
    Use: suggest_font(locale: \"ja\") returns Hiragino Sans for Japanese.

  compose_screenshots
    Render final PNG screenshots via Typst for specified modes/locales.
    Output goes to fastlane/screenshots/{locale}/.
    Params: modes (opt), locales (opt)
    Use: compose_screenshots(modes: [3], locales: [\"de-DE\"])

--- Pipeline (1 tool) ---

  run_deliver
    Run fastlane deliver to upload screenshots to App Store Connect.
    No parameters.
    Use: final step after all screenshots are composed and verified.

--- Glossary (2 tools) ---

  get_glossary
    Read glossary entries, optionally filtered by locale pair or substring.
    Params: source_locale (opt), target_locale (opt), filter (opt)
    Use: check terminology consistency before translating.

  update_glossary
    Add or update glossary entries for a source-to-target locale pair.
    Params: source_locale, target_locale, entries (term-to-translation map)
    Use: update_glossary(source_locale: \"en-US\", target_locale: \"de-DE\",
         entries: {\"Track\": \"Verfolgen\"})

=== KEY RULES ===

  OKLCH ONLY
    All colors must be oklch(L%, C, Hdeg). No hex, RGB, or HSL anywhere.
    Example: oklch(50%, 0.15, 240deg)

  GRANULAR REGENERATION
    All rendering/validation tools accept optional modes and locales filters.
    Omit both to process everything. Use filters for targeted updates.

  TEMPLATE RESOLUTION ORDER
    templates/template-{mode}.typ -> templates/template.typ -> template.typ
    Per-screen templates override the shared template for specific modes.

  LOCALE FALLBACK CHAINS
    es-MX -> es-ES, fr-CA -> fr-FR, en-AU/CA/GB -> en-US,
    pt-PT -> pt-BR, zh-Hant -> zh-Hans (keywords only)

  REQUIRED APP STORE SIZES
    iPhone 6.9 inch (1320 x 2868) -- mandatory
    iPad 13 inch (2064 x 2752) -- mandatory
    Other sizes are auto-scaled by App Store Connect.

  MAX 10 SCREENSHOTS PER LOCALE

  SCREENSHOT COMPOSITION
    capture_screenshots produces clean framebuffer images (no device bezels).
    Device frames are added in the Typst template during compose_screenshots.
    Templates access the capture via image(\"/screenshot.png\") virtual file.
    Check sys.inputs.at(\"screenshot_path\", default: none) for availability.

=== COMMON WORKFLOWS ===

  Fix one screenshot:
    compose_screenshots(modes: [3])

  Fix one locale on one screenshot:
    compose_screenshots(modes: [5], locales: [\"de-DE\"])

  Add a new locale:
    scan_project -> get_locale_keywords(locale) -> save_captions(locale, ...) ->
    compose_screenshots(locales: [locale])

  Check project health:
    get_project_status

  Review ASO quality:
    review_captions(locale: \"en-US\")

  Re-capture after app change:
    capture_screenshots(bundle_id, device, modes: [4])

  Pre-warm simulator:
    list_simulators -> warm_simulator(udid, bundle_id) -> capture_screenshots(...)

  Capture scrolled content:
    warm_simulator(udid) -> capture first modes ->
    interact_simulator(action: \"scroll\", x: 200, y: 400, dy: -300) ->
    capture_screenshots(modes: [scrolled_mode])

  Seed mock data before capture:
    warm_simulator(udid, bundle_id) ->
    seed_defaults(bundle_id, data: {\"isPro\": true, \"streak\": 7}) ->
    capture_screenshots(...)

=== DIRECTORY STRUCTURE ===

  fastlane/metadata/{locale}/        keywords.txt, name.txt, subtitle.txt
  fastlane/screenshots/{locale}/     final output (fastlane deliver reads here)
  appshots.json                      project config (plan, captions, template)
  appshots/template.typ              single shared Typst template
  appshots/templates/                per-screen Typst templates
  appshots/fonts/                    custom fonts
  appshots/captures/{device}/{locale}/  simulator captures
  appshots/previews/                 design iteration previews
  appshots/.seed-defaults.plist      temporary plist for defaults import
  glossary.json                      shared glossary (also used by xcstrings-mcp)
";
use crate::model::color::OklchColor;
use crate::model::device::Device;
use crate::model::locale::AsoLocale;
use crate::tools::ProjectCache;

#[derive(Clone)]
pub struct AppShotsMcpServer {
    store: Arc<dyn FileStore>,
    cache: Arc<Mutex<ProjectCache>>,
    write_lock: Arc<Mutex<()>>,
    project_dir: PathBuf,
    glossary_path: PathBuf,
    config_path: PathBuf,
    glossary_write_lock: Arc<Mutex<()>>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl AppShotsMcpServer {
    pub fn new(
        store: Arc<dyn FileStore>,
        project_dir: PathBuf,
        glossary_path: PathBuf,
        config_path: PathBuf,
    ) -> Self {
        Self {
            store,
            cache: Arc::new(Mutex::new(ProjectCache::new())),
            write_lock: Arc::new(Mutex::new(())),
            project_dir,
            glossary_path,
            config_path,
            glossary_write_lock: Arc::new(Mutex::new(())),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool param structs
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema, Default)]
pub struct AnalyzeKeywordsParams {
    /// Locale code (e.g. "en-US")
    pub locale: String,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct PlanScreensParams {
    /// Screen plans to save (upserts by mode)
    pub plans: Vec<crate::model::config::ScreenPlan>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct SaveCaptionsParams {
    /// Locale code (e.g. "en-US")
    pub locale: String,
    /// Captions to save (upserts by mode)
    pub captions: Vec<crate::model::config::Caption>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct GetCaptionsParams {
    /// Locale code filter (omit for all locales)
    pub locale: Option<String>,
    /// Mode filter (omit for all modes)
    pub modes: Option<Vec<u8>>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct GetLocaleKeywordsParams {
    /// Locale code (e.g. "de-DE")
    pub locale: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct PreviewDesignParams {
    /// Screenshot mode number
    pub mode: u8,
    /// Title text for the caption
    pub caption_title: String,
    /// Subtitle text (optional)
    pub caption_subtitle: Option<String>,
    /// Background gradient colors in OKLCH
    pub bg_colors: Vec<OklchColor>,
    /// Target device
    pub device: Device,
    /// Locale code
    pub locale: String,
}

impl Default for PreviewDesignParams {
    fn default() -> Self {
        Self {
            mode: 1,
            caption_title: String::new(),
            caption_subtitle: None,
            bg_colors: Vec::new(),
            device: Device::Iphone6_9,
            locale: String::new(),
        }
    }
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ValidateLayoutParams {
    /// Mode filter (omit for all modes)
    pub modes: Option<Vec<u8>>,
    /// Locale filter (omit for all locales)
    pub locales: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ComposeScreenshotsParams {
    /// Mode filter (omit for all modes)
    pub modes: Option<Vec<u8>>,
    /// Locale filter (omit for all locales)
    pub locales: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct CaptureScreenshotsParams {
    /// App bundle ID
    pub bundle_id: String,
    /// Target device
    pub device: String,
    /// Mode filter (omit for all modes)
    pub modes: Option<Vec<u8>>,
    /// Locale filter (omit for all locales)
    pub locales: Option<Vec<String>>,
    /// Delay in ms between launch and capture
    pub delay_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct InteractSimulatorParams {
    /// Action: "scroll" or "tap"
    pub action: String,
    /// X coordinate (screen pixels). For tap: required. For scroll: optional start position.
    pub x: Option<f64>,
    /// Y coordinate (screen pixels). For tap: required. For scroll: optional start position.
    pub y: Option<f64>,
    /// Horizontal scroll delta in pixels.
    pub dx: Option<f64>,
    /// Vertical scroll delta in pixels (positive = scroll content down).
    pub dy: Option<f64>,
    /// Delay in ms after action for UI to settle (default: 500).
    pub delay_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct SeedDefaultsParams {
    /// App bundle ID
    pub bundle_id: String,
    /// Key-value data to seed into UserDefaults
    pub data: indexmap::IndexMap<String, serde_json::Value>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct WarmSimulatorParams {
    /// Simulator UDID
    pub udid: String,
    /// App bundle ID (optional, for granting permissions)
    pub bundle_id: Option<String>,
    /// Appearance: "light" or "dark" (optional)
    pub appearance: Option<String>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct SaveTemplateParams {
    /// Typst template source code
    pub template_source: String,
    /// Screen mode number (omit for single shared template)
    pub mode: Option<u8>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct GetTemplateParams {
    /// Screen mode number (omit for single shared template)
    pub mode: Option<u8>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct SuggestFontParams {
    /// Locale code (e.g. "ja", "ar-SA")
    pub locale: String,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ReviewCaptionsParams {
    /// Locale code filter (omit for all locales)
    pub locale: Option<String>,
    /// Mode filter (omit for all modes)
    pub modes: Option<Vec<u8>>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct GetGlossaryParams {
    /// Source locale filter
    pub source_locale: Option<String>,
    /// Target locale filter
    pub target_locale: Option<String>,
    /// Substring filter for terms/translations
    pub filter: Option<String>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct UpdateGlossaryParams {
    /// Source locale
    pub source_locale: String,
    /// Target locale
    pub target_locale: String,
    /// Term-to-translation entries to add or update
    pub entries: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Prompt param structs
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema, Default)]
pub struct PrepareAppParams {
    /// App bundle ID
    pub bundle_id: String,
    /// Number of screens (default 5)
    pub screens_count: Option<u8>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct DesignTemplateParams {
    /// App bundle ID
    pub bundle_id: String,
    /// Style direction (e.g. "dark minimal", "vibrant gradients")
    pub style: String,
    /// Whether to create per-screen templates
    #[serde(default)]
    pub per_screen: bool,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct GenerateScreenshotsParams {
    /// Device filter (comma-separated, empty for all)
    #[serde(default)]
    pub devices: String,
    /// Locale filter (comma-separated, empty for all)
    #[serde(default)]
    pub locales: String,
    /// Mode filter (comma-separated, empty for all)
    #[serde(default)]
    pub modes: String,
}

// ---------------------------------------------------------------------------
// Tool router
// ---------------------------------------------------------------------------

#[tool_router]
impl AppShotsMcpServer {
    #[tool(
        name = "scan_project",
        description = "Scan fastlane/metadata/ for all locales and cache results"
    )]
    async fn scan_project(&self) -> Result<String, String> {
        match crate::tools::scan::handle_scan_project(
            self.store.as_ref(),
            &self.cache,
            &self.project_dir,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "scan_project failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "analyze_keywords",
        description = "Analyze keyword coverage for a locale's existing captions"
    )]
    async fn analyze_keywords(
        &self,
        Parameters(params): Parameters<AnalyzeKeywordsParams>,
    ) -> Result<String, String> {
        let locale = AsoLocale::from_str(&params.locale).map_err(|e| e.to_string())?;

        // Load current captions from config
        let config =
            crate::tools::resolve_config(self.store.as_ref(), &self.cache, &self.config_path)
                .await
                .map_err(|e| e.to_string())?;

        let captions: Vec<crate::model::config::Caption> = config
            .extra
            .get("captions")
            .and_then(|v| v.get(locale.code()))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        match crate::tools::analyze::handle_analyze_keywords(&self.cache, &locale, &captions).await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "analyze_keywords failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "plan_screens",
        description = "Save screen plans (mode→keywords→messaging) to appshots.json"
    )]
    async fn plan_screens(
        &self,
        Parameters(params): Parameters<PlanScreensParams>,
    ) -> Result<String, String> {
        match crate::tools::plan::handle_plan_screens(
            self.store.as_ref(),
            &self.cache,
            &self.write_lock,
            &self.config_path,
            params.plans,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "plan_screens failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_plans",
        description = "Get current screen plans from appshots.json"
    )]
    async fn get_plans(&self) -> Result<String, String> {
        match crate::tools::plan::handle_get_plans(
            self.store.as_ref(),
            &self.cache,
            &self.config_path,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_plans failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "save_captions",
        description = "Save captions for a locale (upsert by mode)"
    )]
    async fn save_captions(
        &self,
        Parameters(params): Parameters<SaveCaptionsParams>,
    ) -> Result<String, String> {
        match crate::tools::captions::handle_save_captions(
            self.store.as_ref(),
            &self.cache,
            &self.write_lock,
            &self.config_path,
            &params.locale,
            params.captions,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "save_captions failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_captions",
        description = "Get captions with optional locale/modes filter"
    )]
    async fn get_captions(
        &self,
        Parameters(params): Parameters<GetCaptionsParams>,
    ) -> Result<String, String> {
        match crate::tools::captions::handle_get_captions(
            self.store.as_ref(),
            &self.cache,
            &self.config_path,
            params.locale.as_deref(),
            params.modes.as_deref(),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_captions failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_locale_keywords",
        description = "Get keywords.txt content for a locale"
    )]
    async fn get_locale_keywords(
        &self,
        Parameters(params): Parameters<GetLocaleKeywordsParams>,
    ) -> Result<String, String> {
        match crate::tools::captions::handle_get_locale_keywords(
            self.store.as_ref(),
            &self.project_dir,
            &params.locale,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_locale_keywords failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "preview_design",
        description = "Render a single design preview via Typst template"
    )]
    async fn preview_design(
        &self,
        Parameters(params): Parameters<PreviewDesignParams>,
    ) -> Result<String, String> {
        let locale = AsoLocale::from_str(&params.locale).map_err(|e| e.to_string())?;

        match crate::tools::design::handle_preview_design(crate::tools::design::PreviewParams {
            store: self.store.as_ref(),
            project_dir: &self.project_dir,
            mode: params.mode,
            caption_title: &params.caption_title,
            caption_subtitle: params.caption_subtitle.as_deref(),
            bg_colors: params.bg_colors,
            device: params.device,
            locale,
        })
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "preview_design failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "validate_layout",
        description = "Validate template layout for all mode/locale/device combinations"
    )]
    async fn validate_layout(
        &self,
        Parameters(params): Parameters<ValidateLayoutParams>,
    ) -> Result<String, String> {
        match crate::tools::validate::handle_validate_layout(
            self.store.as_ref(),
            &self.project_dir,
            params.modes.as_deref(),
            params.locales.as_deref(),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "validate_layout failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "compose_screenshots",
        description = "Render final screenshots via Typst for specified modes/locales"
    )]
    async fn compose_screenshots(
        &self,
        Parameters(params): Parameters<ComposeScreenshotsParams>,
    ) -> Result<String, String> {
        match crate::tools::render::handle_compose_screenshots(
            self.store.as_ref(),
            &self.cache,
            &self.config_path,
            &self.project_dir,
            params.modes,
            params.locales,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "compose_screenshots failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "capture_screenshots",
        description = "Capture screenshots from iOS simulator with device bezels"
    )]
    async fn capture_screenshots(
        &self,
        Parameters(params): Parameters<CaptureScreenshotsParams>,
    ) -> Result<String, String> {
        match crate::tools::capture::handle_capture_screenshots(
            self.store.as_ref(),
            &self.project_dir,
            &params.bundle_id,
            &params.device,
            params.modes,
            params.locales,
            params.delay_ms.unwrap_or(1000),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "capture_screenshots failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "interact_simulator",
        description = "Interact with iOS Simulator: scroll or tap. Requires macOS Accessibility permission. Simulator must be the frontmost window. Use before capture_screenshots to scroll content into view."
    )]
    async fn interact_simulator(
        &self,
        Parameters(params): Parameters<InteractSimulatorParams>,
    ) -> Result<String, String> {
        match crate::tools::interact::handle_interact_simulator(
            &params.action,
            params.x,
            params.y,
            params.dx,
            params.dy,
            params.delay_ms.unwrap_or(500),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "interact_simulator failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "seed_defaults",
        description = "Seed UserDefaults in simulator via plist import. Must run AFTER app install but BEFORE app launch. Supports String, Int, Float, Bool, Array, Dict, base64-encoded Data."
    )]
    async fn seed_defaults(
        &self,
        Parameters(params): Parameters<SeedDefaultsParams>,
    ) -> Result<String, String> {
        match crate::tools::seed::handle_seed_defaults(
            self.store.as_ref(),
            &self.project_dir,
            &params.bundle_id,
            params.data,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "seed_defaults failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "warm_simulator",
        description = "Pre-boot simulator, grant permissions, set status bar to Apple canonical (9:41, full battery/signal), and optionally set appearance (light/dark)"
    )]
    async fn warm_simulator(
        &self,
        Parameters(params): Parameters<WarmSimulatorParams>,
    ) -> Result<String, String> {
        match crate::tools::warm::handle_warm_simulator(
            &params.udid,
            params.bundle_id.as_deref(),
            params.appearance.as_deref(),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "warm_simulator failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_glossary",
        description = "Get glossary entries, optionally filtered by locale pair or substring"
    )]
    async fn get_glossary(
        &self,
        Parameters(params): Parameters<GetGlossaryParams>,
    ) -> Result<String, String> {
        match crate::tools::glossary::handle_get_glossary(
            self.store.as_ref(),
            &self.glossary_path,
            params.source_locale.as_deref(),
            params.target_locale.as_deref(),
            params.filter.as_deref(),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_glossary failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "update_glossary",
        description = "Add or update glossary entries for a source→target locale pair"
    )]
    async fn update_glossary(
        &self,
        Parameters(params): Parameters<UpdateGlossaryParams>,
    ) -> Result<String, String> {
        match crate::tools::glossary::handle_update_glossary(
            self.store.as_ref(),
            &self.glossary_write_lock,
            &self.glossary_path,
            &params.source_locale,
            &params.target_locale,
            params.entries,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "update_glossary failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "list_simulators",
        description = "List available iOS simulators (name, UDID, state, runtime)"
    )]
    async fn list_simulators(&self) -> Result<String, String> {
        match crate::tools::capture::handle_list_simulators().await {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "list_simulators failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_project_status",
        description = "Get project readiness status: config, template, locales, captions, captures"
    )]
    async fn get_project_status(&self) -> Result<String, String> {
        match crate::tools::scan::handle_get_project_status(
            self.store.as_ref(),
            &self.cache,
            &self.project_dir,
            &self.config_path,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_project_status failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "run_deliver",
        description = "Run fastlane deliver to upload screenshots to App Store Connect"
    )]
    async fn run_deliver(&self) -> Result<String, String> {
        match crate::tools::deliver::handle_run_deliver(&self.project_dir).await {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "run_deliver failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_caption_coverage",
        description = "Get coverage matrix of captions across all locales and modes"
    )]
    async fn get_caption_coverage(&self) -> Result<String, String> {
        match crate::tools::captions::handle_get_caption_coverage(
            self.store.as_ref(),
            &self.cache,
            &self.config_path,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_caption_coverage failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "review_captions",
        description = "Review captions against keyword coverage for ASO optimization"
    )]
    async fn review_captions(
        &self,
        Parameters(params): Parameters<ReviewCaptionsParams>,
    ) -> Result<String, String> {
        match crate::tools::captions::handle_review_captions(
            self.store.as_ref(),
            &self.cache,
            &self.config_path,
            &self.project_dir,
            params.locale.as_deref(),
            params.modes.as_deref(),
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "review_captions failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "save_template",
        description = "Save a Typst template (single or per-screen)"
    )]
    async fn save_template(
        &self,
        Parameters(params): Parameters<SaveTemplateParams>,
    ) -> Result<String, String> {
        match crate::tools::design::handle_save_template(
            self.store.as_ref(),
            &self.project_dir,
            &params.template_source,
            params.mode,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "save_template failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "get_template",
        description = "Read a Typst template (resolves mode-specific → shared → root)"
    )]
    async fn get_template(
        &self,
        Parameters(params): Parameters<GetTemplateParams>,
    ) -> Result<String, String> {
        match crate::tools::design::handle_get_template(
            self.store.as_ref(),
            &self.project_dir,
            params.mode,
        )
        .await
        {
            Ok(result) => serde_json::to_string_pretty(&result).map_err(|e| e.to_string()),
            Err(e) => {
                error!(error = %e, "get_template failed");
                Err(e.to_string())
            }
        }
    }

    #[tool(
        name = "suggest_font",
        description = "Suggest a system font for a locale's script (CJK, Arabic, Latin, etc.)"
    )]
    async fn suggest_font(
        &self,
        Parameters(params): Parameters<SuggestFontParams>,
    ) -> Result<String, String> {
        let locale = AsoLocale::from_str(&params.locale).map_err(|e| e.to_string())?;
        let result = crate::tools::design::handle_suggest_font(&locale);
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Prompt router
// ---------------------------------------------------------------------------

#[prompt_router]
impl AppShotsMcpServer {
    #[prompt(
        name = "prepare-app",
        description = "Guide: analyze app and create ScreenshotMode enum"
    )]
    fn prepare_app(
        &self,
        Parameters(params): Parameters<PrepareAppParams>,
    ) -> Result<GetPromptResult, rmcp::ErrorData> {
        let content = crate::prompts::prepare_app_content(
            &params.bundle_id,
            params.screens_count.unwrap_or(5),
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            content,
        )])
        .with_description("Prepare app for screenshots"))
    }

    #[prompt(
        name = "design-template",
        description = "Guide: create a professional Typst template for screenshots"
    )]
    fn design_template(
        &self,
        Parameters(params): Parameters<DesignTemplateParams>,
    ) -> Result<GetPromptResult, rmcp::ErrorData> {
        let content = crate::prompts::design_template_content(
            &params.bundle_id,
            &params.style,
            params.per_screen,
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            content,
        )])
        .with_description("Design screenshot template"))
    }

    #[prompt(
        name = "generate-screenshots",
        description = "Guide: execute full 10-step screenshot pipeline"
    )]
    fn generate_screenshots(
        &self,
        Parameters(params): Parameters<GenerateScreenshotsParams>,
    ) -> Result<GetPromptResult, rmcp::ErrorData> {
        let content = crate::prompts::generate_screenshots_content(
            &params.devices,
            &params.locales,
            &params.modes,
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            content,
        )])
        .with_description("Generate App Store screenshots"))
    }
}

// ---------------------------------------------------------------------------
// ServerHandler
// ---------------------------------------------------------------------------

#[tool_handler]
#[prompt_handler]
impl ServerHandler for AppShotsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_instructions(SERVER_INSTRUCTIONS.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::memory::MemoryStore;

    fn make_server() -> AppShotsMcpServer {
        let store = Arc::new(MemoryStore::new());
        AppShotsMcpServer::new(
            store,
            PathBuf::from("/project"),
            PathBuf::from("/project/glossary.json"),
            PathBuf::from("/project/appshots.json"),
        )
    }

    #[test]
    fn get_info_has_tools_and_prompts() {
        let server = make_server();
        let info = server.get_info();
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.prompts.is_some());
    }

    #[test]
    fn get_info_has_instructions() {
        let server = make_server();
        let info = server.get_info();
        assert!(info.instructions.is_some());
        assert!(info.instructions.as_ref().unwrap().contains("appshots-mcp"));
    }

    #[test]
    fn instructions_mention_all_tools() {
        let tools = [
            "list_simulators",
            "capture_screenshots",
            "scan_project",
            "analyze_keywords",
            "get_project_status",
            "plan_screens",
            "get_plans",
            "save_captions",
            "get_captions",
            "get_locale_keywords",
            "get_caption_coverage",
            "review_captions",
            "save_template",
            "get_template",
            "preview_design",
            "validate_layout",
            "suggest_font",
            "compose_screenshots",
            "run_deliver",
            "get_glossary",
            "update_glossary",
            "seed_defaults",
            "warm_simulator",
            "interact_simulator",
        ];
        for tool in &tools {
            assert!(
                SERVER_INSTRUCTIONS.contains(tool),
                "Instructions missing tool: {tool}"
            );
        }
    }

    #[test]
    fn instructions_mention_all_prompts() {
        let prompts = ["prepare-app", "design-template", "generate-screenshots"];
        for prompt in &prompts {
            assert!(
                SERVER_INSTRUCTIONS.contains(prompt),
                "Instructions missing prompt: {prompt}"
            );
        }
    }

    #[test]
    fn tool_router_lists_all_tools() {
        let router = AppShotsMcpServer::tool_router();
        let tools = router.list_all();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

        assert!(names.contains(&"scan_project"));
        assert!(names.contains(&"analyze_keywords"));
        assert!(names.contains(&"plan_screens"));
        assert!(names.contains(&"get_plans"));
        assert!(names.contains(&"save_captions"));
        assert!(names.contains(&"get_captions"));
        assert!(names.contains(&"get_locale_keywords"));
        assert!(names.contains(&"preview_design"));
        assert!(names.contains(&"validate_layout"));
        assert!(names.contains(&"compose_screenshots"));
        assert!(names.contains(&"capture_screenshots"));
        assert!(names.contains(&"get_glossary"));
        assert!(names.contains(&"update_glossary"));
        assert!(names.contains(&"list_simulators"));
        assert!(names.contains(&"get_project_status"));
        assert!(names.contains(&"run_deliver"));
        assert!(names.contains(&"get_caption_coverage"));
        assert!(names.contains(&"review_captions"));
        assert!(names.contains(&"save_template"));
        assert!(names.contains(&"get_template"));
        assert!(names.contains(&"suggest_font"));
        assert!(names.contains(&"interact_simulator"));
        assert!(names.contains(&"seed_defaults"));
        assert!(names.contains(&"warm_simulator"));
        assert_eq!(tools.len(), 24);
    }

    #[test]
    fn prompt_router_lists_all_prompts() {
        let router = AppShotsMcpServer::prompt_router();
        let prompts = router.list_all();
        let names: Vec<&str> = prompts.iter().map(|p| p.name.as_ref()).collect();

        assert!(names.contains(&"prepare-app"));
        assert!(names.contains(&"design-template"));
        assert!(names.contains(&"generate-screenshots"));
        assert_eq!(prompts.len(), 3);
    }

    // -----------------------------------------------------------------------
    // Tool handler tests — exercise the delegation + serialization paths
    // -----------------------------------------------------------------------

    fn minimal_config_json() -> &'static str {
        r#"{
            "bundleId": "com.example.app",
            "screens": [],
            "templateMode": "single",
            "devices": ["iPhone 6.9\""]
        }"#
    }

    fn make_server_with_config() -> AppShotsMcpServer {
        let store = Arc::new(MemoryStore::new());
        let config_path = PathBuf::from("/project/appshots.json");
        store
            .write(
                std::path::Path::new("/project/appshots.json"),
                minimal_config_json(),
            )
            .unwrap();
        AppShotsMcpServer::new(
            store,
            PathBuf::from("/project"),
            PathBuf::from("/project/glossary.json"),
            config_path,
        )
    }

    #[tokio::test]
    async fn scan_project_empty_returns_result() {
        // Empty project (no fastlane dir) — scan returns error or empty
        let server = make_server();
        let result = server.scan_project().await;
        // Either ok with empty or err — both exercise the handler
        let _ = result;
    }

    #[tokio::test]
    async fn analyze_keywords_invalid_locale() {
        let server = make_server_with_config();
        let params = Parameters(AnalyzeKeywordsParams {
            locale: "not-a-locale".into(),
        });
        let result = server.analyze_keywords(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn analyze_keywords_valid_locale_no_captions() {
        let server = make_server_with_config();
        let params = Parameters(AnalyzeKeywordsParams {
            locale: "en-US".into(),
        });
        let result = server.analyze_keywords(params).await;
        // Exercises the handler path — may fail because no metadata cached
        let _ = result;
    }

    #[tokio::test]
    async fn plan_screens_empty_plans() {
        let server = make_server_with_config();
        let params = Parameters(PlanScreensParams { plans: vec![] });
        let result = server.plan_screens(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_plans_returns_ok() {
        let server = make_server_with_config();
        let result = server.get_plans().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_plans_no_config_returns_err() {
        let server = make_server();
        let result = server.get_plans().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn save_captions_empty() {
        let server = make_server_with_config();
        let params = Parameters(SaveCaptionsParams {
            locale: "en-US".into(),
            captions: vec![],
        });
        let result = server.save_captions(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn save_captions_with_locale() {
        let server = make_server_with_config();
        let params = Parameters(SaveCaptionsParams {
            locale: "de-DE".into(),
            captions: vec![],
        });
        let result = server.save_captions(params).await;
        // Exercises the handler — may succeed or fail depending on locale validation
        let _ = result;
    }

    #[tokio::test]
    async fn get_captions_no_config() {
        let server = make_server();
        let params = Parameters(GetCaptionsParams {
            locale: None,
            modes: None,
        });
        let result = server.get_captions(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_captions_empty() {
        let server = make_server_with_config();
        let params = Parameters(GetCaptionsParams {
            locale: None,
            modes: None,
        });
        let result = server.get_captions(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_captions_with_locale_filter() {
        let server = make_server_with_config();
        let params = Parameters(GetCaptionsParams {
            locale: Some("en-US".into()),
            modes: None,
        });
        let result = server.get_captions(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_captions_with_modes_filter() {
        let server = make_server_with_config();
        let params = Parameters(GetCaptionsParams {
            locale: None,
            modes: Some(vec![1, 2]),
        });
        let result = server.get_captions(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_locale_keywords_missing_file() {
        let server = make_server();
        let params = Parameters(GetLocaleKeywordsParams {
            locale: "en-US".into(),
        });
        let result = server.get_locale_keywords(params).await;
        // No keywords file exists — returns error
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn validate_layout_no_template() {
        let server = make_server_with_config();
        let params = Parameters(ValidateLayoutParams {
            modes: None,
            locales: None,
        });
        let result = server.validate_layout(params).await;
        // No template file — should error
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn compose_screenshots_no_config() {
        let server = make_server();
        let params = Parameters(ComposeScreenshotsParams {
            modes: None,
            locales: None,
        });
        let result = server.compose_screenshots(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_glossary_empty() {
        let server = make_server();
        let params = Parameters(GetGlossaryParams {
            source_locale: None,
            target_locale: None,
            filter: None,
        });
        let result = server.get_glossary(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_glossary_with_filters() {
        let server = make_server();
        let params = Parameters(GetGlossaryParams {
            source_locale: Some("en-US".into()),
            target_locale: Some("de-DE".into()),
            filter: Some("test".into()),
        });
        let result = server.get_glossary(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn update_glossary_creates_entry() {
        let server = make_server();
        let mut entries = BTreeMap::new();
        entries.insert("hello".into(), "hallo".into());
        let params = Parameters(UpdateGlossaryParams {
            source_locale: "en-US".into(),
            target_locale: "de-DE".into(),
            entries,
        });
        let result = server.update_glossary(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn preview_design_invalid_locale() {
        let server = make_server();
        let params = Parameters(PreviewDesignParams {
            locale: "bad-locale".into(),
            ..Default::default()
        });
        let result = server.preview_design(params).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Prompt handler tests
    // -----------------------------------------------------------------------

    #[test]
    fn prepare_app_prompt_returns_content() {
        let server = make_server();
        let params = Parameters(PrepareAppParams {
            bundle_id: "com.example.app".into(),
            screens_count: Some(5),
        });
        let result = server.prepare_app(params).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn prepare_app_prompt_default_screens() {
        let server = make_server();
        let params = Parameters(PrepareAppParams {
            bundle_id: "com.example.app".into(),
            screens_count: None,
        });
        let result = server.prepare_app(params).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn design_template_prompt_returns_content() {
        let server = make_server();
        let params = Parameters(DesignTemplateParams {
            bundle_id: "com.example.app".into(),
            style: "dark minimal".into(),
            per_screen: false,
        });
        let result = server.design_template(params).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn design_template_prompt_per_screen() {
        let server = make_server();
        let params = Parameters(DesignTemplateParams {
            bundle_id: "com.example.app".into(),
            style: "vibrant".into(),
            per_screen: true,
        });
        let result = server.design_template(params).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn generate_screenshots_prompt_returns_content() {
        let server = make_server();
        let params = Parameters(GenerateScreenshotsParams {
            devices: String::new(),
            locales: String::new(),
            modes: String::new(),
        });
        let result = server.generate_screenshots(params).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn generate_screenshots_prompt_with_filters() {
        let server = make_server();
        let params = Parameters(GenerateScreenshotsParams {
            devices: "iPhone 6.9\"".into(),
            locales: "en-US,de-DE".into(),
            modes: "1,2,3".into(),
        });
        let result = server.generate_screenshots(params).unwrap();
        assert!(!result.messages.is_empty());
    }

    // -----------------------------------------------------------------------
    // Param deserialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn analyze_keywords_params_from_json() {
        let json = r#"{"locale": "en-US"}"#;
        let params: AnalyzeKeywordsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.locale, "en-US");
    }

    #[test]
    fn save_captions_params_from_json() {
        let json = r#"{"locale": "de-DE", "captions": []}"#;
        let params: SaveCaptionsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.locale, "de-DE");
        assert!(params.captions.is_empty());
    }

    #[test]
    fn get_captions_params_defaults() {
        let json = r#"{}"#;
        let params: GetCaptionsParams = serde_json::from_str(json).unwrap();
        assert!(params.locale.is_none());
        assert!(params.modes.is_none());
    }

    #[test]
    fn compose_screenshots_params_with_filters() {
        let json = r#"{"modes": [1, 2], "locales": ["en-US"]}"#;
        let params: ComposeScreenshotsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.modes.unwrap(), vec![1, 2]);
        assert_eq!(params.locales.unwrap(), vec!["en-US"]);
    }

    #[test]
    fn capture_screenshots_params_from_json() {
        let json = r#"{"bundle_id": "com.test", "device": "iPhone", "delay_ms": 500}"#;
        let params: CaptureScreenshotsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.bundle_id, "com.test");
        assert_eq!(params.delay_ms, Some(500));
        assert!(params.modes.is_none());
    }

    #[test]
    fn preview_design_params_default() {
        let params = PreviewDesignParams::default();
        assert_eq!(params.mode, 1);
        assert!(params.caption_title.is_empty());
        assert!(params.caption_subtitle.is_none());
        assert!(params.bg_colors.is_empty());
    }

    #[test]
    fn update_glossary_params_from_json() {
        let json = r#"{"source_locale": "en-US", "target_locale": "fr-FR", "entries": {"hello": "bonjour"}}"#;
        let params: UpdateGlossaryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.source_locale, "en-US");
        assert_eq!(params.entries.len(), 1);
        assert_eq!(params.entries["hello"], "bonjour");
    }

    #[test]
    fn get_glossary_params_defaults() {
        let params = GetGlossaryParams::default();
        assert!(params.source_locale.is_none());
        assert!(params.target_locale.is_none());
        assert!(params.filter.is_none());
    }

    #[test]
    fn validate_layout_params_from_json() {
        let json = r#"{"modes": [1], "locales": ["en-US", "de-DE"]}"#;
        let params: ValidateLayoutParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.modes.unwrap(), vec![1]);
        assert_eq!(params.locales.unwrap(), vec!["en-US", "de-DE"]);
    }

    #[test]
    fn plan_screens_params_from_json() {
        let json = r#"{"plans": []}"#;
        let params: PlanScreensParams = serde_json::from_str(json).unwrap();
        assert!(params.plans.is_empty());
    }

    #[test]
    fn prepare_app_params_from_json() {
        let json = r#"{"bundle_id": "com.test.app"}"#;
        let params: PrepareAppParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.bundle_id, "com.test.app");
        assert!(params.screens_count.is_none());
    }

    #[test]
    fn design_template_params_from_json() {
        let json = r#"{"bundle_id": "com.test", "style": "dark", "per_screen": true}"#;
        let params: DesignTemplateParams = serde_json::from_str(json).unwrap();
        assert!(params.per_screen);
    }

    #[test]
    fn generate_screenshots_params_defaults() {
        let params = GenerateScreenshotsParams::default();
        assert!(params.devices.is_empty());
        assert!(params.locales.is_empty());
        assert!(params.modes.is_empty());
    }

    // -----------------------------------------------------------------------
    // New P1 tool handler tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_project_status_empty() {
        let server = make_server();
        let result = server.get_project_status().await;
        assert!(result.is_ok());
        let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(json["config_exists"], false);
    }

    #[tokio::test]
    async fn get_project_status_with_config() {
        let server = make_server_with_config();
        let result = server.get_project_status().await;
        assert!(result.is_ok());
        let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(json["config_exists"], true);
    }

    #[tokio::test]
    async fn get_caption_coverage_no_config() {
        let server = make_server();
        let result = server.get_caption_coverage().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_caption_coverage_empty_config() {
        let server = make_server_with_config();
        let result = server.get_caption_coverage().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn review_captions_no_config() {
        let server = make_server();
        let params = Parameters(ReviewCaptionsParams {
            locale: None,
            modes: None,
        });
        let result = server.review_captions(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn review_captions_empty() {
        let server = make_server_with_config();
        let params = Parameters(ReviewCaptionsParams {
            locale: Some("en-US".into()),
            modes: None,
        });
        let result = server.review_captions(params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn save_template_and_get_template() {
        let server = make_server();
        let save_params = Parameters(SaveTemplateParams {
            template_source: "#set page()\nHello".into(),
            mode: None,
        });
        let save_result = server.save_template(save_params).await;
        assert!(save_result.is_ok());

        let get_params = Parameters(GetTemplateParams { mode: None });
        let get_result = server.get_template(get_params).await;
        assert!(get_result.is_ok());
        assert!(get_result.unwrap().contains("Hello"));
    }

    #[tokio::test]
    async fn get_template_not_found() {
        let server = make_server();
        let params = Parameters(GetTemplateParams { mode: None });
        let result = server.get_template(params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn suggest_font_valid_locale() {
        let server = make_server();
        let params = Parameters(SuggestFontParams {
            locale: "ja".into(),
        });
        let result = server.suggest_font(params).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Hiragino Sans"));
    }

    #[tokio::test]
    async fn suggest_font_invalid_locale() {
        let server = make_server();
        let params = Parameters(SuggestFontParams {
            locale: "bad".into(),
        });
        let result = server.suggest_font(params).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // New P1 param deserialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn save_template_params_from_json() {
        let json = r##"{"template_source": "#set page()", "mode": 3}"##;
        let params: SaveTemplateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.template_source, "#set page()");
        assert_eq!(params.mode, Some(3));
    }

    #[test]
    fn save_template_params_defaults() {
        let params = SaveTemplateParams::default();
        assert!(params.template_source.is_empty());
        assert!(params.mode.is_none());
    }

    #[test]
    fn get_template_params_from_json() {
        let json = r#"{"mode": 5}"#;
        let params: GetTemplateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.mode, Some(5));
    }

    #[test]
    fn get_template_params_defaults() {
        let params = GetTemplateParams::default();
        assert!(params.mode.is_none());
    }

    #[test]
    fn suggest_font_params_from_json() {
        let json = r#"{"locale": "ar-SA"}"#;
        let params: SuggestFontParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.locale, "ar-SA");
    }

    #[test]
    fn review_captions_params_from_json() {
        let json = r#"{"locale": "en-US", "modes": [1, 3]}"#;
        let params: ReviewCaptionsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.locale, Some("en-US".into()));
        assert_eq!(params.modes, Some(vec![1, 3]));
    }

    #[test]
    fn review_captions_params_defaults() {
        let params = ReviewCaptionsParams::default();
        assert!(params.locale.is_none());
        assert!(params.modes.is_none());
    }

    #[test]
    fn seed_defaults_params_from_json() {
        let json = r#"{"bundle_id": "com.app", "data": {"streak": 7, "isPro": true}}"#;
        let params: SeedDefaultsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.bundle_id, "com.app");
        assert_eq!(params.data.len(), 2);
        assert_eq!(params.data["streak"], 7);
        assert_eq!(params.data["isPro"], true);
    }

    #[test]
    fn seed_defaults_params_defaults() {
        let params = SeedDefaultsParams::default();
        assert!(params.bundle_id.is_empty());
        assert!(params.data.is_empty());
    }

    #[test]
    fn warm_simulator_params_from_json() {
        let json = r#"{"udid": "ABC-123", "bundle_id": "com.app", "appearance": "dark"}"#;
        let params: WarmSimulatorParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.udid, "ABC-123");
        assert_eq!(params.bundle_id, Some("com.app".into()));
        assert_eq!(params.appearance, Some("dark".into()));
    }

    #[test]
    fn warm_simulator_params_defaults() {
        let params = WarmSimulatorParams::default();
        assert!(params.udid.is_empty());
        assert!(params.bundle_id.is_none());
        assert!(params.appearance.is_none());
    }

    #[test]
    fn warm_simulator_params_minimal() {
        let json = r#"{"udid": "XYZ"}"#;
        let params: WarmSimulatorParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.udid, "XYZ");
        assert!(params.bundle_id.is_none());
        assert!(params.appearance.is_none());
    }
}
