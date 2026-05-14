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
use super::tool_activation_store::ToolActivationStore;
use super::tool_use_summary::ToolUseSummaryService;

use crate::agent::coding_mode::{CodingMode, CodingModeHandle};
use crate::commands::registry::CommandRegistry;
use crate::tasks::runner::TaskRunner;
use crate::tools::exit_plan_mode::PendingPlan;
use crate::tools::todo_write::TodoStore;

pub struct RuntimeFlags {
    pub effort: parking_lot::RwLock<String>,
    pub vim_mode: parking_lot::RwLock<String>,
    pub color_mode: parking_lot::RwLock<String>,
}

impl Default for RuntimeFlags {
    fn default() -> Self {
        Self {
            effort: parking_lot::RwLock::new(
                std::env::var("SEN_EFFORT").unwrap_or_else(|_| "medium".into()),
            ),
            vim_mode: parking_lot::RwLock::new(
                std::env::var("SEN_VIM_MODE").unwrap_or_else(|_| "off".into()),
            ),
            color_mode: parking_lot::RwLock::new(
                std::env::var("SEN_COLOR").unwrap_or_else(|_| "auto".into()),
            ),
        }
    }
}

impl RuntimeFlags {
    pub fn get_effort(&self) -> String {
        self.effort.read().clone()
    }
    pub fn set_effort(&self, val: &str) {
        *self.effort.write() = val.to_string();
    }
    pub fn get_vim_mode(&self) -> String {
        self.vim_mode.read().clone()
    }
    pub fn set_vim_mode(&self, val: &str) {
        *self.vim_mode.write() = val.to_string();
    }
    pub fn get_color_mode(&self) -> String {
        self.color_mode.read().clone()
    }
    pub fn set_color_mode(&self, val: &str) {
        *self.color_mode.write() = val.to_string();
    }
}

pub struct ServiceContainer {

    pub analytics: AnalyticsService,
    pub compact: CompactService,
    pub lsp: LspService,
    pub mcp: McpManager,

    pub rate_limiter: RateLimiter,

    pub session_memory: SessionMemoryService,

    pub token_estimator: TokenEstimator,

    pub notifier: Notifier,

    pub oauth: OAuthService,

    pub agent_summary: AgentSummaryService,

    pub auto_dream: AutoDreamService,

    pub extraction_config: ExtractionConfig,

    pub plugin_service: PluginService,
    pub policy_limits: PolicyLimitsService,

    pub prompt_suggestion: PromptSuggestionService,

    pub settings_sync: SettingsSyncService,

    pub team_memory_sync: TeamMemorySyncService,

    pub tool_use_summary: Arc<std::sync::Mutex<ToolUseSummaryService>>,

    pub command_registry: CommandRegistry,
    pub task_runner: TaskRunner,

    pub coding_mode: CodingModeHandle,

    pub session_coding_modes:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, CodingMode>>>,

    pub pending_plan: PendingPlan,

    pub todo_store: TodoStore,

    pub max_context_tokens: AtomicUsize,

    pub runtime_flags: Arc<RuntimeFlags>,

    pub shared_config: Arc<crate::config::hot_reload::SharedConfig>,

    pub agent_metrics: Arc<crate::observability::agent_metrics::AgentMetrics>,

    pub blackboard: Arc<crate::memory::blackboard::Blackboard>,

    pub health_broadcaster: crate::agent::health_signal::HealthBroadcaster,

    pub deferred_builtin_names: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,

    pub tool_activation_store: Arc<ToolActivationStore>,

    pub tool_search_invocations_total: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_activations_total: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_high_risk_blocked_total: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_total_latency_ms: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_latency_samples: Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolSearchMetricsSnapshot {
    pub invocations: u64,
    pub activations: u64,
    pub high_risk_blocked: u64,
    pub avg_latency_ms: u64,
}

pub struct ServiceContainerConfig {
    pub data_dir: PathBuf,
    pub auto_dream_enabled: bool,
    pub team_sync_enabled: bool,
    pub policy_rules: Vec<PolicyRule>,
    pub conflict_strategy: ConflictStrategy,

    pub shared_config: Option<Arc<crate::config::hot_reload::SharedConfig>>,
}

impl Default for ServiceContainerConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".senweavercoding"),
            auto_dream_enabled: false,
            team_sync_enabled: false,
            policy_rules: Vec::new(),
            conflict_strategy: ConflictStrategy::LastWriterWins,
            shared_config: None,
        }
    }
}

impl ServiceContainer {

