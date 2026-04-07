// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tool subsystem for agent-callable capabilities.
//!
//! This module implements the tool execution surface exposed to the LLM during
//! agentic loops. Each tool implements the [`Tool`] trait defined in [`traits`],
//! which requires a name, description, JSON parameter schema, and an async
//! `execute` method returning a structured [`ToolResult`].
//!
//! Tools are assembled into registries by [`default_tools`] (shell, file read/write)
//! and [`all_tools`] (full set including memory, browser, cron, HTTP, delegation,
//! and optional integrations). Security policy enforcement is injected via
//! [`SecurityPolicy`](crate::security::SecurityPolicy) at construction time.
//!
//! # Extension
//!
//! To add a new tool, implement [`Tool`] in a new submodule and register it in
//! [`all_tools_with_runtime`]. See `AGENTS.md` §7.3 for the full change playbook.

pub mod ask_user;
pub mod backup_tool;
pub mod brief;
pub mod browser;
pub mod browser_delegate;
pub mod browser_open;
pub mod calculator;
pub mod canvas;
pub mod claude_code;
pub mod claude_code_runner;
pub mod cli_discovery;
pub mod cloud_ops;
pub mod cloud_patterns;
pub mod codex_cli;
pub mod composio;
pub mod code_to_spec;
pub mod content_search;
pub mod incremental_optimize;
pub mod cron_add;
pub mod cron_list;
pub mod cron_remove;
pub mod cron_run;
pub mod cron_runs;
pub mod cron_update;
pub mod data_management;
pub mod delegate;
pub mod diagnostics;
pub mod dir_list;
pub mod discord_search;
pub mod enter_plan_mode;
pub mod escalate;
pub mod exa_search;
pub mod exit_plan_mode;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod fs_ops;
pub mod gemini_cli;
pub mod git_operations;
pub mod github_search;
pub mod glob_edit;
pub mod glob_search;
pub mod google_workspace;
#[cfg(feature = "hardware")]
pub mod hardware_board_info;
#[cfg(feature = "hardware")]
pub mod hardware_memory_map;
#[cfg(feature = "hardware")]
pub mod hardware_memory_read;
pub mod http_request;
pub mod image_gen;
pub mod image_info;
pub mod image_search;
pub mod jira_tool;
pub mod knowledge_tool;
pub mod linkedin;
pub mod linkedin_client;
pub mod llm_task;
pub mod lsp;
pub mod lsp_rename;
pub mod mcp_client;
pub mod mcp_deferred;
pub mod mcp_protocol;
pub mod mcp_resources_list;
pub mod mcp_resources_read;
pub mod mcp_tool;
pub mod mcp_transport;
pub mod memory_export;
pub mod memory_forget;
pub mod memory_purge;
pub mod memory_recall;
pub mod memory_store;
pub mod microsoft365;
pub mod model_routing_config;
pub mod model_switch;
pub mod multi_edit;
pub mod multi_search;
pub mod node_capabilities;
pub mod node_tool;
pub mod now;
pub mod notebook_edit;
pub mod notion_tool;
pub mod opencode_cli;
pub mod patch_apply;
pub mod pdf_read;
pub mod pipeline;
pub mod poll;
pub mod powershell;
pub mod present_files;
pub mod project_intel;
pub mod proxy_config;
pub mod pushover;
pub mod reaction;
pub mod read_skill;
pub mod reddit_search;
pub mod report_template_tool;
pub mod restore_file;
pub mod report_templates;
pub mod schedule;
pub mod schema;
pub mod screenshot;
pub mod security_ops;
pub mod send_message;
pub mod sessions;
pub mod setup_agent;
pub mod shell;
pub mod skill_http;
pub mod skill_tool;
pub mod sleep;
pub mod sop_advance;
pub mod sop_approve;
pub mod sop_execute;
pub mod sop_list;
pub mod sop_status;
pub mod structured_output;
pub mod swarm;
pub mod tavily_search;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod team_create;
pub mod team_delete;
pub mod text_browser;
pub mod todo_write;
pub mod tool_groups;
pub mod update_plan;
pub mod tool_search;
pub mod traits;
pub mod verifiable_intent;
pub mod view_image;
pub mod weather_tool;
pub mod web_fetch;
mod web_search_provider_routing;
pub mod web_search_tool;
pub mod workspace_tool;
pub mod worktree_enter;
pub mod worktree_exit;
pub mod youtube_search;

