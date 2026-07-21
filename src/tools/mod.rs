// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod ask;
pub mod background;
pub mod backup_tool;
pub mod brief;
pub mod browser;
pub mod claude_code;
#[cfg(feature = "tool-cloud-ops")]
pub mod cloud;
pub mod code;
#[cfg(feature = "tool-cron")]
pub mod cron;
pub mod file;
pub mod flow;
pub mod github;
pub mod glob;
#[cfg(feature = "hardware")]
pub mod hardware;
#[cfg(feature = "tool-image")]
pub mod image;
pub mod deck_compile;
pub mod design_system_read;
pub mod designer;
pub mod figma_fetch;
pub mod inline;
pub mod mcp;
pub mod media;
pub mod memory;
pub mod multi;
pub mod plan_mode;
pub mod read;
#[cfg(feature = "tool-reports")]
pub mod report;
#[cfg(feature = "tool-sop")]
pub mod sop;
pub mod task;
#[cfg(feature = "tool-team")]
pub mod team;
pub mod web;
pub mod worktree;
#[cfg(feature = "tool-utility-misc")]
pub mod calculator;
pub mod canvas;
pub mod cli_discovery;
pub mod codex_cli;
#[cfg(feature = "tool-productivity")]
pub mod composio;
pub mod content_search;
pub mod custom_tool;
#[cfg(feature = "tool-cloud-ops")]
pub mod data_management;
pub mod debug_test_report;
pub mod delegate;
pub mod spawn_workers;
pub mod diagnostics;
pub mod dir_list;
#[cfg(feature = "office-docs")]
pub mod document;
#[cfg(feature = "tool-search-social")]
pub mod discord_search;
pub mod edit_history;
pub mod error;
pub mod escalate;
pub mod exa_search;
pub mod fs_ops;
pub mod handler;
pub mod gemini_cli;
pub mod git_operations;
#[cfg(feature = "tool-curator")]
pub mod curator;
#[cfg(feature = "tool-productivity")]
pub mod google_workspace;
pub mod handle;
pub mod http_request;
pub mod incremental_optimize;
#[cfg(feature = "tool-productivity")]
pub mod jira_tool;
pub mod knowledge_tool;
#[cfg(feature = "tool-linkedin")]
pub mod linkedin;
pub mod autoresearch;
pub mod llm_task;
pub mod lsp;
pub mod microsoft365;
pub mod model;
pub mod node;
pub mod notebook_edit;
#[cfg(feature = "tool-productivity")]
pub mod notion_tool;
pub mod now;
pub mod opencode_cli;
pub mod diff_apply;
pub mod patch_apply;
pub mod write_plan;
pub mod pdf_read;
pub mod pipeline;
pub mod poll;
pub mod powershell;
pub mod present_files;
pub mod project_intel;
pub mod proxy_config;
#[cfg(feature = "tool-pushover")]
pub mod pushover;
pub mod reaction;
#[cfg(feature = "tool-search-social")]
pub mod reddit_search;
pub mod registry;
pub mod restore_file;
#[cfg(feature = "tool-cron")]
pub mod schedule;
pub mod schema;
#[cfg(feature = "tool-image")]
pub mod screenshot;
#[cfg(feature = "tool-cloud-ops")]
pub mod security_ops;
pub mod send_message;
pub mod sessions;
pub mod setup_agent;
pub mod shell;
pub mod skill;
pub mod sleep;
pub mod spec_cache;
pub mod structured_output;
pub mod swarm;
pub mod tavily_search;
pub mod text_browser;
pub mod todo_write;
pub mod traits;
pub mod update_plan;
pub mod verifiable_intent;
#[cfg(feature = "tool-image")]
pub mod view_image;
#[cfg(feature = "tool-utility-misc")]
pub mod weather_tool;
#[cfg(feature = "tool-workspace-deep")]
pub mod workspace_deep_search;
pub mod workspace_tool;
#[cfg(feature = "tool-search-social")]
pub mod youtube_search;

pub use ask::question::AskQuestionTool;
pub use ask::user::AskUserTool;
pub use backup_tool::BackupTool;
pub use brief::BriefTool;
pub use browser::BrowserTool;
pub use browser::delegate::{BrowserDelegateConfig, BrowserDelegateTool};
pub use browser::open::BrowserOpenTool;
#[cfg(feature = "tool-utility-misc")]
pub use calculator::CalculatorTool;
pub use canvas::CanvasStore;
#[cfg(feature = "tool-utility-misc")]
pub use canvas::CanvasTool;
pub use claude_code::core::ClaudeCodeTool;
pub use claude_code::runner::ClaudeCodeRunnerTool;
#[cfg(feature = "tool-cloud-ops")]
pub use cloud::ops::CloudOpsTool;
#[cfg(feature = "tool-cloud-ops")]
pub use cloud::patterns::CloudPatternsTool;
pub use code::to_spec::CodeToSpecTool;
pub use codex_cli::CodexCliTool;
#[cfg(feature = "tool-productivity")]
pub use composio::ComposioTool;
pub use content_search::ContentSearchTool;
#[cfg(feature = "tool-cron")]
pub use cron::add::CronAddTool;
#[cfg(feature = "tool-cron")]
pub use cron::list::CronListTool;
#[cfg(feature = "tool-cron")]
pub use cron::remove::CronRemoveTool;
#[cfg(feature = "tool-cron")]
pub use cron::run::CronRunTool;
#[cfg(feature = "tool-cron")]
pub use cron::runs::CronRunsTool;
#[cfg(feature = "tool-cron")]
pub use cron::update::CronUpdateTool;
#[cfg(feature = "tool-cloud-ops")]
pub use data_management::DataManagementTool;
pub use delegate::DelegateTool;
pub use diagnostics::DiagnosticsTool;
pub use dir_list::DirListTool;
#[cfg(feature = "office-docs")]
pub use document::DocumentConvertTool;
#[cfg(feature = "office-docs")]
pub use document::PdfOpsTool;
#[cfg(feature = "office-docs")]
pub use document::PresentationCreateTool;
pub use error::ToolErrorCause;

