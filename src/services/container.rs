// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// ServiceContainer — centralized service initialization and access.
// Wires all services ported from claude-code-typescript-srcinto a single dependency-injectable
// container that the agent core, commands, hooks, and TUI can consume.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use super::agent_summary::AgentSummaryService;
use super::analytics::AnalyticsService;
use super::auto_dream::AutoDreamService;
use super::compact::CompactService;
use super::extract_memories::ExtractionConfig;
use super::lsp::LspService;
use super::mcp_manager::McpManager;
use super::notifier::Notifier;
use super::oauth::OAuthService;
use super::plugin_service::PluginService;
use super::policy_limits::{PolicyLimitsService, PolicyRule};
use super::prompt_suggestion::PromptSuggestionService;
use super::rate_limit::RateLimiter;
use super::session_memory::SessionMemoryService;
use super::settings_sync::{ConflictStrategy, SettingsSyncService};
use super::team_memory_sync::TeamMemorySyncService;
use super::token_estimation::TokenEstimator;
use super::tool_use_summary::ToolUseSummaryService;

use crate::agent::coding_mode::CodingModeHandle;
use crate::tools::exit_plan_mode::PendingPlan;
use crate::commands::registry::CommandRegistry;
use crate::tasks::runner::TaskRunner;

/// All services wired together for the agent runtime.
///
/// Core services are actively used by the agent loop and command handlers.
/// Extension services are available for plugins, channels, and future features.
pub struct ServiceContainer {
    // -- Core services (actively wired) --
    pub analytics: AnalyticsService,
    pub compact: CompactService,
    pub lsp: LspService,
    pub mcp: McpManager,

    /// Rate limiter for API call throttling. Used by the agent loop to enforce
    /// per-provider request limits before making LLM calls. Register buckets
    /// via `rate_limiter.register(key, window, max_requests)`.
    pub rate_limiter: RateLimiter,

    /// Session-scoped memory for tracking user preferences, project context,
    /// decisions, and error patterns within a single session. Wired into the
    /// agent loop for context enrichment via `build_memory_prompt()`.
    pub session_memory: SessionMemoryService,

    /// Fast approximate token counter for budget management. Used by the agent
    /// loop to estimate context size before sending messages to the provider.
    pub token_estimator: TokenEstimator,

    // -- Extension services (available for plugins and future features) --

    /// Desktop/system notification service. Available for channels and plugins
    /// to send OS-level notifications (e.g., task completion, alerts).
    pub notifier: Notifier,

    /// OAuth token management service. Available for integrations that require
    /// OAuth2 flows (e.g., GitHub, Google, Slack).
    pub oauth: OAuthService,

    /// Periodic session summary generator. Available for plugins to produce
    /// post-session reports or analytics digests.
    pub agent_summary: AgentSummaryService,

    /// Background "dream" processing service. When enabled, performs
    /// asynchronous memory consolidation and insight extraction between turns.
    pub auto_dream: AutoDreamService,

    /// Configuration for automatic memory extraction from conversation turns.
    /// Controls which categories of information are auto-extracted into memory.
    pub extraction_config: ExtractionConfig,

    pub plugin_service: PluginService,
    pub policy_limits: PolicyLimitsService,

    /// Contextual prompt suggestion engine. Available for TUI and channels
    /// to suggest follow-up prompts based on conversation history.
    pub prompt_suggestion: PromptSuggestionService,

    /// Cross-device settings synchronization service. Keeps agent configuration
    /// consistent across multiple machines via the configured sync backend.
    pub settings_sync: SettingsSyncService,

    /// Team-shared memory synchronization. When enabled, syncs memory entries
    /// across team members for shared project context.
    pub team_memory_sync: TeamMemorySyncService,

    pub tool_use_summary: Arc<std::sync::Mutex<ToolUseSummaryService>>,

    // -- Command & task systems --
    pub command_registry: CommandRegistry,
    pub task_runner: TaskRunner,

    /// Active coding mode — shared across loop, commands, and TUI.
    pub coding_mode: CodingModeHandle,

    /// Pending plan content from Plan-to-Build auto-continue.
    pub pending_plan: PendingPlan,

    /// Model context window size (tokens). Used by ContextEng mode to display
    /// budget status. Defaults to 128 000; callers should update from agent config.
    pub max_context_tokens: AtomicUsize,
}

/// Configuration for building a ServiceContainer.
pub struct ServiceContainerConfig {
    pub data_dir: PathBuf,
    pub auto_dream_enabled: bool,
    pub team_sync_enabled: bool,
    pub policy_rules: Vec<PolicyRule>,
    pub conflict_strategy: ConflictStrategy,
}

impl Default for ServiceContainerConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".senweavercoding"),
            auto_dream_enabled: false,
            team_sync_enabled: false,
            policy_rules: Vec::new(),
            conflict_strategy: ConflictStrategy::LastWriterWins,
        }
    }
}