pub use ask_user::AskUserTool;
pub use backup_tool::BackupTool;
pub use brief::BriefTool;
pub use browser::{BrowserTool, ComputerUseConfig};
#[allow(unused_imports)]
pub use browser_delegate::{BrowserDelegateConfig, BrowserDelegateTool};
pub use browser_open::BrowserOpenTool;
pub use calculator::CalculatorTool;
pub use canvas::{CanvasStore, CanvasTool};
pub use claude_code::ClaudeCodeTool;
pub use claude_code_runner::ClaudeCodeRunnerTool;
pub use cloud_ops::CloudOpsTool;
pub use cloud_patterns::CloudPatternsTool;
pub use codex_cli::CodexCliTool;
pub use composio::ComposioTool;
pub use code_to_spec::CodeToSpecTool;
pub use content_search::ContentSearchTool;
pub use cron_add::CronAddTool;
pub use cron_list::CronListTool;
pub use cron_remove::CronRemoveTool;
pub use cron_run::CronRunTool;
pub use cron_runs::CronRunsTool;
pub use cron_update::CronUpdateTool;
pub use data_management::DataManagementTool;
pub use delegate::DelegateTool;
pub use diagnostics::DiagnosticsTool;
pub use dir_list::DirListTool;
// Re-exported for downstream consumers of background delegation results.
#[allow(unused_imports)]
pub use delegate::{BackgroundDelegateResult, BackgroundTaskStatus};
pub use discord_search::DiscordSearchTool;
pub use enter_plan_mode::{EnterPlanModeTool, PlanModeFlag};
pub use escalate::EscalateToHumanTool;
pub use exa_search::ExaSearchTool;
pub use exit_plan_mode::ExitPlanModeTool;
pub use file_edit::FileEditTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use fs_ops::{CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool};
pub use gemini_cli::GeminiCliTool;
pub use git_operations::GitOperationsTool;
pub use github_search::GitHubSearchTool;
pub use glob_edit::GlobEditTool;
pub use glob_search::GlobSearchTool;
pub use google_workspace::GoogleWorkspaceTool;
#[cfg(feature = "hardware")]
pub use hardware_board_info::HardwareBoardInfoTool;
#[cfg(feature = "hardware")]
pub use hardware_memory_map::HardwareMemoryMapTool;
#[cfg(feature = "hardware")]
pub use hardware_memory_read::HardwareMemoryReadTool;
pub use http_request::HttpRequestTool;
pub use incremental_optimize::IncrementalOptimizeTool;
pub use image_gen::ImageGenTool;
pub use image_info::ImageInfoTool;
pub use image_search::ImageSearchTool;
pub use jira_tool::JiraTool;
pub use knowledge_tool::KnowledgeTool;
pub use linkedin::LinkedInTool;
pub use llm_task::LlmTaskTool;
pub use lsp::LspTool;
pub use lsp_rename::LspRenameTool;
pub use mcp_client::McpRegistry;
pub use mcp_deferred::{ActivatedToolSet, DeferredMcpToolSet};
pub use mcp_resources_list::McpResourcesListTool;
pub use mcp_resources_read::McpResourcesReadTool;
pub use mcp_tool::McpToolWrapper;
pub use memory_export::MemoryExportTool;
pub use memory_forget::MemoryForgetTool;
pub use memory_purge::MemoryPurgeTool;
pub use memory_recall::MemoryRecallTool;
pub use memory_store::MemoryStoreTool;
pub use microsoft365::Microsoft365Tool;
pub use model_routing_config::ModelRoutingConfigTool;
pub use model_switch::ModelSwitchTool;
pub use multi_edit::MultiEditTool;
pub use multi_search::MultiSearchTool;
#[allow(unused_imports)]
pub use node_tool::NodeTool;
pub use now::NowTool;
pub use notebook_edit::NotebookEditTool;
pub use notion_tool::NotionTool;
pub use opencode_cli::OpenCodeCliTool;
pub use pdf_read::PdfReadTool;
pub use patch_apply::PatchApplyTool;
pub use poll::{ChannelMapHandle, PollTool};
pub use powershell::PowerShellTool;
pub use present_files::PresentFilesTool;
pub use project_intel::ProjectIntelTool;
pub use proxy_config::ProxyConfigTool;
pub use pushover::PushoverTool;
pub use reaction::ReactionTool;
pub use read_skill::ReadSkillTool;
pub use reddit_search::RedditSearchTool;
pub use report_template_tool::ReportTemplateTool;
pub use restore_file::RestoreFileTool;
pub use schedule::ScheduleTool;
#[allow(unused_imports)]
pub use schema::{CleaningStrategy, SchemaCleanr};
pub use screenshot::ScreenshotTool;
pub use security_ops::SecurityOpsTool;
pub use send_message::{AgentMailbox, AgentMessage, SendMessageTool};
pub use sessions::{SessionsHistoryTool, SessionsListTool, SessionsSendTool};
pub use setup_agent::SetupAgentTool;
pub use shell::ShellTool;
#[allow(unused_imports)]
pub use skill_http::SkillHttpTool;
#[allow(unused_imports)]
pub use skill_tool::SkillShellTool;
pub use sleep::SleepTool;
pub use sop_advance::SopAdvanceTool;
pub use sop_approve::SopApproveTool;
pub use sop_execute::SopExecuteTool;
pub use sop_list::SopListTool;
pub use sop_status::SopStatusTool;
pub use structured_output::StructuredOutputTool;
pub use swarm::SwarmTool;
pub use task_create::TaskCreateTool;
pub use task_get::TaskGetTool;
pub use task_list::TaskListTool;
pub use task_output::TaskOutputTool;
pub use task_stop::TaskStopTool;
pub use task_update::TaskUpdateTool;
pub use team_create::{TeamCreateTool, TeamInfo, TeamRegistry};
pub use team_delete::TeamDeleteTool;
pub use text_browser::TextBrowserTool;
pub use todo_write::TodoWriteTool;
pub use tool_search::ToolSearchTool;
pub use update_plan::UpdatePlanTool;
pub use traits::Tool;
#[allow(unused_imports)]
pub use traits::{ToolResult, ToolSpec};
pub use verifiable_intent::VerifiableIntentTool;
pub use view_image::ViewImageTool;
pub use weather_tool::WeatherTool;
pub use web_fetch::WebFetchTool;
pub use tavily_search::TavilySearchTool;
pub use web_search_tool::WebSearchTool;
pub use workspace_tool::WorkspaceTool;
pub use worktree_enter::WorktreeEnterTool;
pub use worktree_exit::WorktreeExitTool;
pub use youtube_search::YouTubeSearchTool;

