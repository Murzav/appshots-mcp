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
        .with_instructions(
            "appshots-mcp: MCP server for generating ASO-optimized App Store screenshots. \
             Generates up to 780 final images per app (39 locales x 5-10 screenshots x 1-2 devices). \
             Use the prepare-app prompt to start, then design-template, then generate-screenshots."
                .to_owned(),
        )
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
        assert!(info.instructions.unwrap().contains("appshots-mcp"));
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
        assert_eq!(tools.len(), 21);
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
}