impl ServiceContainer {
    /// Build and initialize all services.
    pub fn new(cfg: ServiceContainerConfig) -> Self {
        let sync_file = cfg.data_dir.join("settings_sync.json");

        // Build the command registry with all slash commands registered
        let command_registry = register_all_commands();

        Self {
            analytics: AnalyticsService::new(true),
            compact: CompactService,
            lsp: LspService::new(),
            mcp: McpManager::new(),
            notifier: Notifier::new(),
            oauth: OAuthService::new(),
            rate_limiter: RateLimiter::new(),
            session_memory: SessionMemoryService::new(),
            token_estimator: TokenEstimator::new(4.0),

            agent_summary: AgentSummaryService,
            auto_dream: AutoDreamService::new(cfg.auto_dream_enabled),
            extraction_config: ExtractionConfig::default(),
            plugin_service: PluginService::new(),
            policy_limits: PolicyLimitsService::new(cfg.policy_rules),
            prompt_suggestion: PromptSuggestionService,
            settings_sync: SettingsSyncService::new(sync_file, cfg.conflict_strategy),
            team_memory_sync: TeamMemorySyncService::new(cfg.team_sync_enabled),
            tool_use_summary: Arc::new(std::sync::Mutex::new(ToolUseSummaryService::new())),

            command_registry,
            task_runner: TaskRunner::new(),
            coding_mode: crate::agent::coding_mode::new_coding_mode_handle(),
            pending_plan: crate::tools::exit_plan_mode::new_pending_plan(),
            max_context_tokens: AtomicUsize::new(128_000),
        }
    }

    /// Update the context window budget from agent config (call once after init).
    pub fn set_max_context_tokens(&self, tokens: usize) {
        self.max_context_tokens.store(tokens, Ordering::Relaxed);
    }

    /// Read the context window budget.
    pub fn get_max_context_tokens(&self) -> usize {
        self.max_context_tokens.load(Ordering::Relaxed)
    }

    /// Check if a slash command exists.
    pub fn has_command(&self, name: &str) -> bool {
        self.command_registry.find(name).is_some()
    }

    /// Check if a tool is allowed by policy.
    pub fn check_tool_policy(&self, tool_name: &str) -> bool {
        self.policy_limits.check_tool(tool_name).allowed
    }

    /// Check if a model is allowed by policy.
    pub fn check_model_policy(&self, model_id: &str) -> bool {
        self.policy_limits.check_model(model_id).allowed
    }

    /// Check if spending is within limits (in USD cents).
    pub fn check_spending_policy(&self, current_cents: u64) -> bool {
        self.policy_limits.check_spending(current_cents).allowed
    }
}

// ---------------------------------------------------------------------------
// Global singleton (optional — for code that cannot take &ServiceContainer)
// ---------------------------------------------------------------------------

static GLOBAL_SERVICES: OnceLock<ServiceContainer> = OnceLock::new();

/// Initialize the global service container. Call once from main.
pub fn init_services(cfg: ServiceContainerConfig) -> &'static ServiceContainer {
    GLOBAL_SERVICES.get_or_init(|| ServiceContainer::new(cfg))
}

/// Access the global service container (panics if not initialized).
pub fn get_services() -> &'static ServiceContainer {
    GLOBAL_SERVICES
        .get()
        .expect("ServiceContainer not initialized — call init_services() first")
}

// ---------------------------------------------------------------------------
// Command registration — wires all command handlers
// ---------------------------------------------------------------------------