use crate::config::{Config, DelegateAgentConfig};
use crate::memory::Memory;
use crate::runtime::{NativeRuntime, RuntimeAdapter};
use crate::security::{SecurityPolicy, create_sandbox};
pub mod task_types;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
#[allow(unused_imports)]
pub use task_types::{TaskInfo, TaskManager, TaskManagerHandle, TaskState};

/// Shared handle to the delegate tool's parent-tools list.
/// Callers can push additional tools (e.g. MCP wrappers) after construction.
pub type DelegateParentToolsHandle = Arc<RwLock<Vec<Arc<dyn Tool>>>>;

/// Thin wrapper that makes an `Arc<dyn Tool>` usable as `Box<dyn Tool>`.
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

/// Create the default tool registry
pub fn default_tools(security: Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    default_tools_with_runtime(security, Arc::new(NativeRuntime::new()))
}

/// Create the default tool registry with explicit runtime adapter.
pub fn default_tools_with_runtime(
    security: Arc<SecurityPolicy>,
    runtime: Arc<dyn RuntimeAdapter>,
) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ShellTool::new(security.clone(), runtime)),
        Box::new(FileReadTool::new(security.clone())),
        Box::new(FileWriteTool::new(security.clone())),
        Box::new(FileEditTool::new(security.clone())),
        Box::new(NotebookEditTool::new(security.clone())),
        Box::new(GlobSearchTool::new(security.clone())),
        Box::new(GlobEditTool::new(security.clone(), std::env::current_dir().unwrap_or_default())),
        Box::new(LspRenameTool::new(security.clone(), std::env::current_dir().unwrap_or_default())),
        Box::new(PatchApplyTool::new(security.clone(), std::env::current_dir().unwrap_or_default())),
        Box::new(ContentSearchTool::new(security)),
    ]
}

/// Register skill-defined tools into an existing tool registry.
///
/// Converts each skill's `[[tools]]` entries into callable `Tool` implementations
/// and appends them to the registry. Skill tools that would shadow a built-in tool
/// name are skipped with a warning.
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

/// Create full tool registry including memory tools and optional Composio
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