pub use code::codebase_search::CodebaseSearchTool;
pub use code::graph_query::CodeGraphQueryTool;
pub use code::outline::CodeOutlineTool;
pub use code::review::CodeReviewTool;
pub use code::xfile_refactor::CodeXfileRefactorTool;
pub use delegate::{BackgroundDelegateResult, BackgroundTaskStatus};
#[cfg(feature = "tool-search-social")]
pub use discord_search::DiscordSearchTool;
pub use plan_mode::enter::{EnterPlanModeTool, PlanModeFlag};
pub use escalate::EscalateToHumanTool;
pub use exa_search::ExaSearchTool;
pub use plan_mode::exit::ExitPlanModeTool;
pub use file::edit::FileEditTool;
pub use file::read::FileReadTool;
pub use file::write::FileWriteTool;
pub use flow::rollback::FlowRollbackTool;
pub use flow::run::FlowRunTool;
pub use fs_ops::{CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool};
pub use gemini_cli::GeminiCliTool;
pub use git_operations::GitOperationsTool;
pub use github::advanced_search::GitHubAdvancedSearchTool;
pub use github::search::GitHubSearchTool;
pub use glob::edit::GlobEditTool;
pub use glob::search::GlobSearchTool;
#[cfg(feature = "tool-productivity")]
pub use google_workspace::GoogleWorkspaceTool;
#[cfg(feature = "hardware")]
pub use hardware::board_info::HardwareBoardInfoTool;
#[cfg(feature = "hardware")]
pub use hardware::memory::map::HardwareMemoryMapTool;
#[cfg(feature = "hardware")]
pub use hardware::memory::read::HardwareMemoryReadTool;
pub use http_request::HttpRequestTool;
#[cfg(feature = "tool-image")]
pub use image::generate::ImageGenTool;
pub use deck_compile::DeckCompileTool;
pub use design_system_read::DesignSystemReadTool;
pub use designer::lint::DesignerLintTool;
pub use designer::scaffold::DesignerScaffoldTool;
pub use designer::skill_read::DesignerSkillReadTool;
pub use designer::template_read::DesignerTemplateReadTool;
pub use figma_fetch::FigmaFetchTool;
pub use media::MediaGenTool;
#[cfg(feature = "tool-image")]
pub use image::info::ImageInfoTool;
#[cfg(feature = "tool-image")]
pub use image::search::ImageSearchTool;
pub use incremental_optimize::IncrementalOptimizeTool;
#[cfg(feature = "tool-productivity")]
pub use jira_tool::JiraTool;
pub use knowledge_tool::KnowledgeTool;
#[cfg(feature = "tool-linkedin")]
pub use linkedin::LinkedInTool;
pub use autoresearch::{
    AutoresearchRuntime, MultiPersonaReviewTool, ScenarioMatrixTool, SecurityAuditTool,
};
pub use llm_task::LlmTaskTool;
pub use lsp::LspTool;
pub use lsp::format::LspFormatTool;
pub use lsp::rename::LspRenameTool;
pub use mcp::client::McpRegistry;
pub use mcp::deferred::{
    ActivatedToolSet, DeferredBuiltinToolSet, DeferredMcpToolSet, build_deferred_builtin_section,
    build_deferred_builtin_section_with_surface, build_deferred_tools_section,
};
pub use mcp::resources::list::McpResourcesListTool;
pub use mcp::resources::read::McpResourcesReadTool;
pub use mcp::tool::McpToolWrapper;
pub use memory::export::MemoryExportTool;
pub use memory::forget::MemoryForgetTool;
pub use memory::purge::MemoryPurgeTool;
pub use memory::recall::MemoryRecallTool;
pub use memory::store::MemoryStoreTool;
pub use microsoft365::Microsoft365Tool;
pub use model::routing_config::ModelRoutingConfigTool;
pub use model::switch::ModelSwitchTool;
pub use multi::edit::MultiEditTool;
pub use multi::search::MultiSearchTool;
pub use node::tool::NodeTool;
pub use notebook_edit::NotebookEditTool;
#[cfg(feature = "tool-productivity")]
pub use notion_tool::NotionTool;
pub use now::NowTool;
pub use opencode_cli::OpenCodeCliTool;
pub use diff_apply::DiffApplyTool;
pub use patch_apply::PatchApplyTool;
pub use write_plan::WritePlanTool;
pub use pdf_read::PdfReadTool;
pub use poll::{ChannelMapHandle, PollTool};
pub use powershell::PowerShellTool;
pub use present_files::PresentFilesTool;
pub use project_intel::ProjectIntelTool;
pub use proxy_config::ProxyConfigTool;
#[cfg(feature = "tool-pushover")]
pub use pushover::PushoverTool;
pub use reaction::ReactionTool;
pub use read::skill::ReadSkillTool;
pub use read::user_rule::ReadUserRuleTool;
#[cfg(feature = "tool-search-social")]
pub use reddit_search::RedditSearchTool;
pub use registry::ToolRegistry;
#[cfg(feature = "tool-reports")]
pub use report::template_tool::ReportTemplateTool;
pub use restore_file::RestoreFileTool;
#[cfg(feature = "tool-cron")]
pub use schedule::ScheduleTool;
pub use schema::{CleaningStrategy, SchemaCleanr};
#[cfg(feature = "tool-image")]
pub use screenshot::ScreenshotTool;
#[cfg(feature = "tool-cloud-ops")]
pub use security_ops::SecurityOpsTool;
pub use send_message::SendMessageTool;
pub use sessions::{
    SessionsHistoryTool, SessionsListTool, SessionsOutlineTool, SessionsSearchTool, SessionsSendTool,
};
pub use setup_agent::SetupAgentTool;
pub use shell::ShellTool;
pub use skill::http::SkillHttpTool;
pub use skill::tool::SkillShellTool;
pub use sleep::SleepTool;
#[cfg(feature = "tool-sop")]
pub use sop::advance::SopAdvanceTool;
#[cfg(feature = "tool-sop")]
pub use sop::approve::SopApproveTool;
#[cfg(feature = "tool-sop")]
pub use sop::execute::SopExecuteTool;
#[cfg(feature = "tool-sop")]
pub use sop::list::SopListTool;
#[cfg(feature = "tool-sop")]
pub use sop::status::SopStatusTool;
pub use structured_output::StructuredOutputTool;
pub use swarm::SwarmTool;
pub use task::create::TaskCreateTool;
pub use task::get::TaskGetTool;
pub use task::list::TaskListTool;
pub use task::output::TaskOutputTool;
pub use task::stop::TaskStopTool;
pub use task::update::TaskUpdateTool;
pub use tavily_search::TavilySearchTool;
#[cfg(feature = "tool-team")]
pub use team::create::TeamCreateTool;
#[cfg(feature = "tool-team")]
pub use team::delete::TeamDeleteTool;
pub use text_browser::TextBrowserTool;
pub use todo_write::TodoWriteTool;
pub use handler::search::ToolSearchTool;
pub use handler::tier::{
    BuiltinDeferredRegistrationOptions, BuiltinToolTier, TOOL_TIERS, ToolRiskLevel,
    ToolSurfaceBaseline, ToolTierEntry, apply_builtin_deferred_registration,
    apply_builtin_deferred_registration_with_options, build_deferred_builtin_set_for_surface,
    classify as classify_tool_tier, deferred_loading_effective, is_eager_tier, known_tool_names,
    partition_for_llm, risk_for, tier_for,
};
pub use traits::Tool;
pub use traits::{ToolResult, ToolSpec};
pub use update_plan::UpdatePlanTool;
pub use verifiable_intent::VerifiableIntentTool;
#[cfg(feature = "tool-image")]
pub use view_image::ViewImageTool;
#[cfg(feature = "tool-utility-misc")]
pub use weather_tool::WeatherTool;
pub use web::fetch::WebFetchTool;
pub use web::search::tool::WebSearchTool;
pub use workspace_tool::WorkspaceTool;
pub use worktree::enter::WorktreeEnterTool;
pub use worktree::exit::WorktreeExitTool;
#[cfg(feature = "tool-search-social")]
pub use youtube_search::YouTubeSearchTool;

use crate::config::{Config, DelegateAgentConfig};
use crate::memory::Memory;
use crate::runtime::{NativeRuntime, RuntimeAdapter};
use crate::security::{SecurityPolicy, create_sandbox};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
pub use task::types::{TaskInfo, TaskManager, TaskManagerHandle, TaskState};

pub type DelegateParentToolsHandle = Arc<RwLock<Vec<Arc<dyn Tool>>>>;

pub struct ArcToolRef(pub Arc<dyn Tool>);

#[async_trait]
impl Tool for ArcToolRef {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.0.parameters_schema()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.0.execute(args).await
    }
}

#[derive(Clone)]
struct ArcDelegatingTool {
    inner: Arc<dyn Tool>,
}

impl ArcDelegatingTool {
    fn boxed(inner: Arc<dyn Tool>) -> Box<dyn Tool> {
        Box::new(Self { inner })
    }
}

#[async_trait]
impl Tool for ArcDelegatingTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.inner.parameters_schema()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.inner.execute(args).await
    }
}

fn boxed_registry_from_arcs(tools: Vec<Arc<dyn Tool>>) -> Vec<Box<dyn Tool>> {
    tools.into_iter().map(ArcDelegatingTool::boxed).collect()
}