    pub fn new(cfg: ServiceContainerConfig) -> Self {
        let sync_file = cfg.data_dir.join("settings_sync.json");

        let command_registry = register_all_commands();

        let shared_config = cfg.shared_config.unwrap_or_else(|| {
            Arc::new(crate::config::hot_reload::SharedConfig::new(
                crate::config::schema::Config::default(),
            ))
        });

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
            session_coding_modes: Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            pending_plan: crate::tools::exit_plan_mode::new_pending_plan(),
            todo_store: Arc::new(parking_lot::RwLock::new(Vec::new())),
            max_context_tokens: AtomicUsize::new(128_000),
            runtime_flags: Arc::new(RuntimeFlags::default()),
            shared_config,
            agent_metrics: Arc::new(crate::observability::agent_metrics::AgentMetrics::new()),
            blackboard: Arc::new(crate::memory::blackboard::Blackboard::new()),
            health_broadcaster: crate::agent::health_signal::HealthBroadcaster::new(),
            deferred_builtin_names: Arc::new(parking_lot::RwLock::new(
                std::collections::HashSet::new(),
            )),
            tool_activation_store: Arc::new(ToolActivationStore::new(
                cfg.data_dir.join("tool_activations"),
            )),
            tool_search_invocations_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_activations_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_high_risk_blocked_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_total_latency_ms: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_latency_samples: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn record_tool_search_invocation(&self, latency_ms: u64) {
        use std::sync::atomic::Ordering;
        self.tool_search_invocations_total
            .fetch_add(1, Ordering::Relaxed);
        self.tool_search_total_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        self.tool_search_latency_samples
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tool_search_activations(&self, count: u64) {
        use std::sync::atomic::Ordering;
        if count > 0 {
            self.tool_search_activations_total
                .fetch_add(count, Ordering::Relaxed);
        }
    }

    pub fn record_tool_search_high_risk_blocked(&self) {
        use std::sync::atomic::Ordering;
        self.tool_search_high_risk_blocked_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn tool_search_metrics_snapshot(&self) -> ToolSearchMetricsSnapshot {
        use std::sync::atomic::Ordering;
        let invocations = self.tool_search_invocations_total.load(Ordering::Relaxed);
        let activations = self.tool_search_activations_total.load(Ordering::Relaxed);
        let blocked = self
            .tool_search_high_risk_blocked_total
            .load(Ordering::Relaxed);
        let total = self.tool_search_total_latency_ms.load(Ordering::Relaxed);
        let samples = self.tool_search_latency_samples.load(Ordering::Relaxed);
        let avg = if samples > 0 { total / samples } else { 0 };
        ToolSearchMetricsSnapshot {
            invocations,
            activations,
            high_risk_blocked: blocked,
            avg_latency_ms: avg,
        }
    }

    pub fn set_max_context_tokens(&self, tokens: usize) {
        self.max_context_tokens.store(tokens, Ordering::Relaxed);
    }

    pub fn get_max_context_tokens(&self) -> usize {
        self.max_context_tokens.load(Ordering::Relaxed)
    }

    pub fn has_command(&self, name: &str) -> bool {
        self.command_registry.find(name).is_some()
    }

    pub fn check_tool_policy(&self, tool_name: &str) -> bool {
        self.policy_limits.check_tool(tool_name).allowed
    }

    pub fn check_model_policy(&self, model_id: &str) -> bool {
        self.policy_limits.check_model(model_id).allowed
    }

    pub fn check_spending_policy(&self, current_cents: u64) -> bool {
        self.policy_limits.check_spending(current_cents).allowed
    }

    pub fn config(&self) -> Arc<crate::config::schema::Config> {
        self.shared_config.load()
    }

    pub fn config_subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::config::ConfigChangedEvent> {
        self.shared_config.subscribe()
    }

    pub fn update_config(&self, new_config: crate::config::schema::Config) {
        self.shared_config
            .store(new_config, vec!["service_container.update".into()]);
    }

    pub fn config_subscribe_filtered(
        &self,
        prefixes: Vec<String>,
        callback: impl FnMut(Arc<crate::config::schema::Config>) + Send + 'static,
    ) -> crate::runtime::TaskHandle {
        self.shared_config
            .clone()
            .subscribe_filtered(prefixes, callback)
    }

    pub fn session_coding_mode(&self, session_key: &str) -> Option<CodingMode> {
        self.session_coding_modes.read().get(session_key).copied()
    }

    pub fn set_session_coding_mode(&self, session_key: &str, mode: CodingMode) {
        self.session_coding_modes
            .write()
            .insert(session_key.to_string(), mode);
    }

    pub fn clear_session_coding_mode(&self, session_key: &str) {
        self.session_coding_modes.write().remove(session_key);
    }

    pub fn resolve_coding_mode_for(&self, session_key: Option<&str>) -> CodingMode {
        if let Some(key) = session_key {
            if let Some(mode) = self.session_coding_modes.read().get(key).copied() {
                return mode;
            }
        }
        *self.coding_mode.read()
    }
}

static GLOBAL_SERVICES: OnceLock<ServiceContainer> = OnceLock::new();

pub fn init_services(cfg: ServiceContainerConfig) -> &'static ServiceContainer {
    GLOBAL_SERVICES.get_or_init(|| ServiceContainer::new(cfg))
}

pub fn get_services() -> &'static ServiceContainer {
    GLOBAL_SERVICES
        .get()
        .expect("ServiceContainer not initialized — call init_services() first")
}

pub fn try_get_services() -> Option<&'static ServiceContainer> {
    GLOBAL_SERVICES.get()
}

pub fn register_all_commands() -> CommandRegistry {
    use crate::commands::registry::{CommandCategory, SlashCommand};
    use std::sync::Arc;

    let registry = CommandRegistry::from_inventory();

    #[allow(unused_macros)]
    macro_rules! register_cmd {
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

    registry
}