/// Create full tool registry including memory tools and optional Composio.
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
    let sandbox = create_sandbox(&root_config.security);
    let plan_mode_flag: PlanModeFlag = Arc::new(RwLock::new(false));
    let task_manager: TaskManagerHandle = Arc::new(RwLock::new(TaskManager::new()));
    let team_registry: team_create::TeamRegistry = Arc::new(RwLock::new(HashMap::new()));
    let optimization_state: incremental_optimize::OptimizationStateHandle =
        Arc::new(parking_lot::RwLock::new(incremental_optimize::OptimizationState::default()));
    let mut tool_arcs: Vec<Arc<dyn Tool>> = vec![
        Arc::new(
            ShellTool::new_with_sandbox(security.clone(), runtime, sandbox)
                .with_timeout_secs(root_config.shell_tool.timeout_secs),
        ),
        Arc::new(FileReadTool::new(security.clone())),
        Arc::new(FileWriteTool::new(security.clone())),
        Arc::new(FileEditTool::new(security.clone())),
        Arc::new(MultiEditTool::new(security.clone())),
        Arc::new(NotebookEditTool::new(security.clone())),
        Arc::new(GlobSearchTool::new(security.clone())),
        Arc::new(ContentSearchTool::new(security.clone())),
        Arc::new(CronAddTool::new(config.clone(), security.clone())),
        Arc::new(CronListTool::new(config.clone())),
        Arc::new(CronRemoveTool::new(config.clone(), security.clone())),
        Arc::new(CronUpdateTool::new(config.clone(), security.clone())),
        Arc::new(CronRunTool::new(config.clone(), security.clone())),
        Arc::new(CronRunsTool::new(config.clone())),
        Arc::new(MemoryStoreTool::new(memory.clone(), security.clone())),
        Arc::new(MemoryRecallTool::new(memory.clone())),
        Arc::new(MemoryForgetTool::new(memory.clone(), security.clone())),
        Arc::new(MemoryExportTool::new(memory.clone())),
        Arc::new(MemoryPurgeTool::new(memory, security.clone())),
        Arc::new(ScheduleTool::new(security.clone(), root_config.clone())),
        Arc::new(ModelRoutingConfigTool::new(
            config.clone(),
            security.clone(),
        )),
        Arc::new(ModelSwitchTool::new(security.clone())),
        Arc::new(ProxyConfigTool::new(config.clone(), security.clone())),
        Arc::new(GitOperationsTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
        )),
        Arc::new(CodeToSpecTool::new(workspace_dir.to_path_buf())),
        Arc::new(IncrementalOptimizeTool::new(
            optimization_state.clone(),
            workspace_dir.to_path_buf(),
        )),
        Arc::new(PushoverTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
        )),
        Arc::new(CalculatorTool::new()),
        Arc::new(LspTool::new(workspace_dir.to_path_buf())),
        Arc::new(StructuredOutputTool::new(None)),
        Arc::new(McpResourcesListTool::new()),
        Arc::new(McpResourcesReadTool::new(None)),
        Arc::new(EnterPlanModeTool::new(Arc::clone(&plan_mode_flag))),
        {
            let pending_plan = if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
                svc.pending_plan.clone()
            } else {
                crate::tools::exit_plan_mode::new_pending_plan()
            };
            Arc::new(ExitPlanModeTool::new_with_pending_plan(
                Arc::clone(&plan_mode_flag),
                pending_plan,
            ))
        },
        Arc::new(SleepTool::new()),
        Arc::new(WeatherTool::new()),
        Arc::new(CanvasTool::new(canvas_store.unwrap_or_default())),
        Arc::new(TodoWriteTool::new(Arc::new(RwLock::new(Vec::new())))),
        Arc::new(NowTool::new()),
        Arc::new(CopyPathTool::new(security.clone())),
        Arc::new(MovePathTool::new(security.clone())),
        Arc::new(DeletePathTool::new(security.clone())),
        Arc::new(CreateDirectoryTool::new(security.clone())),
        Arc::new(RestoreFileTool::new(security.clone())),
        Arc::new(DiagnosticsTool::new(workspace_dir.to_path_buf())),
        Arc::new(UpdatePlanTool::new(Arc::new(RwLock::new(Vec::new())))),
        Arc::new(BriefTool::new()),
        Arc::new(TaskCreateTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskGetTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskUpdateTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskListTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskOutputTool::new(Arc::clone(&task_manager))),
        Arc::new(TaskStopTool::new(Arc::clone(&task_manager))),
        Arc::new(TeamCreateTool::new(Arc::clone(&team_registry))),
        Arc::new(TeamDeleteTool::new(Arc::clone(&team_registry))),
        Arc::new(SendMessageTool::new(
            Arc::new(RwLock::new(std::collections::HashMap::new())),
            "main".to_string(),
        )),
        Arc::new(DirListTool::new(security.clone())),
        Arc::new(PresentFilesTool::new(security.clone())),
        Arc::new(ViewImageTool::new(security.clone())),
        Arc::new(ImageSearchTool::new(
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
        )),
        Arc::new(SetupAgentTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
        )),
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
        Arc::new(YouTubeSearchTool::new(
            std::env::var("YOUTUBE_API_KEY").ok(),
            5,
            root_config.web_search.timeout_secs,
        )),
        Arc::new(GitHubSearchTool::from_env(
            root_config.web_search.timeout_secs,
        )),
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

    #[cfg(target_os = "windows")]
    tool_arcs.push(Arc::new(PowerShellTool::new(security.clone())));
    tool_arcs.push(Arc::new(WorktreeEnterTool::new(security.clone())));
    tool_arcs.push(Arc::new(WorktreeExitTool::new(security.clone())));

    // Register discord_search if discord_history channel is configured
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

    // LLM task tool — always registered when a provider is configured
    {
        let llm_task_provider = root_config
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".to_string());
        let llm_task_model = root_config
            .default_model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o-mini".to_string());
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
            extra_headers: root_config.extra_headers.clone(),
            api_path: root_config.api_path.clone(),
            provider_max_tokens: root_config.provider_max_tokens,
        };
        tool_arcs.push(Arc::new(LlmTaskTool::new(
            security.clone(),
            llm_task_provider,
            llm_task_model,
            root_config.default_temperature,
            root_config.api_key.clone(),
            llm_task_runtime_options,
        )));
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

    if browser_config.enabled {
        // Add legacy browser_open tool for simple URL opening
        tool_arcs.push(Arc::new(BrowserOpenTool::new(
            security.clone(),
            browser_config.allowed_domains.clone(),
        )));
        // Add full browser automation tool (pluggable backend)
        tool_arcs.push(Arc::new(BrowserTool::new_with_backend(
            security.clone(),
            browser_config.allowed_domains.clone(),
            browser_config.session_name.clone(),
            browser_config.backend.clone(),
            browser_config.native_headless,
            browser_config.native_webdriver_url.clone(),
            browser_config.native_chrome_path.clone(),
            ComputerUseConfig {
                endpoint: browser_config.computer_use.endpoint.clone(),
                api_key: browser_config.computer_use.api_key.clone(),
                timeout_ms: browser_config.computer_use.timeout_ms,
                allow_remote_endpoint: browser_config.computer_use.allow_remote_endpoint,
                window_allowlist: browser_config.computer_use.window_allowlist.clone(),
                max_coordinate_x: browser_config.computer_use.max_coordinate_x,
                max_coordinate_y: browser_config.computer_use.max_coordinate_y,
            },
        )));
    }

    // Browser delegation tool (conditionally registered; requires shell access)
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

    if web_fetch_config.enabled {
        tool_arcs.push(Arc::new(WebFetchTool::new(
            security.clone(),
            web_fetch_config.allowed_domains.clone(),
            web_fetch_config.blocked_domains.clone(),
            web_fetch_config.max_response_size,
            web_fetch_config.timeout_secs,
            web_fetch_config.firecrawl.clone(),
            web_fetch_config.allowed_private_hosts.clone(),
        )));
    }

    // Text browser tool (headless text-based browser rendering)
    if root_config.text_browser.enabled {
        tool_arcs.push(Arc::new(TextBrowserTool::new(
            security.clone(),
            root_config.text_browser.preferred_browser.clone(),
            root_config.text_browser.timeout_secs,
        )));
    }

    // Web search tool (enabled by default for GLM and other models)
    if root_config.web_search.enabled {
        tool_arcs.push(Arc::new(WebSearchTool::new_with_config(
            root_config.web_search.provider.clone(),
            root_config.web_search.brave_api_key.clone(),
            root_config.web_search.searxng_instance_url.clone(),
            root_config.web_search.max_results,
            root_config.web_search.timeout_secs,
            root_config.config_path.clone(),
            root_config.secrets.encrypt,
        )));
    }

    // Notion API tool (conditionally registered)
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

    // Jira integration (config-gated)
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
            tracing::warn!("Jira tool enabled but jira.base_url is empty — skipping registration");
        } else if root_config.jira.email.trim().is_empty() {
            tracing::warn!("Jira tool enabled but jira.email is empty — skipping registration");
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

    // Project delivery intelligence
    if root_config.project_intel.enabled {
        tool_arcs.push(Arc::new(ProjectIntelTool::new(
            root_config.project_intel.default_language.clone(),
            root_config.project_intel.risk_sensitivity.clone(),
        )));
        // Report template tool — direct access to template engine
        tool_arcs.push(Arc::new(ReportTemplateTool::new()));
    }

    // MCSS Security Operations
    if root_config.security_ops.enabled {
        tool_arcs.push(Arc::new(SecurityOpsTool::new(
            root_config.security_ops.clone(),
        )));
    }

    // Backup tool (enabled by default)
    if root_config.backup.enabled {
        tool_arcs.push(Arc::new(BackupTool::new(
            workspace_dir.to_path_buf(),
            root_config.backup.include_dirs.clone(),
            root_config.backup.max_keep,
        )));
    }

    // Data management tool (disabled by default)
    if root_config.data_retention.enabled {
        tool_arcs.push(Arc::new(DataManagementTool::new(
            workspace_dir.to_path_buf(),
            root_config.data_retention.retention_days,
        )));
    }

    // Cloud operations advisory tools (read-only analysis)
    if root_config.cloud_ops.enabled {
        tool_arcs.push(Arc::new(CloudOpsTool::new(root_config.cloud_ops.clone())));
        tool_arcs.push(Arc::new(CloudPatternsTool::new()));
    }

    // Google Workspace CLI (gws) integration — requires shell access
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

    // Claude Code delegation tool
    if root_config.claude_code.enabled {
        tool_arcs.push(Arc::new(ClaudeCodeTool::new(
            security.clone(),
            root_config.claude_code.clone(),
        )));
    }

    // Claude Code task runner with Slack progress and SSH handoff
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

    // Codex CLI delegation tool
    if root_config.codex_cli.enabled {
        tool_arcs.push(Arc::new(CodexCliTool::new(
            security.clone(),
            root_config.codex_cli.clone(),
        )));
    }

    // Gemini CLI delegation tool
    if root_config.gemini_cli.enabled {
        tool_arcs.push(Arc::new(GeminiCliTool::new(
            security.clone(),
            root_config.gemini_cli.clone(),
        )));
    }

    // OpenCode CLI delegation tool
    if root_config.opencode_cli.enabled {
        tool_arcs.push(Arc::new(OpenCodeCliTool::new(
            security.clone(),
            root_config.opencode_cli.clone(),
        )));
    }

    // PDF extraction (feature-gated at compile time via rag-pdf)
    tool_arcs.push(Arc::new(PdfReadTool::new(security.clone())));

    // Vision tools are always available
    tool_arcs.push(Arc::new(ScreenshotTool::new(security.clone())));
    tool_arcs.push(Arc::new(ImageInfoTool::new(security.clone())));

    // Session-to-session messaging tools (always available when sessions dir exists)
    if let Ok(session_store) = crate::channels::session_store::SessionStore::new(workspace_dir) {
        let backend: Arc<dyn crate::channels::session_backend::SessionBackend> =
            Arc::new(session_store);
        tool_arcs.push(Arc::new(SessionsListTool::new(backend.clone())));
        tool_arcs.push(Arc::new(SessionsHistoryTool::new(
            backend.clone(),
            security.clone(),
        )));
        tool_arcs.push(Arc::new(SessionsSendTool::new(backend, security.clone())));
    }

    // LinkedIn integration (config-gated)
    if root_config.linkedin.enabled {
        tool_arcs.push(Arc::new(LinkedInTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
            root_config.linkedin.api_version.clone(),
            root_config.linkedin.content.clone(),
            root_config.linkedin.image.clone(),
        )));
    }

    // Standalone image generation tool (config-gated)
    if root_config.image_gen.enabled {
        tool_arcs.push(Arc::new(ImageGenTool::new(
            security.clone(),
            workspace_dir.to_path_buf(),
            root_config.image_gen.default_model.clone(),
            root_config.image_gen.api_key_env.clone(),
        )));
    }

    // Poll tool — always registered; uses late-bound channel map handle
    let channel_map_handle: ChannelMapHandle = Arc::new(RwLock::new(HashMap::new()));
    tool_arcs.push(Arc::new(PollTool::new(
        security.clone(),
        Arc::clone(&channel_map_handle),
    )));

    // SOP tools (registered when sops_dir is configured)
    if root_config.sop.sops_dir.is_some() {
        let sop_engine = Arc::new(std::sync::Mutex::new(crate::sop::SopEngine::new(
            root_config.sop.clone(),
        )));
        tool_arcs.push(Arc::new(SopListTool::new(Arc::clone(&sop_engine))));
        tool_arcs.push(Arc::new(SopExecuteTool::new(Arc::clone(&sop_engine))));
        tool_arcs.push(Arc::new(SopAdvanceTool::new(Arc::clone(&sop_engine))));
        tool_arcs.push(Arc::new(SopApproveTool::new(Arc::clone(&sop_engine))));
        tool_arcs.push(Arc::new(SopStatusTool::new(Arc::clone(&sop_engine))));
    }

    if let Some(key) = composio_key {
        if !key.is_empty() {
            tool_arcs.push(Arc::new(ComposioTool::new(
                key,
                composio_entity_id,
                security.clone(),
            )));
        }
    }

    // Emoji reaction tool — always registered; channel map populated later by start_channels.
    let reaction_tool = ReactionTool::new(security.clone());
    let reaction_handle = reaction_tool.channel_map_handle();
    tool_arcs.push(Arc::new(reaction_tool));

    // Interactive ask_user tool — always registered; channel map populated later by start_channels.
    let ask_user_tool = AskUserTool::new(security.clone());
    let ask_user_handle = ask_user_tool.channel_map_handle();
    tool_arcs.push(Arc::new(ask_user_tool));

    // Human escalation tool — always registered; channel map populated later by start_channels.
    let escalate_tool = EscalateToHumanTool::new(security.clone(), workspace_dir.to_path_buf());
    let escalate_handle = escalate_tool.channel_map_handle();
    tool_arcs.push(Arc::new(escalate_tool));

    // Microsoft 365 Graph API integration
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
                    "microsoft365: client_credentials auth_flow requires a non-empty client_secret — skipping M365 tool"
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

    // Knowledge graph tool
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

    // Add delegation tool when agents are configured
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
        extra_headers: root_config.extra_headers.clone(),
        api_path: root_config.api_path.clone(),
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
        .with_workspace_dir(workspace_dir.to_path_buf());
        tool_arcs.push(Arc::new(delegate_tool));
        Some(parent_tools)
    };

    // Add swarm tool when swarms are configured
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

    // Workspace management tool (conditionally registered when workspace isolation is enabled)
    if root_config.workspace.enabled {
        let workspaces_dir = if root_config.workspace.workspaces_dir.starts_with("~/") {
            let home = directories::UserDirs::new()
                .map(|u| u.home_dir().to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            home.join(&root_config.workspace.workspaces_dir[2..])
        } else {
            std::path::PathBuf::from(&root_config.workspace.workspaces_dir)
        };
        let ws_manager = crate::config::workspace::WorkspaceManager::new(workspaces_dir);
        tool_arcs.push(Arc::new(WorkspaceTool::new(
            Arc::new(tokio::sync::RwLock::new(ws_manager)),
            security.clone(),
        )));
    }

    // Verifiable Intent tool (opt-in via config)
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

    // ── WASM plugin tools (requires plugins-wasm feature) ──
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

        if plugin_path.exists() && config.plugins.enabled {
            match crate::plugins::host::PluginHost::new(
                plugin_path.parent().unwrap_or(&plugin_path),
            ) {
                Ok(host) => {
                    let tool_manifests = host.tool_plugins();
                    let count = tool_manifests.len();
                    for manifest in tool_manifests {
                        tool_arcs.push(Arc::new(crate::plugins::wasm_tool::WasmTool::new(
                            manifest.name.clone(),
                            manifest.description.clone().unwrap_or_default(),
                            manifest.name.clone(),
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

    // Pipeline tool (execute_pipeline) — multi-step tool chaining.
    if root_config.pipeline.enabled {
        let pipeline_tools: Vec<Arc<dyn Tool>> = tool_arcs.clone();
        tool_arcs.push(Arc::new(pipeline::PipelineTool::new(
            root_config.pipeline.clone(),
            pipeline_tools,
        )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BrowserConfig, Config, MemoryConfig};
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> Config {
        Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        }
    }

    #[test]
    fn default_tools_has_expected_count() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(security);
        assert_eq!(tools.len(), 6);
    }

    #[test]
    fn all_tools_excludes_browser_when_disabled() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig {
            enabled: false,
            allowed_domains: vec!["example.com".into()],
            session_name: None,
            ..BrowserConfig::default()
        };
        let http = crate::config::HttpRequestConfig::default();
        let cfg = test_config(&tmp);

        let (tools, _, _, _, _, _, _) = all_tools(
            Arc::new(Config::default()),
            &security,
            mem,
            None,
            None,
            &browser,
            &http,
            &crate::config::WebFetchConfig::default(),
            tmp.path(),
            &HashMap::new(),
            None,
            &cfg,
            None,
        );
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"browser_open"));
        assert!(names.contains(&"schedule"));
        assert!(names.contains(&"model_routing_config"));
        assert!(names.contains(&"pushover"));
        assert!(names.contains(&"proxy_config"));
    }

    #[test]
    fn all_tools_includes_browser_when_enabled() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig {
            enabled: true,
            allowed_domains: vec!["example.com".into()],
            session_name: None,
            ..BrowserConfig::default()
        };
        let http = crate::config::HttpRequestConfig::default();
        let cfg = test_config(&tmp);

        let (tools, _, _, _, _, _, _) = all_tools(
            Arc::new(Config::default()),
            &security,
            mem,
            None,
            None,
            &browser,
            &http,
            &crate::config::WebFetchConfig::default(),
            tmp.path(),
            &HashMap::new(),
            None,
            &cfg,
            None,
        );
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"browser_open"));
        assert!(names.contains(&"content_search"));
        assert!(names.contains(&"model_routing_config"));
        assert!(names.contains(&"pushover"));
        assert!(names.contains(&"proxy_config"));
    }

    #[test]
    fn default_tools_names() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(security);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"file_edit"));
        assert!(names.contains(&"notebook_edit"));
        assert!(names.contains(&"glob_search"));
        assert!(names.contains(&"content_search"));
    }

    #[test]
    fn default_tools_all_have_descriptions() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(security);
        for tool in &tools {
            assert!(
                !tool.description().is_empty(),
                "Tool {} has empty description",
                tool.name()
            );
        }
    }

    #[test]
    fn default_tools_all_have_schemas() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(security);
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "Tool {} schema is not an object",
                tool.name()
            );
            assert!(
                schema["properties"].is_object(),
                "Tool {} schema has no properties",
                tool.name()
            );
        }
    }

    #[test]
    fn tool_spec_generation() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(security);
        for tool in &tools {
            let spec = tool.spec();
            assert_eq!(spec.name, tool.name());
            assert_eq!(spec.description, tool.description());
            assert!(spec.parameters.is_object());
        }
    }

    #[test]
    fn tool_result_serde() {
        let result = ToolResult {
            success: true,
            output: "hello".into(),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.output, "hello");
        assert!(parsed.error.is_none());
    }

    #[test]
    fn tool_result_with_error_serde() {
        let result = ToolResult {
            success: false,
            output: String::new(),
            error: Some("boom".into()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(!parsed.success);
        assert_eq!(parsed.error.as_deref(), Some("boom"));
    }

    #[test]
    fn tool_spec_serde() {
        let spec = ToolSpec {
            name: "test".into(),
            description: "A test tool".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: ToolSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.description, "A test tool");
    }

    #[test]
    fn all_tools_includes_delegate_when_agents_configured() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let http = crate::config::HttpRequestConfig::default();
        let cfg = test_config(&tmp);

        let mut agents = HashMap::new();
        agents.insert(
            "researcher".to_string(),
            DelegateAgentConfig {
                provider: "ollama".to_string(),
                model: "llama3".to_string(),
                system_prompt: None,
                api_key: None,
                temperature: None,
                max_depth: 3,
                agentic: false,
                allowed_tools: Vec::new(),
                max_iterations: 10,
                timeout_secs: None,
                agentic_timeout_secs: None,
                skills_directory: None,
            },
        );

        let (tools, _, _, _, _, _, _) = all_tools(
            Arc::new(Config::default()),
            &security,
            mem,
            None,
            None,
            &browser,
            &http,
            &crate::config::WebFetchConfig::default(),
            tmp.path(),
            &agents,
            Some("delegate-test-credential"),
            &cfg,
            None,
        );
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"delegate"));
    }

    #[test]
    fn all_tools_excludes_delegate_when_no_agents() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let http = crate::config::HttpRequestConfig::default();
        let cfg = test_config(&tmp);

        let (tools, _, _, _, _, _, _) = all_tools(
            Arc::new(Config::default()),
            &security,
            mem,
            None,
            None,
            &browser,
            &http,
            &crate::config::WebFetchConfig::default(),
            tmp.path(),
            &HashMap::new(),
            None,
            &cfg,
            None,
        );
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"delegate"));
    }

    #[test]
    fn all_tools_includes_read_skill_in_compact_mode() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let http = crate::config::HttpRequestConfig::default();
        let mut cfg = test_config(&tmp);
        cfg.skills.prompt_injection_mode = crate::config::SkillsPromptInjectionMode::Compact;

        let (tools, _, _, _, _, _, _) = all_tools(
            Arc::new(cfg.clone()),
            &security,
            mem,
            None,
            None,
            &browser,
            &http,
            &crate::config::WebFetchConfig::default(),
            tmp.path(),
            &HashMap::new(),
            None,
            &cfg,
            None,
        );
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"read_skill"));
    }

    #[test]
    fn all_tools_excludes_read_skill_in_full_mode() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let http = crate::config::HttpRequestConfig::default();
        let mut cfg = test_config(&tmp);
        cfg.skills.prompt_injection_mode = crate::config::SkillsPromptInjectionMode::Full;

        let (tools, _, _, _, _, _, _) = all_tools(
            Arc::new(cfg.clone()),
            &security,
            mem,
            None,
            None,
            &browser,
            &http,
            &crate::config::WebFetchConfig::default(),
            tmp.path(),
            &HashMap::new(),
            None,
            &cfg,
            None,
        );
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"read_skill"));
    }
}