/// Build the full slash-command registry (same commands as `ServiceContainer::new`).
pub fn register_all_commands() -> CommandRegistry {
    use crate::commands::registry::{CommandCategory, SlashCommand};
    use std::sync::Arc;

    let mut registry = CommandRegistry::new();

    macro_rules! register_cmd {
        // Aliases arm must precede the generic `$desc` arm so `["perms"]` is not parsed as description.
        ($name:expr, [$($alias:expr),*], $desc:expr, $usage:expr, $cat:expr, $handler:path) => {
            registry.register(SlashCommand {
                name: $name.to_string(),
                aliases: vec![$($alias.to_string()),*],
                description: $desc.to_string(),
                usage: $usage.to_string(),
                category: $cat,
                hidden: false,
                requires_interactive: false,
                remote_safe: true,
                handler: Arc::new(|ctx| Box::pin($handler(ctx))),
            });
        };
        ($name:expr, $desc:expr, $usage:expr, $cat:expr, $handler:path) => {
            registry.register(SlashCommand {
                name: $name.to_string(),
                aliases: Vec::new(),
                description: $desc.to_string(),
                usage: $usage.to_string(),
                category: $cat,
                hidden: false,
                requires_interactive: false,
                remote_safe: true,
                handler: Arc::new(|ctx| Box::pin($handler(ctx))),
            });
        };
        ($name:expr, $desc:expr, $usage:expr, $cat:expr, $handler:path, interactive) => {
            registry.register(SlashCommand {
                name: $name.to_string(),
                aliases: Vec::new(),
                description: $desc.to_string(),
                usage: $usage.to_string(),
                category: $cat,
                hidden: false,
                requires_interactive: true,
                remote_safe: false,
                handler: Arc::new(|ctx| Box::pin($handler(ctx))),
            });
        };
    }

    use CommandCategory::*;

    register_cmd!(
        "add-dir",
        "Add a directory to context",
        "/add-dir <path>",
        General,
        crate::commands::add_dir::handle
    );
    register_cmd!(
        "clear",
        "Clear the terminal",
        "/clear",
        Session,
        crate::commands::clear::handle,
        interactive
    );
    register_cmd!(
        "color",
        "Set color mode (auto, always, never)",
        "/color <auto|always|never>",
        Configuration,
        crate::commands::color::handle
    );
    register_cmd!(
        "compact",
        "Compact conversation",
        "/compact [prompt]",
        Session,
        crate::commands::compact::handle
    );
    register_cmd!(
        "config",
        "View or modify config",
        "/config <subcommand>",
        Configuration,
        crate::commands::config_cmd::handle
    );
    register_cmd!(
        "context",
        "Show context usage",
        "/context",
        General,
        crate::commands::context::handle
    );
    register_cmd!(
        "cost",
        "Show session cost",
        "/cost",
        General,
        crate::commands::cost::handle
    );
    register_cmd!(
        "diff",
        "Show pending git changes in the workspace",
        "/diff [git diff args]",
        General,
        crate::commands::diff::handle
    );
    register_cmd!(
        "doctor",
        "Run diagnostics",
        "/doctor",
        Debug,
        crate::commands::doctor_cmd::handle
    );
    register_cmd!(
        "effort",
        "Set the reasoning effort level (low, medium, high)",
        "/effort <low|medium|high>",
        Configuration,
        crate::commands::effort::handle
    );
    register_cmd!(
        "export",
        "Export the current session transcript",
        "/export [path]",
        Session,
        crate::commands::export::handle
    );
    register_cmd!(
        "fast",
        "Switch to a faster, cheaper model for simple tasks",
        "/fast",
        Configuration,
        crate::commands::fast::handle
    );
    register_cmd!(
        "help",
        "Show help",
        "/help [command]",
        General,
        crate::commands::help::handle
    );
    register_cmd!(
        "history",
        "Manage conversation history",
        "/history <subcommand>",
        Session,
        crate::commands::history::handle
    );
    register_cmd!(
        "hooks",
        "List and manage session hooks",
        "/hooks [list|add|remove] [args]",
        Configuration,
        crate::commands::hooks::handle
    );
    register_cmd!(
        "memory",
        "Manage memories",
        "/memory <subcommand>",
        Memory,
        crate::commands::memory_cmd::handle
    );
    register_cmd!(
        "model",
        "Switch or show model",
        "/model [name]",
        Configuration,
        crate::commands::model::handle
    );
    register_cmd!(
        "permissions",
        ["perms"],
        "Show current permission settings",
        "/permissions [subcommand]",
        Configuration,
        crate::commands::permissions::handle
    );
    register_cmd!(
        "plan",
        "Switch to plan mode (alias for /mode plan)",
        "/plan",
        Session,
        crate::commands::plan::handle
    );
    register_cmd!(
        "mode",
        "Switch coding mode (vibe, agent, spec, plan, ask, tdd, debug, architect, pair, context, mvai)",
        "/mode [name]",
        Session,
        crate::commands::mode::handle
    );
    register_cmd!(
        "plugin",
        "Manage plugins",
        "/plugin <subcommand>",
        Tools,
        crate::commands::plugin_cmd::handle
    );
    register_cmd!(
        "resume",
        "Resume a session",
        "/resume [session_id]",
        Session,
        crate::commands::resume::handle
    );
    register_cmd!(
        "review",
        "Request a code review of recent changes",
        "/review [focus]",
        General,
        crate::commands::review::handle
    );
    register_cmd!(
        "skills",
        "Manage skills",
        "/skills <subcommand>",
        Skills,
        crate::commands::skills_cmd::handle
    );
    register_cmd!(
        "stats",
        "Show session statistics (tokens, cost, tool calls)",
        "/stats",
        Session,
        crate::commands::stats::handle
    );
    register_cmd!(
        "status",
        "Show agent status",
        "/status",
        General,
        crate::commands::status::handle
    );
    register_cmd!(
        "tasks",
        "Manage background tasks",
        "/tasks <subcommand>",
        Tasks,
        crate::commands::tasks_cmd::handle
    );
    register_cmd!(
        "theme",
        "Change output theme",
        "/theme [name]",
        Configuration,
        crate::commands::theme::handle
    );
    register_cmd!(
        "vim",
        "Toggle vim keybinding mode",
        "/vim",
        Configuration,
        crate::commands::vim::handle
    );
    register_cmd!(
        "voice",
        "Toggle voice mode",
        "/voice",
        Session,
        crate::commands::voice_cmd::handle,
        interactive
    );

    registry
}
