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
        assert_eq!(tools.len(), 13);
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
}
