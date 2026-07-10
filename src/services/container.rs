// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use super::agent_summary::AgentSummaryService;
use super::assist::analytics::AnalyticsService;
use super::compact::CompactService;
use super::memory::extract::ExtractionConfig;
use super::lsp::LspService;
use super::mcp_manager::McpManager;
use super::assist::notifier::Notifier;
use super::oauth::OAuthService;
use super::plugin_service::PluginService;
use super::governance::policy_limits::{PolicyLimitsService, PolicyRule};
use super::prompt_suggestion::PromptSuggestionService;
use super::governance::rate_limit::RateLimiter;
use super::memory::session::SessionMemoryService;
use super::settings_sync::{ConflictStrategy, SettingsSyncService};
use super::memory::team_sync::TeamMemorySyncService;
use super::assist::tips::TipManager;
use super::token_estimation::TokenEstimator;
use super::tool_telemetry::activation_store::ToolActivationStore;
use super::tool_telemetry::use_summary::ToolUseSummaryService;

use crate::agent::coding_mode::{CodingMode, CodingModeHandle};
use crate::commands::registry::CommandRegistry;
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

    pub extraction_config: ExtractionConfig,

    pub plugin_service: PluginService,
    pub policy_limits: PolicyLimitsService,

    pub prompt_suggestion: PromptSuggestionService,

    pub settings_sync: SettingsSyncService,

    pub team_memory_sync: TeamMemorySyncService,

    pub tool_use_summary: Arc<parking_lot::Mutex<ToolUseSummaryService>>,

    pub command_registry: CommandRegistry,

    pub coding_mode: CodingModeHandle,

    pub session_coding_modes:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, CodingMode>>>,

    session_auto_coding_modes:
        Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,

    global_auto_coding_mode: Arc<std::sync::atomic::AtomicBool>,

    session_pending_plans:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, String>>>,

    session_designer:
        Arc<parking_lot::RwLock<std::collections::HashMap<String, DesignerSelection>>>,

    session_debug: Arc<parking_lot::RwLock<std::collections::HashMap<String, DebugSelection>>>,

    #[cfg(feature = "tool-curator")]
    pub curator_state: crate::tools::curator::state::CuratorState,

    #[cfg(feature = "tool-curator")]
    pub pending_curator: crate::tools::curator::state::PendingCurator,

    #[cfg(feature = "tool-curator")]
    pub curator_mode_flag: crate::tools::curator::tools::CuratorModeFlag,

    pub todo_store: TodoStore,

    pub max_context_tokens: AtomicUsize,

    pub runtime_flags: Arc<RuntimeFlags>,

    pub shared_config: Arc<crate::config::hot_reload::SharedConfig>,

    pub agent_metrics: Arc<crate::observability::agent_metrics::AgentMetrics>,

    pub blackboard: Arc<crate::memory::blackboard::Blackboard>,

    pub health_broadcaster: crate::agent::health_signal::HealthBroadcaster,

    pub deferred_builtin_names: Arc<
        parking_lot::RwLock<
            std::collections::HashMap<String, std::collections::HashSet<String>>,
        >,
    >,

    pub tool_activation_store: Arc<ToolActivationStore>,

    pub template_library: super::template_library::TemplateLibraryStore,

    pub tool_search_invocations_total: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_activations_total: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_high_risk_blocked_total: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_total_latency_ms: Arc<std::sync::atomic::AtomicU64>,
    pub tool_search_latency_samples: Arc<std::sync::atomic::AtomicU64>,

    pub proxy_runtime: Arc<crate::services::proxy::runtime::ProxyRuntime>,

    pub remote_sessions: crate::remote::manager::RemoteSessionManager,

    pub tips: parking_lot::Mutex<TipManager>,

    #[cfg(feature = "lan-comms")]
    pub lan: Option<Arc<crate::lan::LanService>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesignerSelection {
    pub submode_id: String,
    pub params: serde_json::Value,
    #[serde(default)]
    pub ref_artifact: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DebugSelection {
    pub submode_id: String,
    pub params: serde_json::Value,
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
    pub team_sync_enabled: bool,
    pub policy_rules: Vec<PolicyRule>,
    pub conflict_strategy: ConflictStrategy,

    pub shared_config: Option<Arc<crate::config::hot_reload::SharedConfig>>,
}

impl Default for ServiceContainerConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".senweavercoding"),
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

        #[cfg(feature = "lan-comms")]
        let lan = match crate::lan::LanService::new(&cfg.data_dir, &shared_config) {
            Ok(svc) => Some(svc),
            Err(err) => {
                tracing::warn!(error = %err, "failed to initialise LAN service");
                None
            }
        };

        Self {
            analytics: AnalyticsService::new_with_persistence(
                true,
                Some(cfg.data_dir.join("analytics")),
            ),
            compact: CompactService,
            lsp: LspService::new(),
            mcp: McpManager::new(),
            notifier: Notifier::new(),
            oauth: OAuthService::new(),
            rate_limiter: RateLimiter::new(),
            session_memory: SessionMemoryService::new(),
            token_estimator: TokenEstimator::new(4.0),

            agent_summary: AgentSummaryService,
            extraction_config: ExtractionConfig::default(),
            plugin_service: PluginService::new(),
            policy_limits: PolicyLimitsService::new(cfg.policy_rules),
            prompt_suggestion: PromptSuggestionService,
            settings_sync: SettingsSyncService::new(sync_file, cfg.conflict_strategy),
            team_memory_sync: TeamMemorySyncService::new(cfg.team_sync_enabled),
            tool_use_summary: Arc::new(parking_lot::Mutex::new(ToolUseSummaryService::new())),

            command_registry,
            coding_mode: crate::agent::coding_mode::coding_mode_handle_with(
                crate::util::get_runtime_var("SEN_CODING_MODE")
                    .as_deref()
                    .and_then(CodingMode::from_str_loose)
                    .unwrap_or_default(),
            ),
            session_coding_modes: Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            session_auto_coding_modes: Arc::new(parking_lot::RwLock::new(
                std::collections::HashSet::new(),
            )),
            global_auto_coding_mode: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            session_pending_plans: Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            session_designer: Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            session_debug: Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            #[cfg(feature = "tool-curator")]
            curator_state: crate::tools::curator::state::new_curator_state(),
            #[cfg(feature = "tool-curator")]
            pending_curator: crate::tools::curator::state::new_pending_curator(),
            #[cfg(feature = "tool-curator")]
            curator_mode_flag: std::sync::Arc::new(
                crate::tools::curator::tools::CuratorModeRegistry::new(),
            ),
            todo_store: crate::tools::todo_write::new_todo_store(),
            max_context_tokens: AtomicUsize::new(128_000),
            runtime_flags: Arc::new(RuntimeFlags::default()),
            shared_config,
            agent_metrics: Arc::new(crate::observability::agent_metrics::AgentMetrics::new()),
            blackboard: Arc::new(crate::memory::blackboard::Blackboard::new()),
            health_broadcaster: crate::agent::health_signal::HealthBroadcaster::new(),
            deferred_builtin_names: Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
            tool_activation_store: Arc::new(ToolActivationStore::new(
                cfg.data_dir.join("tool_activations"),
            )),
            template_library: super::template_library::TemplateLibraryStore::new(&cfg.data_dir),
            tool_search_invocations_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_activations_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_high_risk_blocked_total: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_total_latency_ms: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tool_search_latency_samples: Arc::new(std::sync::atomic::AtomicU64::new(0)),

            proxy_runtime: crate::services::proxy::runtime::ProxyRuntime::global(),
            remote_sessions: crate::remote::manager::RemoteSessionManager::new(),
            tips: parking_lot::Mutex::new(TipManager::new(2)),
            #[cfg(feature = "lan-comms")]
            lan,
        }
    }

    pub fn proxy_runtime(&self) -> &crate::services::proxy::runtime::ProxyRuntime {
        &self.proxy_runtime
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
        self.session_auto_coding_modes.write().remove(session_key);
    }

    pub fn is_session_auto_coding_mode(&self, session_key: &str) -> bool {
        self.session_auto_coding_modes.read().contains(session_key)
    }

    pub fn set_session_auto_coding_mode(&self, session_key: &str, enabled: bool) {
        let mut guard = self.session_auto_coding_modes.write();
        if enabled {
            guard.insert(session_key.to_string());
        } else {
            guard.remove(session_key);
        }
    }

    pub fn is_global_auto_coding_mode(&self) -> bool {
        self.global_auto_coding_mode.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_global_auto_coding_mode(&self, enabled: bool) {
        self.global_auto_coding_mode
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn session_designer(&self, session_key: &str) -> Option<DesignerSelection> {
        self.session_designer.read().get(session_key).cloned()
    }

    pub fn set_session_designer(
        &self,
        session_key: &str,
        submode_id: String,
        params: serde_json::Value,
        ref_artifact: Option<String>,
    ) {
        self.session_designer.write().insert(
            session_key.to_string(),
            DesignerSelection {
                submode_id,
                params,
                ref_artifact,
            },
        );
    }

    pub fn clear_session_designer(&self, session_key: &str) {
        self.session_designer.write().remove(session_key);
    }

    pub fn session_debug(&self, session_key: &str) -> Option<DebugSelection> {
        self.session_debug.read().get(session_key).cloned()
    }

    pub fn set_session_debug(
        &self,
        session_key: &str,
        submode_id: String,
        params: serde_json::Value,
    ) {
        self.session_debug.write().insert(
            session_key.to_string(),
            DebugSelection {
                submode_id,
                params,
            },
        );
    }

    pub fn clear_session_debug(&self, session_key: &str) {
        self.session_debug.write().remove(session_key);
    }

    pub fn resolve_coding_mode_for(&self, session_key: Option<&str>) -> CodingMode {
        if let Some(key) = session_key {
            if let Some(mode) = self.session_coding_modes.read().get(key).copied() {
                return mode;
            }
        }
        *self.coding_mode.read()
    }

    fn pending_plan_key() -> String {
        crate::session::current_session_context()
            .map(|c| c.session_id)
            .unwrap_or_else(|| "default".to_string())
    }

    pub fn set_pending_plan(&self, plan: String) {
        self.session_pending_plans
            .write()
            .insert(Self::pending_plan_key(), plan);
    }

    pub fn take_pending_plan(&self) -> Option<String> {
        self.session_pending_plans
            .write()
            .remove(&Self::pending_plan_key())
    }

    fn deferred_builtin_key() -> String {
        crate::session::current_session_context()
            .map(|c| c.session_id)
            .unwrap_or_else(|| "default".to_string())
    }

    pub fn set_deferred_builtin_names(&self, names: std::collections::HashSet<String>) {
        self.deferred_builtin_names
            .write()
            .insert(Self::deferred_builtin_key(), names);
    }

    pub fn deferred_builtin_names_snapshot(&self) -> std::collections::HashSet<String> {
        self.deferred_builtin_names
            .read()
            .get(&Self::deferred_builtin_key())
            .cloned()
            .unwrap_or_default()
    }

    pub fn deferred_builtin_total(&self) -> usize {
        self.deferred_builtin_names
            .read()
            .values()
            .map(|s| s.len())
            .sum()
    }
}

static GLOBAL_SERVICES: OnceLock<ServiceContainer> = OnceLock::new();

pub fn init_services(cfg: ServiceContainerConfig) -> &'static ServiceContainer {
    if GLOBAL_SERVICES.get().is_some() {
        tracing::warn!(
            "init_services called after the global ServiceContainer was already initialized; \
             the new configuration is ignored. Initialize services once at startup before any \
             component reads them."
        );
        return GLOBAL_SERVICES.get().expect("just checked is_some");
    }
    let services = GLOBAL_SERVICES.get_or_init(|| ServiceContainer::new(cfg));
    services.analytics.start_persistence_loop();
    services
}

pub fn get_services() -> Option<&'static ServiceContainer> {
    GLOBAL_SERVICES.get()
}

pub fn require_services() -> &'static ServiceContainer {
    if let Some(services) = GLOBAL_SERVICES.get() {
        return services;
    }
    tracing::warn!(
        "require_services called before init_services; initializing a default ServiceContainer \
         (call init_services() during startup to control its configuration)"
    );
    let services = GLOBAL_SERVICES.get_or_init(|| {
        ServiceContainer::new(ServiceContainerConfig::default())
    });
    services.analytics.start_persistence_loop();
    services
}

pub fn try_get_services() -> Option<&'static ServiceContainer> {
    get_services()
}

pub fn register_all_commands() -> CommandRegistry {
    CommandRegistry::from_inventory()
}