// One TaskManager per workspace, shared across every agent (parent, delegate
// subagents, coordinator) that builds a tool set against that workspace, so
// task_create/task_get/... actually track shared state cross-agent. Different
// workspaces remain fully isolated.
fn shared_task_manager_for(workspace_root: &std::path::Path) -> TaskManagerHandle {
    static REGISTRY: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<std::path::PathBuf, TaskManagerHandle>>,
    > = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));
    let key = if workspace_root.as_os_str().is_empty() {
        std::path::PathBuf::from("__no_workspace__")
    } else {
        crate::util::normalize_path_for_containment(workspace_root)
    };
    let mut reg = REGISTRY.lock();
    reg.entry(key)
        .or_insert_with(|| Arc::new(RwLock::new(TaskManager::new())))
        .clone()
}

pub fn default_tools(security: Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    default_tools_with_runtime(security, Arc::new(NativeRuntime::new()))
}

pub fn default_tools_with_runtime(
    security: Arc<SecurityPolicy>,
    runtime: Arc<dyn RuntimeAdapter>,
) -> Vec<Box<dyn Tool>> {

    // Lazy: resolves the multi-agent runtime at each acquire, so a tool surface
    // built before init_global_runtime() still serializes with later writers.
    let lock_provider: Arc<dyn crate::apply_model::LockProvider> = Arc::new(
        crate::apply_model::lock_manager_provider::LazyRuntimeLockProvider::new("tool_runtime"),
    );
    let default_workspace_root = security.workspace_root_handle().read().clone();
    let default_history_root = if default_workspace_root.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        default_workspace_root
    };
    let shared_edit_history =
        crate::tools::edit_history::EditHistory::shared_for_workspace(&default_history_root);
    let shared_ops = Arc::new(
        crate::apply_model::OpsApplier::default_for_shared_workspace(
            security.workspace_root_handle(),
        )
        .with_allowed_roots(security.allowed_roots.clone())
        .with_lock_provider(lock_provider)
        .with_edit_history(shared_edit_history.clone()),
    );

    vec![
        Box::new(ShellTool::new(security.clone(), runtime)),
        Box::new(background::status::BackgroundStatusTool::new()),
        Box::new(background::logs::BackgroundLogsTool::new()),
        Box::new(background::kill::BackgroundKillTool::new()),
        Box::new(background::wait::BackgroundWaitTool::new()),
        Box::new(FileReadTool::new(security.clone())),
        Box::new(
            FileWriteTool::new(security.clone())
                .with_ops_applier(shared_ops.clone())
                .with_edit_history(shared_edit_history.clone()),
        ),
        Box::new(
            FileEditTool::new(security.clone())
                .with_ops_applier(shared_ops.clone())
                .with_edit_history(shared_edit_history.clone()),
        ),
        Box::new(
            NotebookEditTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
        ),
        Box::new(GlobSearchTool::new(security.clone())),
        Box::new(
            GlobEditTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
        ),
        Box::new(
            LspRenameTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
        ),
        Box::new(lsp::format::LspFormatTool::new(security.clone())),
        Box::new(
            PatchApplyTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
        ),
        Box::new(DiffApplyTool::new(security.clone()).with_ops_applier(shared_ops)),
        Box::new(WritePlanTool::new()),
        Box::new(ContentSearchTool::new(security)),
    ]
}

pub fn dedupe_tool_specs(specs: &[ToolSpec]) -> Vec<ToolSpec> {
    let mut seen: std::collections::HashSet<&str> =
        std::collections::HashSet::with_capacity(specs.len());
    let mut duplicates: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut deduped: Vec<ToolSpec> = Vec::with_capacity(specs.len());
    for spec in specs {
        if seen.insert(spec.name.as_str()) {
            deduped.push(spec.clone());
        } else {
            *duplicates.entry(spec.name.clone()).or_insert(0) += 1;
        }
    }
    if !duplicates.is_empty() {
        let summary = duplicates
            .iter()
            .map(|(name, extra)| format!("{name} (+{extra})"))
            .collect::<Vec<_>>()
            .join(", ");
        tracing::warn!(
            target: "tools.dedupe",
            input_len = specs.len(),
            output_len = deduped.len(),
            duplicates = %summary,
            "dropped duplicate tool specs before sending to provider"
        );
    }
    deduped
}

pub fn register_skill_tools(
    tools_registry: &mut Vec<Box<dyn Tool>>,
    skills: &[crate::skills::Skill],
    security: Arc<SecurityPolicy>,
) {
    let skill_tools = crate::skills::skills_to_tools(skills, security);
    let existing_names: std::collections::HashSet<String> = tools_registry
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    for tool in skill_tools {
        if existing_names.contains(tool.name()) {
            tracing::warn!(
                "Skill tool '{}' shadows built-in tool, skipping",
                tool.name()
            );
        } else {
            tools_registry.push(tool);
        }
    }
}

#[allow(
    clippy::implicit_hasher,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn all_tools(
    config: Arc<Config>,
    security: &Arc<SecurityPolicy>,
    memory: Arc<dyn Memory>,
    composio_key: Option<&str>,
    composio_entity_id: Option<&str>,
    browser_config: &crate::config::BrowserConfig,
    http_config: &crate::config::HttpRequestConfig,
    web_fetch_config: &crate::config::WebFetchConfig,
    workspace_dir: &std::path::Path,
    agents: &HashMap<String, DelegateAgentConfig>,
    fallback_api_key: Option<&str>,
    root_config: &crate::config::Config,
    canvas_store: Option<CanvasStore>,
) -> (
    Vec<Box<dyn Tool>>,
    Option<DelegateParentToolsHandle>,
    Option<ChannelMapHandle>,
    ChannelMapHandle,
    Option<ChannelMapHandle>,
    Option<ChannelMapHandle>,
    PlanModeFlag,
) {
    all_tools_with_runtime(
        config,
        security,
        Arc::new(NativeRuntime::new()),
        memory,
        composio_key,
        composio_entity_id,
        browser_config,
        http_config,
        web_fetch_config,
        workspace_dir,
        agents,
        fallback_api_key,
        root_config,
        canvas_store,
    )
}

#[allow(
    clippy::implicit_hasher,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn all_tools_with_runtime(
    config: Arc<Config>,
    security: &Arc<SecurityPolicy>,
    runtime: Arc<dyn RuntimeAdapter>,
    memory: Arc<dyn Memory>,
    composio_key: Option<&str>,
    composio_entity_id: Option<&str>,
    browser_config: &crate::config::BrowserConfig,
    http_config: &crate::config::HttpRequestConfig,
    web_fetch_config: &crate::config::WebFetchConfig,
    workspace_dir: &std::path::Path,
    agents: &HashMap<String, DelegateAgentConfig>,
    fallback_api_key: Option<&str>,
    root_config: &crate::config::Config,
    canvas_store: Option<CanvasStore>,
) -> (
    Vec<Box<dyn Tool>>,
    Option<DelegateParentToolsHandle>,
    Option<ChannelMapHandle>,
    ChannelMapHandle,
    Option<ChannelMapHandle>,
    Option<ChannelMapHandle>,
    PlanModeFlag,
) {
    let has_shell_access = runtime.has_shell_access();
    let workspace_root_pb = security.workspace_root_handle().read().clone();
    crate::security::configure_fs_confinement(
        root_config.security.sandbox.confine_filesystem,
        Some(workspace_root_pb.clone()),
        root_config
            .autonomy
            .allowed_roots
            .iter()
            .map(std::path::PathBuf::from)
            .collect(),
    );
    let sandbox = create_sandbox(
        &root_config.security,
        if workspace_root_pb.as_os_str().is_empty() {
            None
        } else {
            Some(workspace_root_pb.as_path())
        },
    );
    let plan_mode_flag: PlanModeFlag = PlanModeFlag::new();
    // Share one TaskManager per workspace so a parent agent and its
    // delegate/coordinator subagents (which build their own tool set against the
    // same workspace) observe the SAME task list. Previously each agent got a
    // private in-memory manager, so cross-agent task tracking silently did
    // nothing. Different workspaces stay isolated.
    let task_manager: TaskManagerHandle = shared_task_manager_for(&workspace_root_pb);
    #[cfg(not(feature = "tool-utility-misc"))]
    let _ = canvas_store;
    #[cfg(not(feature = "tool-productivity"))]
    let _ = (composio_key, composio_entity_id);
    #[cfg(feature = "tool-team")]
    let team_registry: team::create::TeamRegistry = team::global_team_registry();
    let optimization_state: incremental_optimize::OptimizationStateHandle = Arc::new(
        parking_lot::RwLock::new(incremental_optimize::OptimizationState::default()),
    );

    let lock_provider: Arc<dyn crate::apply_model::LockProvider> = Arc::new(
        crate::apply_model::lock_manager_provider::LazyRuntimeLockProvider::new("tool_runtime"),
    );
    let edit_history_root = if workspace_root_pb.as_os_str().is_empty() {
        workspace_dir.to_path_buf()
    } else {
        workspace_root_pb.clone()
    };
    let shared_edit_history =
        crate::tools::edit_history::EditHistory::shared_for_workspace(&edit_history_root);
    let shared_ops = Arc::new(
        crate::apply_model::OpsApplier::default_for_shared_workspace(
            security.workspace_root_handle(),
        )
        .with_allowed_roots(security.allowed_roots.clone())
        .with_lock_provider(lock_provider)
        .with_edit_history(shared_edit_history.clone()),
    );

    let mut tool_arcs: Vec<Arc<dyn Tool>> = vec![
        Arc::new(
            ShellTool::new_with_sandbox(security.clone(), runtime, sandbox)
                .with_timeout_secs(root_config.shell_tool.timeout_secs),
        ),
        Arc::new(background::status::BackgroundStatusTool::new()),
        Arc::new(background::logs::BackgroundLogsTool::new()),
        Arc::new(background::kill::BackgroundKillTool::new()),
        Arc::new(background::wait::BackgroundWaitTool::new()),
        Arc::new(FileReadTool::new(security.clone())),
        Arc::new(
            FileWriteTool::new(security.clone())
                .with_ops_applier(shared_ops.clone())
                .with_edit_history(shared_edit_history.clone()),
        ),
        Arc::new(
            FileEditTool::new(security.clone())
                .with_ops_applier(shared_ops.clone())
                .with_edit_history(shared_edit_history.clone()),
        ),
        Arc::new(
            MultiEditTool::new(security.clone())
                .with_ops_applier(shared_ops.clone())
                .with_edit_history(shared_edit_history.clone()),
        ),
        Arc::new(
            NotebookEditTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
        ),
        Arc::new(GlobSearchTool::new(security.clone())),
        Arc::new(ContentSearchTool::new(security.clone())),
        #[cfg(feature = "tool-workspace-deep")]
        Arc::new(workspace_deep_search::WorkspaceDeepSearchTool::new(
            security.clone(),
        )),
        #[cfg(feature = "tool-cron")]
        Arc::new(CronAddTool::new(config.clone(), security.clone())),
        #[cfg(feature = "tool-cron")]
        Arc::new(CronListTool::new(config.clone())),
        #[cfg(feature = "tool-cron")]
        Arc::new(CronRemoveTool::new(config.clone(), security.clone())),
        #[cfg(feature = "tool-cron")]
        Arc::new(CronUpdateTool::new(config.clone(), security.clone())),
        #[cfg(feature = "tool-cron")]
        Arc::new(CronRunTool::new(config.clone(), security.clone())),
        #[cfg(feature = "tool-cron")]
        Arc::new(CronRunsTool::new(config.clone())),
        Arc::new(MemoryStoreTool::new(memory.clone(), security.clone())),
        Arc::new(MemoryRecallTool::new(memory.clone())),
        Arc::new(MemoryForgetTool::new(memory.clone(), security.clone())),
        Arc::new(MemoryExportTool::new(memory.clone())),
        Arc::new(MemoryPurgeTool::new(memory.clone(), security.clone())),
        #[cfg(feature = "tool-cron")]
        Arc::new(ScheduleTool::new(security.clone(), root_config.clone())),
        Arc::new(ModelRoutingConfigTool::new(
            config.clone(),
            security.clone(),
        )),
        Arc::new(ModelSwitchTool::new(security.clone())),
        Arc::new(ProxyConfigTool::new(config.clone(), security.clone())),
        Arc::new(GitOperationsTool::new(security.clone())),
        Arc::new(
            CodeToSpecTool::new(security.workspace_root_handle())
                .with_security(security.clone()),
        ),
        Arc::new(IncrementalOptimizeTool::new(
            optimization_state.clone(),
            Arc::clone(&security.workspace_root_handle()),
        )),
        #[cfg(feature = "tool-pushover")]
        Arc::new(PushoverTool::new(security.clone())),
        #[cfg(feature = "tool-utility-misc")]
        Arc::new(CalculatorTool::new()),
        Arc::new(LspTool::new(security.workspace_root_handle())),
        {
            let registry = crate::inline_completion::registry::default_provider(root_config);
            let tool = match registry {
                Some(r) => crate::tools::inline::complete::InlineCompleteTool::with_registry(r),
                None => crate::tools::inline::complete::InlineCompleteTool::new(),
            };
            Arc::new(tool)
        },
        Arc::new(StructuredOutputTool::new(None)),
        Arc::new(McpResourcesListTool::new()),
        Arc::new(McpResourcesReadTool::new(None)),
        Arc::new(EnterPlanModeTool::new(plan_mode_flag.clone())),
        Arc::new(
            ExitPlanModeTool::new(plan_mode_flag.clone())
                .with_workspace_root(security.workspace_root_handle()),
        ),
        #[cfg(feature = "tool-curator")]
        {
            let svc = crate::services::try_get_services();
            let curator_flag = svc
                .as_ref()
                .map(|s| s.curator_mode_flag.clone())
                .unwrap_or_else(|| {
                    Arc::new(crate::tools::curator::tools::CuratorModeRegistry::new())
                });
            let curator_state = svc
                .as_ref()
                .map(|s| s.curator_state.clone())
                .unwrap_or_else(crate::tools::curator::state::new_curator_state);
            Arc::new(crate::tools::curator::EnterCuratorModeTool::new(
                curator_flag,
                curator_state,
                security.workspace_root_handle(),
            ))
        },
        #[cfg(feature = "tool-curator")]
        {
            let svc = crate::services::try_get_services();
            let state = svc
                .as_ref()
                .map(|s| s.curator_state.clone())
                .unwrap_or_else(crate::tools::curator::state::new_curator_state);
            Arc::new(crate::tools::curator::CuratorCollectTool::new(
                state,
                security.clone(),
            ))
        },
        #[cfg(feature = "tool-curator")]
        Arc::new(crate::tools::curator::CuratorTemplateListTool::new()),
        #[cfg(feature = "tool-curator")]
        {
            let svc = crate::services::try_get_services();
            let state = svc
                .as_ref()
                .map(|s| s.curator_state.clone())
                .unwrap_or_else(crate::tools::curator::state::new_curator_state);
            Arc::new(crate::tools::curator::CuratorTemplateApplyTool::new(
                state,
                security.clone(),
            ))
        },
        #[cfg(feature = "tool-curator")]
        {
            let svc = crate::services::try_get_services();
            let state = svc
                .as_ref()
                .map(|s| s.curator_state.clone())
                .unwrap_or_else(crate::tools::curator::state::new_curator_state);
            Arc::new(crate::tools::curator::CuratorGitReferenceTool::new(
                state,
                security.clone(),
            ))
        },
        #[cfg(feature = "tool-curator")]
        {
            let svc = crate::services::try_get_services();
            let state = svc
                .as_ref()
                .map(|s| s.curator_state.clone())
                .unwrap_or_else(crate::tools::curator::state::new_curator_state);
            Arc::new(crate::tools::curator::CuratorLocalReferenceTool::new(
                state,
                security.clone(),
            ))
        },
        #[cfg(feature = "tool-curator")]
        {
            let svc = crate::services::try_get_services();
            let curator_flag = svc
                .as_ref()
                .map(|s| s.curator_mode_flag.clone())
                .unwrap_or_else(|| {
                    Arc::new(crate::tools::curator::tools::CuratorModeRegistry::new())
                });
            let curator_state = svc
                .as_ref()
                .map(|s| s.curator_state.clone())
                .unwrap_or_else(crate::tools::curator::state::new_curator_state);
            let pending_curator = svc
                .as_ref()
                .map(|s| s.pending_curator.clone())
                .unwrap_or_else(crate::tools::curator::state::new_pending_curator);
            Arc::new(crate::tools::curator::ExitCuratorModeTool::new(
                curator_flag,
                plan_mode_flag.clone(),
                curator_state,
                pending_curator,
                security.clone(),
            ))
        },
        Arc::new(SleepTool::new()),
        Arc::new(FlowRunTool::new()),
        Arc::new(FlowRollbackTool::new()),
        Arc::new(CodeGraphQueryTool::new()),
        Arc::new(CodebaseSearchTool::new()),
        Arc::new(CodeOutlineTool::new(workspace_dir.to_path_buf())),
        Arc::new(CodeReviewTool::new()),
        Arc::new(
            CodeXfileRefactorTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
        ),
        #[cfg(feature = "tool-utility-misc")]
        Arc::new(WeatherTool::new()),
        #[cfg(feature = "tool-utility-misc")]
        Arc::new(CanvasTool::new(canvas_store.unwrap_or_default())),
        Arc::new(TodoWriteTool::new(
            crate::services::try_get_services()
                .map(|svc| svc.todo_store.clone())
                .unwrap_or_else(crate::tools::todo_write::new_todo_store),
        )),
        Arc::new(NowTool::new()),
        Arc::new(ReadUserRuleTool::new()),
        Arc::new(CopyPathTool::new(security.clone())),
        Arc::new(MovePathTool::new(security.clone())),
        Arc::new(DeletePathTool::new(security.clone())),
        Arc::new(CreateDirectoryTool::new(security.clone())),
        Arc::new(RestoreFileTool::new(security.clone())),
        Arc::new(DiagnosticsTool::new(security.workspace_root_handle())),
        Arc::new(UpdatePlanTool::with_workspace_root(
            Arc::new(RwLock::new(Vec::new())),
            security.workspace_root_handle(),
        )),
        Arc::new(BriefTool::new()),
        Arc::new(TaskCreateTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskGetTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskUpdateTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskListTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskOutputTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskStopTool::new(Arc::clone(&task_manager))),
        #[cfg(feature = "tool-team")]
        Arc::new(TeamCreateTool::new(Arc::clone(&team_registry))),
        #[cfg(feature = "tool-team")]
        Arc::new(TeamDeleteTool::new(Arc::clone(&team_registry))),
        Arc::new(SendMessageTool::new(
            send_message::global_mailbox(),
            "main".to_string(),
        )),
        Arc::new(send_message::ReadMessagesTool::new(
            send_message::global_mailbox(),
            "main".to_string(),
        )),
        Arc::new(DirListTool::new(security.clone())),
        Arc::new(PresentFilesTool::new(security.clone())),
        #[cfg(feature = "tool-image")]
        Arc::new(ViewImageTool::new(security.clone())),
        #[cfg(feature = "tool-image")]
        Arc::new(ImageSearchTool::new(
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
        )),
        Arc::new(SetupAgentTool::new(security.clone())),
        Arc::new(
            MultiSearchTool::new(
                root_config.web_search.max_results,
                root_config.web_search.timeout_secs,
                root_config.web_search.brave_api_key.clone(),
                root_config.web_search.searxng_instance_url.clone(),
            )
            .with_tavily_key(root_config.web_search.tavily_api_key.clone())
            .with_exa_key(root_config.web_search.exa_api_key.clone()),
        ),
        #[cfg(feature = "tool-search-social")]
        Arc::new(YouTubeSearchTool::new(
            std::env::var("YOUTUBE_API_KEY").ok(),
            5,
            root_config.web_search.timeout_secs,
        )),
        Arc::new(GitHubSearchTool::from_env(
            root_config.web_search.timeout_secs,
        )),
        Arc::new(GitHubAdvancedSearchTool::from_env(
            root_config.web_search.timeout_secs,
        )),
        #[cfg(feature = "tool-search-social")]
        Arc::new(RedditSearchTool::new(
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
        )),
        Arc::new(TavilySearchTool::new(
            root_config.web_search.tavily_api_key.clone(),
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
        )),
        Arc::new(ExaSearchTool::new(
            root_config.web_search.exa_api_key.clone(),
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
        )),
    ];

    tool_arcs.push(Arc::new(
        GlobEditTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
    ));
    tool_arcs.push(Arc::new(
        LspRenameTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
    ));
    tool_arcs.push(Arc::new(lsp::format::LspFormatTool::new(security.clone())));
    tool_arcs.push(Arc::new(
        PatchApplyTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
    ));
    tool_arcs.push(Arc::new(
        DiffApplyTool::new(security.clone()).with_ops_applier(shared_ops.clone()),
    ));
    tool_arcs.push(Arc::new(WritePlanTool::new()));

    #[cfg(target_os = "windows")]
    tool_arcs.push(Arc::new(PowerShellTool::new(security.clone())));
    tool_arcs.push(Arc::new(WorktreeEnterTool::new(security.clone())));
    tool_arcs.push(Arc::new(WorktreeExitTool::new(security.clone())));

    #[cfg(feature = "tool-search-social")]
    if root_config.channels_config.discord_history.is_some() {
        match crate::memory::SqliteMemory::new_named(workspace_dir, "discord") {
            Ok(discord_mem) => {
                tool_arcs.push(Arc::new(DiscordSearchTool::new(Arc::new(discord_mem))));
            }
            Err(e) => {
                tracing::warn!("discord_search: failed to open discord.db: {e}");
            }
        }
    }

    {
        match crate::providers::resolve_default_model(root_config) {
            Ok(llm_task_model) => {
                let llm_task_provider_raw = root_config
                    .default_provider
                    .clone()
                    .unwrap_or_else(|| "openrouter".to_string());
                let llm_task_provider = crate::providers::resolve_runtime_provider_name(
                    &llm_task_provider_raw,
                    root_config,
                );
                let llm_task_runtime_options = crate::providers::ProviderRuntimeOptions {
                    auth_profile_override: None,
                    provider_api_url: root_config.api_url.clone(),
                    sen_dir: root_config
                        .config_path
                        .parent()
                        .map(std::path::PathBuf::from),
                    secrets_encrypt: root_config.secrets.encrypt,
                    reasoning_enabled: root_config.runtime.reasoning_enabled,
                    reasoning_effort: root_config.runtime.reasoning_effort.clone(),
                    provider_timeout_secs: Some(root_config.provider_timeout_secs),
                    extra_headers: crate::providers::merged_extra_headers_for_config(root_config),
                    api_path: root_config.api_path.clone(),
                    provider_max_tokens: root_config.provider_max_tokens,
                    model_context_windows: root_config.model_context_windows.clone(),
                };
                let autoresearch_runtime = Arc::new(AutoresearchRuntime::new(
                    security.clone(),
                    llm_task_provider.clone(),
                    llm_task_model.clone(),
                    root_config.default_temperature,
                    root_config.api_key.clone(),
                    llm_task_runtime_options.clone(),
                    Arc::clone(&security.workspace_root_handle()),
                ));
                tool_arcs.push(Arc::new(LlmTaskTool::new(
                    security.clone(),
                    llm_task_provider,
                    llm_task_model,
                    root_config.default_temperature,
                    root_config.api_key.clone(),
                    llm_task_runtime_options,
                )));
                tool_arcs.push(Arc::new(MultiPersonaReviewTool::new(
                    autoresearch_runtime.clone(),
                )));
                tool_arcs.push(Arc::new(ScenarioMatrixTool::new(
                    autoresearch_runtime.clone(),
                )));
                tool_arcs.push(Arc::new(SecurityAuditTool::new(
                    autoresearch_runtime.clone(),
                )));
            }
            Err(e) => {
                tracing::warn!(
                    target = "config",
                    "no_model_configured: skipping LlmTaskTool + autoresearch suite registration: {e}"
                );
            }
        }
    }

    if matches!(
        root_config.skills.prompt_injection_mode,
        crate::config::SkillsPromptInjectionMode::Compact
    ) {
        tool_arcs.push(Arc::new(ReadSkillTool::new(
            workspace_dir.to_path_buf(),
            root_config.skills.open_skills_enabled,
            root_config.skills.open_skills_dir.clone(),
            root_config.skills.disabled_skills.clone(),
        )));
    }

    tool_arcs.push(Arc::new(debug_test_report::DebugTestReportTool::new()));

    if browser_config.enabled {

        tool_arcs.push(Arc::new(BrowserOpenTool::new(
            security.clone(),
            browser_config.allowed_domains.clone(),
        )));

        tool_arcs.push(Arc::new(BrowserTool::new_with_backend(
            security.clone(),
            browser_config.allowed_domains.clone(),
            browser_config.session_name.clone(),
            browser_config.backend.clone(),
            browser_config.native_headless,
            browser_config.native_webdriver_url.clone(),
            browser_config.native_chrome_path.clone(),
        )));
    }

    if root_config.browser_delegate.enabled {
        if has_shell_access {
            tool_arcs.push(Arc::new(BrowserDelegateTool::new(
                security.clone(),
                root_config.browser_delegate.clone(),
            )));
        } else {
            tracing::warn!(
                "browser_delegate: skipped registration because the current runtime does not allow shell access"
            );
        }
    }

    if http_config.enabled {
        tool_arcs.push(Arc::new(HttpRequestTool::new(
            security.clone(),
            http_config.allowed_domains.clone(),
            http_config.max_response_size,
            http_config.timeout_secs,
            http_config.allow_private_hosts,
        )));
    }

    #[allow(unused_variables)]
    let web_fetch_tool: Option<Arc<WebFetchTool>> = if web_fetch_config.enabled {
        let tool = Arc::new(WebFetchTool::new(
            security.clone(),
            web_fetch_config.allowed_domains.clone(),
            web_fetch_config.blocked_domains.clone(),
            web_fetch_config.max_response_size,
            web_fetch_config.timeout_secs,
            web_fetch_config.firecrawl.clone(),
            web_fetch_config.allowed_private_hosts.clone(),
        ));
        tool_arcs.push(tool.clone());
        Some(tool)
    } else {
        None
    };

    if root_config.text_browser.enabled {
        tool_arcs.push(Arc::new(TextBrowserTool::new(
            security.clone(),
            root_config.text_browser.preferred_browser.clone(),
            root_config.text_browser.timeout_secs,
        )));
    }

    #[allow(unused_variables)]
    let web_search_tool: Option<Arc<WebSearchTool>> = if root_config.web_search.enabled {
        let tool = Arc::new(WebSearchTool::new_with_config(
            root_config.web_search.provider.clone(),
            root_config.web_search.brave_api_key.clone(),
            root_config.web_search.searxng_instance_url.clone(),
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
            root_config.config_path.clone(),
            root_config.secrets.encrypt,
        ));
        tool_arcs.push(tool.clone());
        Some(tool)
    } else {
        None
    };

    #[cfg(feature = "tool-curator")]
    if let (Some(ws), Some(wf)) = (web_search_tool.as_ref(), web_fetch_tool.as_ref()) {
        let svc = crate::services::try_get_services();
        let curator_state = svc
            .as_ref()
            .map(|s| s.curator_state.clone())
            .unwrap_or_else(crate::tools::curator::state::new_curator_state);
        tool_arcs.push(Arc::new(crate::tools::curator::CuratorDeepCollectTool::new(
            curator_state,
            security.clone(),
            ws.clone(),
            wf.clone(),
        )));
    }

    #[cfg(feature = "tool-productivity")]
    if root_config.notion.enabled {
        let notion_api_key = if root_config.notion.api_key.trim().is_empty() {
            std::env::var("NOTION_API_KEY").unwrap_or_default()
        } else {
            root_config.notion.api_key.trim().to_string()
        };
        if notion_api_key.trim().is_empty() {
            tracing::warn!(
                "Notion tool enabled but no API key found (set notion.api_key or NOTION_API_KEY env var)"
            );
        } else {
            tool_arcs.push(Arc::new(NotionTool::new(notion_api_key, security.clone())));
        }
    }

    #[cfg(feature = "tool-productivity")]
    if root_config.jira.enabled {
        let api_token = if root_config.jira.api_token.trim().is_empty() {
            std::env::var("JIRA_API_TOKEN").unwrap_or_default()
        } else {
            root_config.jira.api_token.trim().to_string()
        };
        if api_token.trim().is_empty() {
            tracing::warn!(
                "Jira tool enabled but no API token found (set jira.api_token or JIRA_API_TOKEN env var)"
            );
        } else if root_config.jira.base_url.trim().is_empty() {
            tracing::warn!("Jira tool enabled but jira.base_url is empty  -  skipping registration");
        } else if root_config.jira.email.trim().is_empty() {
            tracing::warn!("Jira tool enabled but jira.email is empty  -  skipping registration");
        } else {
            tool_arcs.push(Arc::new(JiraTool::new(
                root_config.jira.base_url.trim().to_string(),
                root_config.jira.email.trim().to_string(),
                api_token,
                root_config.jira.allowed_actions.clone(),
                security.clone(),
                root_config.jira.timeout_secs,
            )));
        }
    }

    if root_config.project_intel.enabled {
        tool_arcs.push(Arc::new(ProjectIntelTool::new(
            root_config.project_intel.default_language.clone(),
            root_config.project_intel.risk_sensitivity.clone(),
        )));

        #[cfg(feature = "tool-reports")]
        tool_arcs.push(Arc::new(ReportTemplateTool::new()));
    }

    #[cfg(feature = "tool-cloud-ops")]
    if root_config.security_ops.enabled {
        tool_arcs.push(Arc::new(SecurityOpsTool::new(
            root_config.security_ops.clone(),
        )));
    }

    if root_config.backup.enabled {
        tool_arcs.push(Arc::new(BackupTool::new(
            workspace_dir.to_path_buf(),
            root_config.backup.include_dirs.clone(),
            root_config.backup.max_keep,
        )));
    }

    #[cfg(feature = "tool-cloud-ops")]
    if root_config.data_retention.enabled {
        tool_arcs.push(Arc::new(DataManagementTool::new(
            workspace_dir.to_path_buf(),
            root_config.data_retention.retention_days,
        )));
    }

    #[cfg(feature = "tool-cloud-ops")]
    if root_config.cloud_ops.enabled {
        tool_arcs.push(Arc::new(CloudOpsTool::new(root_config.cloud_ops.clone())));
        tool_arcs.push(Arc::new(CloudPatternsTool::new()));
    }

    #[cfg(feature = "tool-productivity")]
    if root_config.google_workspace.enabled && has_shell_access {
        tool_arcs.push(Arc::new(GoogleWorkspaceTool::new(
            security.clone(),
            root_config.google_workspace.allowed_services.clone(),
            root_config.google_workspace.allowed_operations.clone(),
            root_config.google_workspace.credentials_path.clone(),
            root_config.google_workspace.default_account.clone(),
            root_config.google_workspace.rate_limit_per_minute,
            root_config.google_workspace.timeout_secs,
            root_config.google_workspace.audit_log,
        )));
    } else if root_config.google_workspace.enabled {
        tracing::warn!(
            "google_workspace: skipped registration because shell access is unavailable"
        );
    }

    if root_config.claude_code.enabled {
        tool_arcs.push(Arc::new(ClaudeCodeTool::new(
            security.clone(),
            root_config.claude_code.clone(),
        )));
    }

    if root_config.claude_code_runner.enabled {
        let gateway_url = format!(
            "http://{}:{}",
            root_config.gateway.host, root_config.gateway.port
        );
        tool_arcs.push(Arc::new(ClaudeCodeRunnerTool::new(
            security.clone(),
            root_config.claude_code_runner.clone(),
            gateway_url,
        )));
    }

    if root_config.codex_cli.enabled {
        tool_arcs.push(Arc::new(CodexCliTool::new(
            security.clone(),
            root_config.codex_cli.clone(),
        )));
    }

    if root_config.gemini_cli.enabled {
        tool_arcs.push(Arc::new(GeminiCliTool::new(
            security.clone(),
            root_config.gemini_cli.clone(),
        )));
    }

    if root_config.opencode_cli.enabled {
        tool_arcs.push(Arc::new(OpenCodeCliTool::new(
            security.clone(),
            root_config.opencode_cli.clone(),
        )));
    }

    tool_arcs.push(Arc::new(PdfReadTool::new(security.clone())));

    #[cfg(feature = "office-docs")]
    tool_arcs.push(Arc::new(DocumentConvertTool::new(security.clone())));
    #[cfg(feature = "office-docs")]
    tool_arcs.push(Arc::new(PdfOpsTool::new(security.clone())));
    #[cfg(feature = "office-docs")]
    tool_arcs.push(Arc::new(PresentationCreateTool::new(security.clone())));

    #[cfg(feature = "tool-image")]
    tool_arcs.push(Arc::new(ScreenshotTool::new(security.clone())));
    #[cfg(feature = "tool-image")]
    tool_arcs.push(Arc::new(ImageInfoTool::new(security.clone())));

    if let Ok(session_store) = crate::channels::session::store::SessionStore::new(workspace_dir) {
        let backend: Arc<dyn crate::channels::session::backend::SessionBackend> =
            Arc::new(session_store);
        tool_arcs.push(Arc::new(SessionsListTool::new(backend.clone())));
        tool_arcs.push(Arc::new(SessionsHistoryTool::new(
            backend.clone(),
            security.clone(),
        )));
        tool_arcs.push(Arc::new(SessionsOutlineTool::new(
            backend.clone(),
            security.clone(),
        )));
        tool_arcs.push(Arc::new(SessionsSearchTool::new(
            backend.clone(),
            security.clone(),
        )));
        tool_arcs.push(Arc::new(SessionsSendTool::new(backend, security.clone())));
    }

    #[cfg(feature = "tool-linkedin")]
    if root_config.linkedin.enabled {
        tool_arcs.push(Arc::new(LinkedInTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
            root_config.linkedin.api_version.clone(),
            root_config.linkedin.content.clone(),
            root_config.linkedin.image.clone(),
        )));
    }

    #[cfg(feature = "tool-image")]
    if root_config.image_gen.enabled {
        tool_arcs.push(Arc::new(ImageGenTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
            root_config.image_gen.default_model.clone(),
            root_config.image_gen.api_key_env.clone(),
        )));
    }

    tool_arcs.push(Arc::new(MediaGenTool::new(
        security.clone(),
        workspace_dir.to_path_buf(),
    )));
    tool_arcs.push(Arc::new(DesignSystemReadTool::new()));
    tool_arcs.push(Arc::new(DesignerSkillReadTool::new()));
    tool_arcs.push(Arc::new(DesignerTemplateReadTool::new()));
    tool_arcs.push(Arc::new(DesignerLintTool::new()));
    tool_arcs.push(Arc::new(DeckCompileTool::new()));
    tool_arcs.push(Arc::new(DesignerScaffoldTool::new()));
    tool_arcs.push(Arc::new(FigmaFetchTool::new()));

    let channel_map_handle: ChannelMapHandle = Arc::new(RwLock::new(HashMap::new()));
    tool_arcs.push(Arc::new(PollTool::new(
        security.clone(),
        Arc::clone(&channel_map_handle),
    )));

    #[cfg(feature = "tool-sop")]
    if root_config.sop.sops_dir.is_some() {
        let sop_engine = crate::sop::engine::global_sop_engine(&root_config.sop);
        let sop_audit = Arc::new(crate::sop::audit::SopAuditLogger::new(Arc::clone(&memory)));
        crate::sop::dispatch::ensure_sop_maintenance(
            Arc::clone(&sop_engine),
            Some(Arc::clone(&sop_audit)),
            workspace_dir.to_path_buf(),
        );
        tool_arcs.push(Arc::new(SopListTool::new(Arc::clone(&sop_engine))));
        tool_arcs.push(Arc::new(
            SopExecuteTool::new(Arc::clone(&sop_engine)).with_audit(Arc::clone(&sop_audit)),
        ));
        tool_arcs.push(Arc::new(
            SopAdvanceTool::new(Arc::clone(&sop_engine)).with_audit(Arc::clone(&sop_audit)),
        ));
        tool_arcs.push(Arc::new(
            SopApproveTool::new(Arc::clone(&sop_engine)).with_audit(Arc::clone(&sop_audit)),
        ));
        tool_arcs.push(Arc::new(SopStatusTool::new(Arc::clone(&sop_engine))));
    }

    #[cfg(feature = "tool-productivity")]
    if let Some(key) = composio_key {
        if !key.is_empty() {
            tool_arcs.push(Arc::new(ComposioTool::new(
                key,
                composio_entity_id,
                security.clone(),
            )));
        }
    }

    let reaction_tool = ReactionTool::new(security.clone());
    let reaction_handle = reaction_tool.channel_map_handle();
    tool_arcs.push(Arc::new(reaction_tool));

    tool_arcs.push(Arc::new(AskQuestionTool::new()));

    let ask_user_tool = AskUserTool::new(security.clone());
    let ask_user_handle = ask_user_tool.channel_map_handle();
    tool_arcs.push(Arc::new(ask_user_tool));

    let escalate_tool = EscalateToHumanTool::new(security.clone(), workspace_dir.to_path_buf());
    let escalate_handle = escalate_tool.channel_map_handle();
    tool_arcs.push(Arc::new(escalate_tool));

    if root_config.microsoft365.enabled {
        let ms_cfg = &root_config.microsoft365;
        let tenant_id = ms_cfg
            .tenant_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string();
        let client_id = ms_cfg
            .client_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string();
        if !tenant_id.is_empty() && !client_id.is_empty() {
            if ms_cfg.auth_flow.trim() == "client_credentials"
                && ms_cfg
                    .client_secret
                    .as_deref()
                    .map_or(true, |s| s.trim().is_empty())
            {
                tracing::error!(
                    "microsoft365: client_credentials auth_flow requires a non-empty client_secret  -  skipping M365 tool"
                );
            } else {
                let resolved = microsoft365::types::Microsoft365ResolvedConfig {
                    tenant_id,
                    client_id,
                    client_secret: ms_cfg.client_secret.clone(),
                    auth_flow: ms_cfg.auth_flow.clone(),
                    scopes: ms_cfg.scopes.clone(),
                    token_cache_encrypted: ms_cfg.token_cache_encrypted,
                    user_id: ms_cfg.user_id.as_deref().unwrap_or("me").to_string(),
                };
                let cache_dir = root_config.config_path.parent().unwrap_or(workspace_dir);
                match Microsoft365Tool::new(resolved, security.clone(), cache_dir) {
                    Ok(tool) => tool_arcs.push(Arc::new(tool)),
                    Err(e) => {
                        tracing::error!("microsoft365: failed to initialize tool: {e}");
                    }
                }
            }
        } else {
            tracing::warn!(
                "microsoft365: skipped registration because tenant_id or client_id is empty"
            );
        }
    }

    if root_config.knowledge.enabled {
        let db_path_str = root_config.knowledge.db_path.replace(
            '~',
            &directories::UserDirs::new()
                .map(|u| u.home_dir().to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string()),
        );
        let db_path = std::path::PathBuf::from(&db_path_str);
        match crate::memory::knowledge_graph::KnowledgeGraph::new(
            &db_path,
            root_config.knowledge.max_nodes,
        ) {
            Ok(graph) => {
                tool_arcs.push(Arc::new(KnowledgeTool::new(Arc::new(graph))));
            }
            Err(e) => {
                tracing::warn!("knowledge graph disabled due to init error: {e}");
            }
        }
    }

    let delegate_fallback_credential = fallback_api_key.and_then(|value| {
        let trimmed_value = value.trim();
        (!trimmed_value.is_empty()).then(|| trimmed_value.to_owned())
    });
    let provider_runtime_options = crate::providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        provider_api_url: root_config.api_url.clone(),
        sen_dir: root_config
            .config_path
            .parent()
            .map(std::path::PathBuf::from),
        secrets_encrypt: root_config.secrets.encrypt,
        reasoning_enabled: root_config.runtime.reasoning_enabled,
        reasoning_effort: root_config.runtime.reasoning_effort.clone(),
        provider_timeout_secs: Some(root_config.provider_timeout_secs),
        provider_max_tokens: root_config.provider_max_tokens,
        extra_headers: crate::providers::merged_extra_headers_for_config(root_config),
        api_path: root_config.api_path.clone(),
        model_context_windows: root_config.model_context_windows.clone(),
    };

    let delegate_handle: Option<DelegateParentToolsHandle> = if agents.is_empty() {
        None
    } else {
        let delegate_agents: HashMap<String, DelegateAgentConfig> = agents
            .iter()
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect();
        let parent_tools = Arc::new(RwLock::new(tool_arcs.clone()));
        let delegate_tool = DelegateTool::new_with_options(
            delegate_agents,
            delegate_fallback_credential.clone(),
            security.clone(),
            provider_runtime_options.clone(),
        )
        .with_parent_tools(Arc::clone(&parent_tools))
        .with_multimodal_config(root_config.multimodal.clone())
        .with_delegate_config(root_config.delegate.clone())
        .with_workspace_root(security.workspace_root_handle());
        tool_arcs.push(Arc::new(delegate_tool));
        Some(parent_tools)
    };

    let mut delegate_parallel_tool =
        crate::tools::delegate::parallel::DelegateParallelTool::new()
            .with_multimodal_config(root_config.multimodal.clone())
            .with_workspace_root(security.workspace_root_handle())
            .with_delegate_config(root_config.delegate.clone());
    if let Some(ref handle) = delegate_handle {
        delegate_parallel_tool =
            delegate_parallel_tool.with_parent_tools(Arc::clone(handle));
    }
    tool_arcs.push(Arc::new(delegate_parallel_tool));

    {
        let live_cfg = crate::config::live::LiveConfig::new(root_config.clone());
        tool_arcs.push(Arc::new(crate::tools::spawn_workers::SpawnWorkersTool::new(
            Arc::clone(&config),
            Some(live_cfg),
        )));
    }

    if !root_config.swarms.is_empty() {
        let swarm_agents: HashMap<String, DelegateAgentConfig> = agents
            .iter()
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect();
        tool_arcs.push(Arc::new(SwarmTool::new(
            root_config.swarms.clone(),
            swarm_agents,
            delegate_fallback_credential,
            security.clone(),
            provider_runtime_options,
        )));
    }

    if root_config.workspace.enabled {
        let ws_base_dir = if root_config.workspace.workspaces_dir.starts_with("~/") {
            let home = directories::UserDirs::new()
                .map(|u| u.home_dir().to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            home.join(&root_config.workspace.workspaces_dir[2..])
        } else {
            std::path::PathBuf::from(&root_config.workspace.workspaces_dir)
        };
        let ws_manager = crate::config::workspace::WorkspaceManager::new(ws_base_dir);
        tool_arcs.push(Arc::new(WorkspaceTool::new(
            Arc::new(tokio::sync::RwLock::new(ws_manager)),
            security.clone(),
        )));
    }

    if root_config.verifiable_intent.enabled {
        let strictness = match root_config.verifiable_intent.strictness.as_str() {
            "permissive" => crate::verifiable_intent::StrictnessMode::Permissive,
            _ => crate::verifiable_intent::StrictnessMode::Strict,
        };
        tool_arcs.push(Arc::new(VerifiableIntentTool::new(
            security.clone(),
            strictness,
        )));
    }

    #[cfg(feature = "plugins-wasm")]
    {
        let plugin_dir = config.plugins.plugins_dir.clone();
        let plugin_path = if plugin_dir.starts_with("~/") {
            let home = directories::UserDirs::new()
                .map(|u| u.home_dir().to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            home.join(&plugin_dir[2..])
        } else {
            std::path::PathBuf::from(&plugin_dir)
        };

        if plugin_path.exists() && config.plugins.enabled && config.plugins.auto_discover {
            match crate::plugins::host::PluginHost::from_plugins_config(
                plugin_path.parent().unwrap_or(&plugin_path),
                &config.plugins,
            ) {
                Ok(host) => {
                    let tool_specs = host.tool_plugin_specs();
                    let count = tool_specs.len();
                    for (plugin_name, description, wasm_path) in tool_specs {
                        tool_arcs.push(Arc::new(crate::plugins::wasm::tool::WasmTool::new(
                            plugin_name,
                            description.unwrap_or_default(),
                            wasm_path.to_string_lossy().into_owned(),
                            "call".to_string(),
                            serde_json::json!({
                                "type": "object",
                                "properties": {
                                    "input": {
                                        "type": "string",
                                        "description": "Input for the plugin"
                                    }
                                },
                                "required": ["input"]
                            }),
                        )));
                    }
                    tracing::info!("Loaded {count} WASM plugin tools");
                }
                Err(e) => {
                    tracing::warn!("Failed to load WASM plugins: {e}");
                }
            }
        }
    }

    if root_config.pipeline.enabled {
        let pipeline_tools: Vec<Arc<dyn Tool>> = tool_arcs.clone();
        tool_arcs.push(Arc::new(pipeline::PipelineTool::new(
            root_config.pipeline.clone(),
            pipeline_tools,
        )));
    }

    {
        let mut seen: HashSet<String> = HashSet::new();
        for tool in tool_arcs.iter() {
            seen.insert(tool.name().to_string());
        }
        for def in &root_config.custom_tools.tools {
            if !def.enabled {
                continue;
            }
            let validation_errors = def.validate();
            if !validation_errors.is_empty() {
                tracing::warn!(
                    name = %def.name,
                    errors = ?validation_errors,
                    "custom_tools: skipping invalid entry"
                );
                continue;
            }
            let registered = format!("custom_{}", def.name.trim());
            if !seen.insert(registered.clone()) {
                tracing::warn!(
                    name = %registered,
                    "custom_tools: duplicate tool name, skipping later entry"
                );
                continue;
            }
            tool_arcs.push(Arc::new(custom_tool::CustomTool::from_def(
                def,
                security.workspace_root_handle(),
            )));
        }
    }

    if !root_config.tool_groups.groups.is_empty() {
        let group_registry = handler::groups::ToolGroupRegistry::from_config(&root_config.tool_groups);
        let active_names = group_registry.active_tools();
        if !active_names.is_empty() {
            let before = tool_arcs.len();
            tool_arcs.retain(|t| {
                let name = t.name();
                group_registry.is_tool_active(name)
            });
            let after = tool_arcs.len();
            if before != after {
                tracing::info!(
                    "Tool groups: filtered {} → {} tools (active groups: {:?})",
                    before,
                    after,
                    group_registry.active_group_names()
                );
            }
        }
    }

    (
        boxed_registry_from_arcs(tool_arcs),
        delegate_handle,
        Some(reaction_handle),
        channel_map_handle,
        Some(ask_user_handle),
        Some(escalate_handle),
        plan_mode_flag,
    )
}