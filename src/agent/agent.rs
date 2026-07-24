// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::agent::dispatcher::{
    NativeToolDispatcher, ParsedToolCall, ToolDispatcher, ToolExecutionResult, XmlToolDispatcher,
};
use crate::agent::memory_loader::{DefaultMemoryLoader, MemoryLoader};
use crate::agent::prompt::{PromptContext, SystemPromptBuilder};
use crate::config::Config;
use crate::error::AgentError;
use crate::i18n::ToolDescriptions;
use crate::memory::{self, Memory, MemoryCategory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::{self, ChatMessage, ConversationMessage, Provider, ToolCall};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools::{self, Tool, ToolSpec};
use anyhow::Result;
use chrono::{Datelike, Timelike};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub(crate) const TURN_EVENT_DRAIN_BUFFER: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardrailApprovalOutcome {
    Approved,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone)]
pub enum TurnEvent {

    Chunk { delta: String },

    StreamReset,

    Thinking { delta: String },

    ToolCall {
        name: String,
        args: serde_json::Value,
        tool_call_id: Option<String>,
    },

    ToolResult {
        name: String,
        output: String,
        success: bool,
        tool_call_id: Option<String>,
    },

    PlanProgressCommitted {
        plan_path: String,
        title: String,
        todos_json: String,
    },

    Error { message: String },

    FileEdit {
        path: String,
        additions: i32,
        deletions: i32,
        diff: Option<String>,
        edit_batch_id: Option<String>,
    },

    StatusUpdate { action: String, detail: String },

    ProgressTick {
        iteration: usize,
        max_iterations: usize,
        tokens_used: u64,
    },

    CommandPreview {
        tool_name: String,
        args: serde_json::Value,
        estimated_duration_ms: Option<u64>,
    },

    Cancelling { reason: String },

    ContextCompressed {
        tokens_before: usize,
        tokens_after: usize,
    },

    PermissionRequest {
        request_id: String,
        tool_name: String,
        input: serde_json::Value,
        description: Option<String>,
    },

    SubagentChunk {
        task_id: String,
        agent_id: String,
        kind: SubagentChunkKind,
        delta: String,
    },

    PiiSanitized {
        report: crate::services::governance::pii_sanitizer::SanitizationReport,
    },

    ProviderRetry {
        attempt: u32,
        max_attempts: u32,
        wait_ms: u64,
        class: String,
        provider: String,
        model: String,
        message: String,
    },

    WorkerSpawned {
        parent_tool_use_id: String,
        worker_id: String,
        title: String,
        model: String,
    },

    WorkerStatus {
        worker_id: String,
        status: String,
        detail: Option<String>,
    },

    WorkerProgress {
        worker_id: String,
        action: String,
        detail: String,
    },

    WorkerCompleted {
        worker_id: String,
        success: bool,
        summary: String,
    },

    WorkerStopped {
        worker_id: String,
        reason: String,
    },

    ParentResumed {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentChunkKind {

    Chunk,

    Thinking,

    ToolCall,

    ToolResult,

    Status,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigChange {

    None,

    Soft,

    Hard,
}

struct RecentFocusView {
    messages: Vec<crate::providers::traits::ChatMessage>,
    leading_system_end: usize,
    retained_summary: Option<String>,
    dropped: Vec<crate::providers::traits::ChatMessage>,
    dropped_turns: usize,
}

#[derive(Clone)]
struct UnfinishedTask {
    seq: u64,
    request: String,
    progress: String,
}

struct RollingSummaryRefreshJob {
    recent_dropped: Vec<crate::providers::traits::ChatMessage>,
    fingerprint: u64,
    dropped_turns: usize,
    model: String,
}

pub struct Agent {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    tool_index: std::collections::HashMap<String, usize>,

    tool_specs: std::sync::Arc<Vec<ToolSpec>>,
    memory: Arc<dyn Memory>,
    observer: Arc<dyn Observer>,
    prompt_builder: SystemPromptBuilder,
    tool_dispatcher: Box<dyn ToolDispatcher>,
    memory_loader: Box<dyn MemoryLoader>,
    config: crate::config::AgentConfig,
    model_name: String,
    temperature: f64,
    workspace_dir: std::path::PathBuf,
    identity_config: crate::config::IdentityConfig,
    skills: Vec<crate::skills::Skill>,
    skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
    auto_save: bool,
    memory_session_id: Option<String>,
    history: Vec<ConversationMessage>,
    classification_config: crate::config::QueryClassificationConfig,
    available_hints: Vec<String>,
    route_model_by_hint: HashMap<String, String>,
    allowed_tools: Option<Vec<String>>,
    response_cache: Option<Arc<crate::memory::response_cache::ResponseCache>>,
    tool_descriptions: Option<ToolDescriptions>,

    security_summary: Option<String>,

    autonomy_level: crate::security::AutonomyLevel,

    activated_tools: Option<Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,

    surface: crate::tools::ToolSurfaceBaseline,

    user_profile_config: crate::agent::user::profile::UserProfileConfig,
    skill_evolution_config: crate::agent::skill_evolution::SkillEvolutionConfig,
    prompt_optimizer_config: crate::agent::prompt::optimizer::PromptOptimizerConfig,

    rbac_engine: Option<std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<crate::security::rbac::CallerIdentity>,
    experience_replay: Option<crate::agent::reward::experience::ExperienceReplay>,
    plan_mode_config: crate::agent::plan_mode::PlanModeConfig,
    intent_analysis_config: crate::agent::intent::IntentAnalysisConfig,

    mode_tool_filter: Option<std::collections::HashSet<String>>,

    mode_filter_dirty: bool,

    current_coding_mode: Option<crate::agent::coding_mode::CodingMode>,

    baseline_max_tool_iterations: usize,

    cancelled: Arc<std::sync::atomic::AtomicBool>,

    cancel_signal: Arc<arc_swap::ArcSwap<tokio_util::sync::CancellationToken>>,

    shared_config: crate::config::live::LiveConfig,

    runtime_selection_override: Option<(String, String)>,

    cached_provider: String,

    cached_api_key: crate::security::secret_string::SecretString,
    cached_api_url: String,

    last_usage: Option<crate::providers::TokenUsage>,

    desktop_security_policy: Option<Arc<SecurityPolicy>>,

    plan_execution_armed: parking_lot::Mutex<Option<String>>,

    resuming_from_ask: std::sync::atomic::AtomicBool,

    rolling_summary: Arc<std::sync::Mutex<Option<(u64, usize, String)>>>,

    rolling_summary_refresh: std::sync::Mutex<Option<RollingSummaryRefreshJob>>,

    rolling_summary_refresh_inflight: Arc<std::sync::atomic::AtomicBool>,

    pending_mcp_registry: std::sync::Arc<
        parking_lot::Mutex<Option<(u64, std::sync::Arc<crate::tools::McpRegistry>)>>,
    >,

    hook_runner: Option<std::sync::Arc<crate::hooks::HotHookRunner>>,

    cached_tools_signature: u64,

    plan_mode_flag: crate::tools::PlanModeFlag,

    last_turn_interrupted: bool,

    unfinished_task: std::sync::Mutex<Option<UnfinishedTask>>,

    pending_intent_decision: std::sync::Mutex<
        Option<(
            String,
            std::time::Instant,
            crate::agent::intent::LlmIntentDecision,
        )>,
    >,

    last_turn_resumed: bool,
}

pub struct AgentBuilder {
    provider: Option<Box<dyn Provider>>,
    tools: Option<Vec<Box<dyn Tool>>>,
    memory: Option<Arc<dyn Memory>>,
    observer: Option<Arc<dyn Observer>>,
    prompt_builder: Option<SystemPromptBuilder>,
    tool_dispatcher: Option<Box<dyn ToolDispatcher>>,
    memory_loader: Option<Box<dyn MemoryLoader>>,
    config: Option<crate::config::AgentConfig>,
    model_name: Option<String>,
    temperature: Option<f64>,
    workspace_dir: Option<std::path::PathBuf>,
    identity_config: Option<crate::config::IdentityConfig>,
    skills: Option<Vec<crate::skills::Skill>>,
    skills_prompt_mode: Option<crate::config::SkillsPromptInjectionMode>,
    auto_save: Option<bool>,
    memory_session_id: Option<String>,
    classification_config: Option<crate::config::QueryClassificationConfig>,
    available_hints: Option<Vec<String>>,
    route_model_by_hint: Option<HashMap<String, String>>,
    allowed_tools: Option<Vec<String>>,
    denied_tools: Option<Vec<String>>,
    response_cache: Option<Arc<crate::memory::response_cache::ResponseCache>>,
    tool_descriptions: Option<ToolDescriptions>,
    security_summary: Option<String>,
    autonomy_level: Option<crate::security::AutonomyLevel>,
    activated_tools: Option<Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    surface: Option<crate::tools::ToolSurfaceBaseline>,
    user_profile_config: Option<crate::agent::user::profile::UserProfileConfig>,
    skill_evolution_config: Option<crate::agent::skill_evolution::SkillEvolutionConfig>,
    prompt_optimizer_config: Option<crate::agent::prompt::optimizer::PromptOptimizerConfig>,
    rbac_engine: Option<std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<crate::security::rbac::CallerIdentity>,
    experience_replay: Option<crate::agent::reward::experience::ExperienceReplay>,
    plan_mode_config: Option<crate::agent::plan_mode::PlanModeConfig>,
    intent_analysis_config: Option<crate::agent::intent::IntentAnalysisConfig>,
    shared_config: Option<crate::config::live::LiveConfig>,

    cached_provider: Option<String>,
    cached_api_key: Option<String>,
    cached_api_url: Option<String>,
    desktop_security_policy: Option<Arc<SecurityPolicy>>,
    hook_runner: Option<std::sync::Arc<crate::hooks::HotHookRunner>>,
    plan_mode_flag: Option<crate::tools::PlanModeFlag>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            provider: None,
            tools: None,
            memory: None,
            observer: None,
            prompt_builder: None,
            tool_dispatcher: None,
            memory_loader: None,
            config: None,
            model_name: None,
            temperature: None,
            workspace_dir: None,
            identity_config: None,
            skills: None,
            skills_prompt_mode: None,
            auto_save: None,
            memory_session_id: None,
            classification_config: None,
            available_hints: None,
            route_model_by_hint: None,
            allowed_tools: None,
            denied_tools: None,
            response_cache: None,
            tool_descriptions: None,
            security_summary: None,
            autonomy_level: None,
            activated_tools: None,
            surface: None,
            user_profile_config: None,
            skill_evolution_config: None,
            prompt_optimizer_config: None,
            rbac_engine: None,
            rbac_identity: None,
            experience_replay: None,
            plan_mode_config: None,
            intent_analysis_config: None,
            shared_config: None,
            cached_provider: None,
            cached_api_key: None,
            cached_api_url: None,
            desktop_security_policy: None,
            hook_runner: None,
            plan_mode_flag: None,
        }
    }

    pub fn plan_mode_flag(mut self, flag: crate::tools::PlanModeFlag) -> Self {
        self.plan_mode_flag = Some(flag);
        self
    }

    pub fn provider(mut self, provider: Box<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn tools(mut self, tools: Vec<Box<dyn Tool>>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn prompt_builder(mut self, prompt_builder: SystemPromptBuilder) -> Self {
        self.prompt_builder = Some(prompt_builder);
        self
    }

    pub fn tool_dispatcher(mut self, tool_dispatcher: Box<dyn ToolDispatcher>) -> Self {
        self.tool_dispatcher = Some(tool_dispatcher);
        self
    }

    pub fn memory_loader(mut self, memory_loader: Box<dyn MemoryLoader>) -> Self {
        self.memory_loader = Some(memory_loader);
        self
    }

    pub fn config(mut self, config: crate::config::AgentConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn model_name(mut self, model_name: String) -> Self {
        self.model_name = Some(model_name);
        self
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn workspace_dir(mut self, workspace_dir: std::path::PathBuf) -> Self {
        self.workspace_dir = Some(workspace_dir);
        self
    }

    pub fn identity_config(mut self, identity_config: crate::config::IdentityConfig) -> Self {
        self.identity_config = Some(identity_config);
        self
    }

    pub fn skills(mut self, skills: Vec<crate::skills::Skill>) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn skills_prompt_mode(
        mut self,
        skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
    ) -> Self {
        self.skills_prompt_mode = Some(skills_prompt_mode);
        self
    }

    pub fn auto_save(mut self, auto_save: bool) -> Self {
        self.auto_save = Some(auto_save);
        self
    }

    pub fn memory_session_id(mut self, memory_session_id: Option<String>) -> Self {
        self.memory_session_id = memory_session_id;
        self
    }

    pub fn classification_config(
        mut self,
        classification_config: crate::config::QueryClassificationConfig,
    ) -> Self {
        self.classification_config = Some(classification_config);
        self
    }

    pub fn available_hints(mut self, available_hints: Vec<String>) -> Self {
        self.available_hints = Some(available_hints);
        self
    }

    pub fn route_model_by_hint(mut self, route_model_by_hint: HashMap<String, String>) -> Self {
        self.route_model_by_hint = Some(route_model_by_hint);
        self
    }

    pub fn allowed_tools(mut self, allowed_tools: Option<Vec<String>>) -> Self {
        self.allowed_tools = allowed_tools;
        self
    }

    pub fn denied_tools(mut self, denied_tools: Option<Vec<String>>) -> Self {
        self.denied_tools = denied_tools;
        self
    }

    pub fn response_cache(
        mut self,
        cache: Option<Arc<crate::memory::response_cache::ResponseCache>>,
    ) -> Self {
        self.response_cache = cache;
        self
    }

    pub fn tool_descriptions(mut self, tool_descriptions: Option<ToolDescriptions>) -> Self {
        self.tool_descriptions = tool_descriptions;
        self
    }

    pub fn security_summary(mut self, summary: Option<String>) -> Self {
        self.security_summary = summary;
        self
    }

    pub fn autonomy_level(mut self, level: crate::security::AutonomyLevel) -> Self {
        self.autonomy_level = Some(level);
        self
    }

    pub fn surface(mut self, surface: crate::tools::ToolSurfaceBaseline) -> Self {
        self.surface = Some(surface);
        self
    }

    pub fn activated_tools(
        mut self,
        activated: Option<Arc<parking_lot::Mutex<tools::ActivatedToolSet>>>,
    ) -> Self {
        self.activated_tools = activated;
        self
    }

    pub fn user_profile_config(
        mut self,
        cfg: crate::agent::user::profile::UserProfileConfig,
    ) -> Self {
        self.user_profile_config = Some(cfg);
        self
    }

    pub fn skill_evolution_config(
        mut self,
        cfg: crate::agent::skill_evolution::SkillEvolutionConfig,
    ) -> Self {
        self.skill_evolution_config = Some(cfg);
        self
    }

    pub fn prompt_optimizer_config(
        mut self,
        cfg: crate::agent::prompt::optimizer::PromptOptimizerConfig,
    ) -> Self {
        self.prompt_optimizer_config = Some(cfg);
        self
    }

    pub fn build(self) -> Result<Agent> {
        let mut tools = self
            .tools
            .ok_or_else(|| anyhow::anyhow!("tools are required"))?;
        let allowed = self.allowed_tools.clone();
        if let Some(ref allow_list) = allowed {
            tools.retain(|t| allow_list.iter().any(|name| name == t.name()));
        }
        let denied = self.denied_tools.clone();
        if let Some(ref deny_list) = denied {
            tools.retain(|t| !deny_list.iter().any(|name| name == t.name()));
        }
        let tool_specs_vec: Vec<ToolSpec> = tools.iter().map(|tool| tool.spec()).collect();
        let tool_specs =
            std::sync::Arc::new(crate::tools::dedupe_tool_specs(&tool_specs_vec));
        let tool_index: std::collections::HashMap<String, usize> = tools
            .iter()
            .enumerate()
            .map(|(i, tool)| (tool.name().to_string(), i))
            .collect();

        let baseline_max_iter = self
            .config
            .as_ref()
            .map(|c| c.max_tool_iterations)
            .unwrap_or_default();

        Ok(Agent {
            provider: self
                .provider
                .ok_or_else(|| anyhow::anyhow!("provider is required"))?,
            tools,
            tool_index,
            tool_specs,
            memory: self
                .memory
                .ok_or_else(|| anyhow::anyhow!("memory is required"))?,
            observer: self
                .observer
                .ok_or_else(|| anyhow::anyhow!("observer is required"))?,
            prompt_builder: self
                .prompt_builder
                .unwrap_or_else(SystemPromptBuilder::with_defaults),
            tool_dispatcher: self
                .tool_dispatcher
                .ok_or_else(|| anyhow::anyhow!("tool_dispatcher is required"))?,
            memory_loader: self
                .memory_loader
                .unwrap_or_else(|| Box::new(DefaultMemoryLoader::default())),
            config: self.config.unwrap_or_default(),
            model_name: self
                .model_name
                .ok_or_else(|| anyhow::anyhow!(
                    "no_model_configured: AgentBuilder.model_name is required; please add at least one model in Provider settings"
                ))?,
            temperature: self.temperature.unwrap_or(0.7),
            workspace_dir: self
                .workspace_dir
                .unwrap_or_else(|| std::path::PathBuf::from(".")),
            identity_config: self.identity_config.unwrap_or_default(),
            skills: self.skills.unwrap_or_default(),
            skills_prompt_mode: self.skills_prompt_mode.unwrap_or_default(),
            auto_save: self.auto_save.unwrap_or(false),
            memory_session_id: self.memory_session_id,
            history: Vec::new(),
            classification_config: self.classification_config.unwrap_or_default(),
            available_hints: self.available_hints.unwrap_or_default(),
            route_model_by_hint: self.route_model_by_hint.unwrap_or_default(),
            allowed_tools: allowed,
            response_cache: self.response_cache,
            tool_descriptions: self.tool_descriptions,
            security_summary: self.security_summary,
            autonomy_level: self
                .autonomy_level
                .unwrap_or(crate::security::AutonomyLevel::Supervised),
            activated_tools: self.activated_tools,
            surface: self
                .surface
                .unwrap_or(crate::tools::ToolSurfaceBaseline::Both),
            user_profile_config: self.user_profile_config.unwrap_or_default(),
            skill_evolution_config: self.skill_evolution_config.unwrap_or_default(),
            prompt_optimizer_config: self.prompt_optimizer_config.unwrap_or_default(),
            rbac_engine: self.rbac_engine,
            rbac_identity: self.rbac_identity,
            experience_replay: self.experience_replay,
            plan_mode_config: self.plan_mode_config.unwrap_or_default(),
            intent_analysis_config: self.intent_analysis_config.unwrap_or_default(),
            mode_tool_filter: None,
            mode_filter_dirty: false,
            current_coding_mode: None,
            baseline_max_tool_iterations: baseline_max_iter,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancel_signal: Arc::new(arc_swap::ArcSwap::from_pointee(
                tokio_util::sync::CancellationToken::new(),
            )),
            shared_config: self
                .shared_config
                .unwrap_or_else(crate::config::live::LiveConfig::default),
            runtime_selection_override: None,
            cached_provider: self.cached_provider.unwrap_or_default(),
            cached_api_key: crate::security::secret_string::SecretString::new(
                self.cached_api_key.unwrap_or_default(),
            ),
            cached_api_url: self.cached_api_url.unwrap_or_default(),
            last_usage: None,
            desktop_security_policy: self.desktop_security_policy,
            plan_execution_armed: parking_lot::Mutex::new(None),
            resuming_from_ask: std::sync::atomic::AtomicBool::new(false),
            rolling_summary: Arc::new(std::sync::Mutex::new(None)),
            rolling_summary_refresh: std::sync::Mutex::new(None),
            rolling_summary_refresh_inflight: Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            pending_mcp_registry: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            hook_runner: self.hook_runner,
            cached_tools_signature: 0,
            plan_mode_flag: self.plan_mode_flag.unwrap_or_default(),
            last_turn_interrupted: false,
            unfinished_task: std::sync::Mutex::new(None),
            pending_intent_decision: std::sync::Mutex::new(None),
            last_turn_resumed: false,
        })
    }

    pub fn rbac_session(
        mut self,
        engine: Option<Arc<crate::security::rbac::RbacEngine>>,
        identity: Option<crate::security::rbac::CallerIdentity>,
    ) -> Self {
        self.rbac_engine = engine;
        self.rbac_identity = identity;
        self
    }

    pub fn experience_replay(
        mut self,
        replay: Option<crate::agent::reward::experience::ExperienceReplay>,
    ) -> Self {
        self.experience_replay = replay;
        self
    }

    pub fn plan_mode_config(mut self, cfg: crate::agent::plan_mode::PlanModeConfig) -> Self {
        self.plan_mode_config = Some(cfg);
        self
    }

    pub fn intent_analysis_config(
        mut self,
        cfg: crate::agent::intent::IntentAnalysisConfig,
    ) -> Self {
        self.intent_analysis_config = Some(cfg);
        self
    }

    pub fn shared_config(mut self, config: crate::config::live::LiveConfig) -> Self {
        self.shared_config = Some(config);
        self
    }

    pub fn cached_provider_config(
        mut self,
        provider: String,
        api_key: String,
        api_url: String,
    ) -> Self {
        self.cached_provider = Some(provider);
        self.cached_api_key = Some(api_key);
        self.cached_api_url = Some(api_url);
        self
    }

    pub fn desktop_security_policy(mut self, policy: Option<Arc<SecurityPolicy>>) -> Self {
        self.desktop_security_policy = policy;
        self
    }

    pub fn hook_runner(
        mut self,
        runner: Option<std::sync::Arc<crate::hooks::HotHookRunner>>,
    ) -> Self {
        self.hook_runner = runner;
        self
    }
}

impl Agent {
    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }

    #[must_use]
    pub fn inline_edit_runner(&self) -> Option<std::sync::Arc<crate::inline_edit::InlineEditRunner>> {
        let config = self.shared_config.load();
        crate::inline_edit::service::default_runner(&config)
    }

    #[must_use]
    pub fn last_usage(&self) -> Option<&crate::providers::TokenUsage> {
        self.last_usage.as_ref()
    }

    pub async fn turn_streamed(
        &mut self,
        user_message: &str,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> Result<String, AgentError> {
        self.sync_tools_from_config_if_changed().await;
        self.try_attach_pending_mcp().await;

        let user_message_owned = user_message.to_string();
        let user_message_for_turn = if let Some(ref runner) = self.hook_runner {
            match runner.run_before_prompt_build(user_message_owned.clone()).await {
                crate::hooks::HookResult::Continue(rewritten) => rewritten,
                crate::hooks::HookResult::RequireApproval(_, message) => {
                    let reason = message
                        .unwrap_or_else(|| "manual approval required".to_string());
                    let banner = format!("[Cancelled by hook: {reason}]");
                    let _ = event_tx
                        .send(TurnEvent::Cancelling {
                            reason: format!("hook:beforeSubmitPrompt:{reason}"),
                        })
                        .await;
                    return Ok(banner);
                }
                crate::hooks::HookResult::Cancel(reason) => {
                    let banner = format!("[Cancelled by hook: {reason}]");
                    let _ = event_tx
                        .send(TurnEvent::Cancelling {
                            reason: format!("hook:beforeSubmitPrompt:{reason}"),
                        })
                        .await;
                    return Ok(banner);
                }
            }
        } else {
            user_message_owned
        };
        let user_message: &str = user_message_for_turn.as_str();

        let result = crate::agent::model_switch::scope_model_switch(
            self.turn_streamed_inner(user_message, event_tx),
        )
        .await;
        self.spawn_pending_rolling_summary_refresh();
        result
    }

    fn spawn_pending_rolling_summary_refresh(&self) {
        if self
            .rolling_summary_refresh_inflight
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        let job = {
            let mut guard = self
                .rolling_summary_refresh
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.take()
        };
        let Some(job) = job else {
            return;
        };
        let provider_name = self.cached_provider.clone();
        if provider_name.trim().is_empty() {
            return;
        }
        let api_key = {
            let key = self.cached_api_key.expose();
            (!key.is_empty()).then(|| key.to_string())
        };
        let api_url = {
            let url = self.cached_api_url.clone();
            (!url.is_empty()).then_some(url)
        };
        let compression_cfg = self.config.context_compression.clone();
        let summary_slot = Arc::clone(&self.rolling_summary);
        let inflight = Arc::clone(&self.rolling_summary_refresh_inflight);
        inflight.store(true, std::sync::atomic::Ordering::Release);

        struct InflightReset(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for InflightReset {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Release);
            }
        }

        let _ = crate::runtime::spawn_supervised("agent.summary.rolling_refresh", async move {
            let _done_guard = InflightReset(inflight);
            let provider = match crate::providers::create_provider_with_url_async(
                provider_name,
                api_key,
                api_url,
            )
            .await
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!(
                        target: "agent.summary",
                        error = %e,
                        "rolling summary refresh skipped: provider rebuild failed"
                    );
                    return;
                }
            };
            let window =
                crate::constants::api_limits::context_window_for_model(&job.model) as usize;
            let compressor = crate::agent::context::compressor::ContextCompressor::new(
                compression_cfg,
                window,
            );
            match tokio::time::timeout(
                std::time::Duration::from_secs(60),
                compressor.summarize_messages(&job.recent_dropped, provider.as_ref(), &job.model),
            )
            .await
            {
                Ok(Ok(text)) if !text.trim().is_empty() => {
                    let mut guard = summary_slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *guard = Some((job.fingerprint, job.dropped_turns, text));
                }
                Ok(_) => {}
                Err(_) => {
                    tracing::debug!(
                        target: "agent.summary",
                        "background rolling summary refresh timed out; keeping stale summary"
                    );
                }
            }
        });
    }

    async fn turn_streamed_inner(
        &mut self,
        user_message: &str,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> Result<String, AgentError> {
        let mut _turn_metrics = crate::agent::executor_core::TurnMetricsGuard::start();

        let _ = event_tx.try_send(TurnEvent::ProgressTick {
            iteration: 0,
            max_iterations: self.config.max_tool_iterations,
            tokens_used: 0,
        });

        if let Err(e) = self.apply_turn_preamble(user_message, &event_tx).await {
            self.plan_execution_armed.lock().take();
            return Err(e.into());
        }
        self.apply_gui_model_switch(&event_tx).await;
        self.apply_bootstrap_model_override(&event_tx).await;
        let mut effective_model = self.classify_model(user_message);

        let armed_plan_path: Option<String> = self.plan_execution_armed.lock().take();

        let mut history_chat = self.tool_dispatcher.to_provider_messages(&self.history);

        let is_design_trigger = user_message
            .trim_start()
            .starts_with(crate::agent::designer::pipeline::DESIGN_TASK_PREFIX);

        let plan_exec_mode = armed_plan_path.is_some();
        let mut focus_base_len: usize = 0;
        let full_history_for_merge: Option<Vec<crate::providers::traits::ChatMessage>> =
            if plan_exec_mode {
                let full = history_chat.clone();
                history_chat = Self::focus_history_for_plan_execution(history_chat);
                focus_base_len = history_chat.len();
                Some(full)
            } else if is_design_trigger {
                None
            } else {
                let min_turns = self.config.recent_turn_window;
                let max_turns = self.config.recent_window_max_turns;
                let ratio = self.config.recent_window_token_ratio;
                let window_model = self.resolve_window_model(&effective_model);
                match Self::focus_history_for_new_turn(
                    &history_chat,
                    &window_model,
                    min_turns,
                    max_turns,
                    ratio,
                ) {
                    Some(view) => {
                        let full = history_chat.clone();
                        let rolling = self
                            .build_rolling_summary(
                                &view.dropped,
                                view.dropped_turns,
                                &effective_model,
                            )
                            .await;
                        let combined = match (view.retained_summary, rolling) {
                            (Some(a), Some(b)) => Some(format!("{a}\n\n{b}")),
                            (Some(a), None) => Some(a),
                            (None, Some(b)) => Some(b),
                            (None, None) => None,
                        };
                        let mut messages = view.messages;
                        if let Some(text) = combined {
                            Self::fold_summary_into_system(
                                &mut messages,
                                view.leading_system_end,
                                &text,
                            );
                        }
                        history_chat = messages;
                        focus_base_len = history_chat.len();
                        Some(full)
                    }
                    None => None,
                }
            };

        let cancel = self.cancel_signal.load_full().as_ref().clone();
        let live_cfg = self.shared_config.load_ref();
        let multimodal = live_cfg.multimodal.clone();
        let pacing = live_cfg.pacing.clone();
        let dedup_exempt = live_cfg.agent.tool_call_dedup_exempt.clone();
        let tool_filter_groups = live_cfg.agent.tool_filter_groups.clone();
        drop(live_cfg);
        let excluded_tools: Vec<String> =
            crate::agent::tool_handler::filter::compute_excluded_mcp_tools(
                &self.tools,
                &tool_filter_groups,
                user_message,
            );

        let hook_runner_arc = self.hook_runner.as_ref().and_then(|h| h.current());
        let hook_runner_ref = hook_runner_arc.as_deref();

        let final_text = loop {
            let provider_name = self.cached_provider.clone();
            let gui_hooks: Arc<GuiHooksFromAgent> = Arc::new(GuiHooksFromAgent::from_agent(self));

            let loop_result = {
                let policy = crate::agent::loop_::policy::PolicyBundle::gui(
                    self.provider.as_ref(),
                    &self.tools,
                    self.observer.as_ref(),
                    provider_name.as_str(),
                    effective_model.as_str(),
                    &multimodal,
                    &pacing,
                    &excluded_tools,
                    &dedup_exempt,
                )
                .with_temperature(self.temperature)
                .with_max_iterations(self.config.max_tool_iterations)
                .with_cancellation(Some(cancel.clone()))
                .with_event_sink(crate::agent::event_sink::EventSink::turn(event_tx.clone()))
                .with_activated_tools(self.activated_tools.as_ref())
                .with_hooks(hook_runner_ref)
                .with_rbac(self.rbac_engine.as_ref(), self.rbac_identity.as_ref())
                .with_model_switch_callback(Some(crate::agent::loop_::get_model_switch_state()))
                .with_response_cache_hook(Some(gui_hooks.clone()))
                .with_memory_session_hook(Some(gui_hooks.clone()))
                .with_turn_preamble_hook(Some(gui_hooks.clone()))
                .with_gui_model_switch_hook(Some(gui_hooks.clone()))
                .with_iteration_context_budget_hook(Some(gui_hooks.clone()))
                .with_experience_recorder_hook(Some(gui_hooks.clone()))
                .with_plan_mode_nudge_hook(Some(gui_hooks.clone()))
                .with_plan_mode_flag(Some(&self.plan_mode_flag))
                .with_plan_execution_path(armed_plan_path.as_deref());

                crate::agent::loop_::unified::UnifiedLoop::new(policy)
                    .run(&mut history_chat)
                    .await
            };

            match loop_result {
                Ok(text) => break text,
                Err(err) => {
                    if crate::agent::model_switch::is_model_switch_requested(&err).is_some() {
                        tracing::info!(
                            target: "runtime_model_switch",
                            "GUI turn received model switch request mid-turn; applying switch and retrying instead of failing"
                        );
                        self.apply_gui_model_switch(&event_tx).await;
                        effective_model = self.model_name.clone();
                        continue;
                    }
                    let msg = err.to_string();
                    let downcast = err.downcast_ref::<AgentError>();
                    let is_cancelled = matches!(downcast, Some(AgentError::TurnCancelled))
                        || crate::agent::error_classify::classify_turn_error_code(&msg)
                            == "CANCELLED";
                    if is_cancelled {
                        let _ = event_tx
                            .send(TurnEvent::Cancelling {
                                reason: msg.clone(),
                            })
                            .await;
                        self.commit_interrupted_turn_history(
                            full_history_for_merge.clone(),
                            history_chat.clone(),
                            focus_base_len,
                        );
                        self.capture_unfinished_task();
                        self.trim_history();
                        self.last_turn_interrupted = true;
                        return Err(AgentError::TurnCancelled);
                    }
                    let is_transport_interruption =
                        matches!(downcast, Some(AgentError::StreamInterrupted(_)));
                    let _ = event_tx
                        .send(TurnEvent::Error {
                            message: msg.clone(),
                        })
                        .await;

                    if is_transport_interruption {
                        self.commit_interrupted_turn_history(
                            full_history_for_merge.clone(),
                            history_chat.clone(),
                            focus_base_len,
                        );
                        self.capture_unfinished_task();
                        self.trim_history();
                        self.last_turn_interrupted = true;
                        return Err(AgentError::StreamInterrupted(msg));
                    }
                    self.record_failed_turn_reinforcement(&msg);
                    self.commit_interrupted_turn_history(
                        full_history_for_merge.clone(),
                        history_chat.clone(),
                        focus_base_len,
                    );
                    crate::agent::dangling_tool_repair::note_failed_turn(&mut self.history);
                    self.capture_unfinished_task();
                    self.trim_history();
                    self.last_turn_interrupted = true;
                    return Err(AgentError::ToolDispatchFailed(msg));
                }
            }
        };

        if let Some(mut full) = full_history_for_merge {
            if plan_exec_mode {
                full.push(crate::providers::traits::ChatMessage::assistant(
                    final_text.clone(),
                ));
            } else {
                let new_turn_start = Self::new_turn_slice_start(&history_chat, focus_base_len);
                if history_chat.len() > new_turn_start {
                    full.extend(history_chat[new_turn_start..].iter().cloned());
                }
            }
            Self::replace_history_from_flat(&mut self.history, full);
        } else {
            Self::replace_history_from_flat(&mut self.history, history_chat);
        }
        self.trim_history();

        if std::mem::take(&mut self.last_turn_resumed) {
            self.clear_unfinished_task();
        }

        crate::evolution::record_provider_model(
            Some(self.cached_provider.as_str()),
            Some(effective_model.as_str()),
        );
        crate::evolution::set_response_text(&final_text);

        _turn_metrics.mark_ok();
        Ok(final_text)
    }

    fn new_turn_slice_start(
        partial_history: &[crate::providers::traits::ChatMessage],
        focus_base_len: usize,
    ) -> usize {
        partial_history
            .iter()
            .rposition(|m| Self::is_user_turn_boundary(m) && m.has_current_request_marker())
            .map(|i| i + 1)
            .unwrap_or_else(|| focus_base_len.min(partial_history.len()))
    }

    fn merge_interrupted_flat(
        full_history: Option<Vec<crate::providers::traits::ChatMessage>>,
        partial_history: Vec<crate::providers::traits::ChatMessage>,
        focus_base_len: usize,
    ) -> Vec<crate::providers::traits::ChatMessage> {
        match full_history {
            Some(mut full) => {
                let new_turn_start = Self::new_turn_slice_start(&partial_history, focus_base_len);
                if partial_history.len() > new_turn_start {
                    full.extend(partial_history[new_turn_start..].iter().cloned());
                }
                full
            }
            None => partial_history,
        }
    }

    fn commit_interrupted_turn_history(
        &mut self,
        plan_exec_full_history: Option<Vec<crate::providers::traits::ChatMessage>>,
        partial_history: Vec<crate::providers::traits::ChatMessage>,
        plan_exec_focus_base_len: usize,
    ) {
        let merged = Self::merge_interrupted_flat(
            plan_exec_full_history,
            partial_history,
            plan_exec_focus_base_len,
        );
        Self::replace_history_from_flat(&mut self.history, merged);
        let has_unfinished = self.has_unfinished_task();
        crate::agent::dangling_tool_repair::close_orphan_user_turns(
            &mut self.history,
            has_unfinished,
        );
    }

    fn capture_unfinished_task(&self) {
        self.capture_unfinished_task_from(&self.history);
    }

    fn capture_unfinished_task_from(&self, history: &[ConversationMessage]) {
        let flat = self.tool_dispatcher.to_provider_messages(history);
        let request = flat
            .iter()
            .rev()
            .find(|m| Self::is_user_turn_boundary(m))
            .map(|m| Self::extract_user_request_snippet(&m.content));
        let Some(request) = request else {
            return;
        };
        if request.trim().is_empty() {
            return;
        }
        let progress = Self::summarize_interrupted_progress(history);
        if let Ok(mut guard) = self.unfinished_task.lock() {
            let seq = guard.as_ref().map(|t| t.seq + 1).unwrap_or(1);
            tracing::info!(
                target: "agent.intent",
                seq,
                request = %request,
                "captured most-recent unfinished task (overwrites any older one)"
            );
            *guard = Some(UnfinishedTask {
                seq,
                request,
                progress,
            });
        }
    }

    fn last_user_boundary_index(history: &[ConversationMessage]) -> Option<usize> {
        history.iter().rposition(|m| match m {
            ConversationMessage::Chat(c) => {
                if c.role != "user" {
                    return false;
                }
                let trimmed = c.content.trim_start();
                !trimmed.starts_with("[Tool results]") && !trimmed.starts_with("[Recovered")
            }
            _ => false,
        })
    }

    fn tool_call_arg_snippet(arguments: &str) -> String {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) {
            for key in [
                "query", "url", "path", "pattern", "command", "file_path", "name", "q",
            ] {
                if let Some(field) = value.get(key).and_then(|v| v.as_str()) {
                    let field = field.trim();
                    if !field.is_empty() {
                        return format!("{key}={}", Self::truncate_snippet(field, 100));
                    }
                }
            }
        }
        Self::truncate_snippet(arguments.trim(), 100)
    }

    fn summarize_interrupted_progress(history: &[ConversationMessage]) -> String {
        let start = Self::last_user_boundary_index(history)
            .map(|i| i + 1)
            .unwrap_or(0);

        let mut steps: Vec<(String, String)> = Vec::new();
        let mut result_snippets: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut last_said = String::new();

        for msg in &history[start.min(history.len())..] {
            match msg {
                ConversationMessage::AssistantToolCalls {
                    text, tool_calls, ..
                } => {
                    if let Some(t) = text.as_deref() {
                        let t = t.trim();
                        if !t.is_empty()
                            && !crate::agent::dangling_tool_repair::is_turn_close_note(t)
                        {
                            last_said = Self::truncate_snippet(t, 240);
                        }
                    }
                    for call in tool_calls {
                        let snippet = Self::tool_call_arg_snippet(&call.arguments);
                        let desc = if snippet.is_empty() {
                            format!("called {}", call.name)
                        } else {
                            format!("called {} ({snippet})", call.name)
                        };
                        steps.push((call.id.clone(), desc));
                    }
                }
                ConversationMessage::ToolResults(rows) => {
                    for row in rows {
                        let flattened = row.content.split_whitespace().collect::<Vec<_>>().join(" ");
                        result_snippets.insert(
                            row.tool_call_id.clone(),
                            Self::truncate_snippet(&flattened, 120),
                        );
                    }
                }
                ConversationMessage::Chat(c) => {
                    if c.role == "assistant" {
                        let t = c.content.trim();
                        if !t.is_empty()
                            && !crate::agent::dangling_tool_repair::is_turn_close_note(t)
                        {
                            last_said = Self::truncate_snippet(t, 240);
                        }
                    }
                }
            }
        }

        let mut out = String::new();
        if !steps.is_empty() {
            let _ = std::fmt::Write::write_str(
                &mut out,
                "already executed (full results are in the transcript above; do NOT repeat these):\n",
            );
            for (id, desc) in steps.iter().take(20) {
                match result_snippets.get(id) {
                    Some(snippet) if !snippet.trim().is_empty() => {
                        let _ = std::fmt::Write::write_fmt(
                            &mut out,
                            format_args!("  - {desc} -> result: {snippet}\n"),
                        );
                    }
                    Some(_) => {
                        let _ = std::fmt::Write::write_fmt(
                            &mut out,
                            format_args!("  - {desc} -> result returned\n"),
                        );
                    }
                    None => {
                        let _ = std::fmt::Write::write_fmt(
                            &mut out,
                            format_args!("  - {desc} -> no result captured\n"),
                        );
                    }
                }
            }
        }
        if !last_said.trim().is_empty() {
            let _ =
                std::fmt::Write::write_fmt(&mut out, format_args!("last said: {last_said}\n"));
        }
        Self::truncate_snippet(out.trim_end(), 1200)
    }

    fn clear_unfinished_task(&self) {
        if let Ok(mut guard) = self.unfinished_task.lock() {
            *guard = None;
        }
    }

    fn has_unfinished_task(&self) -> bool {
        self.unfinished_task
            .lock()
            .ok()
            .is_some_and(|g| g.is_some())
    }

    fn extract_user_request_snippet(envelope: &str) -> String {
        let body = if let Some(idx) = envelope.find("[CURRENT REQUEST") {
            let after = &envelope[idx..];
            match after.find("]\n") {
                Some(close) => &after[close + 2..],
                None => after,
            }
        } else {
            envelope
        };
        let body = body.split("\n\n[").next().unwrap_or(body);
        Self::truncate_snippet(body.trim(), 400)
    }

    fn truncate_snippet(text: &str, max_chars: usize) -> String {
        let trimmed = text.trim();
        if trimmed.chars().count() <= max_chars {
            return trimmed.to_string();
        }
        let truncated: String = trimmed.chars().take(max_chars).collect();
        format!("{truncated}\u{2026}")
    }

    fn cap_memory_context(context: String) -> String {
        const MAX_CONTEXT_CHARS: usize = 16_000;
        if context.chars().count() <= MAX_CONTEXT_CHARS {
            return context;
        }
        let original_chars = context.len();
        let truncated: String = context.chars().take(MAX_CONTEXT_CHARS).collect();
        tracing::warn!(
            target: "agent.recent_window",
            original_chars,
            cap = MAX_CONTEXT_CHARS,
            "memory recall context exceeded cap; truncated to keep the user envelope bounded"
        );
        format!("{truncated}\n[memory context truncated to {MAX_CONTEXT_CHARS} chars]\n")
    }

    fn unfinished_task_note(task: &UnfinishedTask) -> String {
        let mut note = String::from(
            "[UNFINISHED EARLIER TASK \u{2014} this is the MOST RECENT task in THIS session that was \
             interrupted (stopped, cancelled, or errored) before it finished. It is the ONLY task a \
             \"continue\" request should resume; ignore any older interrupted/unfinished tasks:\n  \
             request: ",
        );
        note.push_str(&task.request);
        if !task.progress.trim().is_empty() {
            note.push_str("\n  ALREADY DONE (this is your real progress; the listed steps and their \
                           results already exist in the transcript above):\n");
            for line in task.progress.lines() {
                note.push_str("  ");
                note.push_str(line);
                note.push('\n');
            }
        }
        note.push_str(
            "\nResume THIS task ONLY if the latest user message explicitly asks to continue or \
             finish it (e.g. \"继续\" / \"continue\" / \"接着\" / \"go on\") or clearly refers to it. \
             When you resume, CONTINUE FROM WHERE IT STOPPED: build on the results already obtained \
             above, do the NEXT step toward finishing the request, and do NOT re-issue the same \
             tool calls already listed under ALREADY DONE or restart the task from the beginning. \
             If the listed work is enough to answer, synthesize the final answer directly instead \
             of searching again. Otherwise (the latest message is a greeting, small talk, or a new \
             or unrelated request) answer that message literally and do NOT resume this task on \
             your own. The full earlier transcript stays in memory and is available on demand via \
             sessions_outline + sessions_history.]",
        );
        note
    }

    async fn analyze_intent_llm(
        &self,
        user_message: &str,
    ) -> Option<crate::agent::intent::LlmIntentDecision> {
        if !self.intent_analysis_config.enabled {
            return None;
        }
        if user_message.trim().is_empty() {
            return None;
        }

        const RECENT_SOURCE_WINDOW: usize = 30;
        let window_start = self.history.len().saturating_sub(RECENT_SOURCE_WINDOW);
        let flat = self
            .tool_dispatcher
            .to_provider_messages(&self.history[window_start..]);
        const RECENT_TAIL_MESSAGES: usize = 6;
        let mut recent_tail: Vec<(String, String)> = Vec::new();
        for m in flat.iter().rev() {
            if recent_tail.len() >= RECENT_TAIL_MESSAGES {
                break;
            }
            let role = m.role.as_str();
            if role != "user" && role != "assistant" {
                continue;
            }
            if m.content.trim().is_empty() {
                continue;
            }
            if crate::agent::dangling_tool_repair::is_turn_close_note(&m.content) {
                continue;
            }
            let text = if role == "user" {
                if !Self::is_user_turn_boundary(m) {
                    continue;
                }
                Self::extract_user_request_snippet(&m.content)
            } else {
                if m.content.contains("\"tool_calls\"") {
                    continue;
                }
                Self::truncate_snippet(&m.content, 300)
            };
            if text.trim().is_empty() {
                continue;
            }
            recent_tail.push((role.to_string(), text));
        }
        recent_tail.reverse();

        let candidate = self
            .unfinished_task
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|t| (t.seq, t.request.clone(), t.progress.clone())));

        let prompt = crate::agent::intent::build_intent_user_prompt(
            user_message,
            &recent_tail,
            candidate
                .as_ref()
                .map(|(seq, request, digest)| (*seq, request.as_str(), digest.as_str())),
        );

        let summary_model = crate::services::try_get_services().and_then(|svc| {
            svc.config()
                .agent
                .context_compression
                .summary_model
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
        });
        let intent_model = self
            .intent_analysis_config
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .or(summary_model.as_deref())
            .unwrap_or(&self.model_name);
        let timeout = std::time::Duration::from_secs(5);
        let raw = match tokio::time::timeout(
            timeout,
            self.provider.chat_with_system(
                Some(crate::agent::intent::INTENT_SYSTEM_PROMPT),
                &prompt,
                intent_model,
                0.0,
            ),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                tracing::warn!(
                    target: "agent.intent",
                    error = %e,
                    "llm intent analysis failed; falling back to heuristic classification"
                );
                return None;
            }
            Err(_) => {
                tracing::warn!(
                    target: "agent.intent",
                    "llm intent analysis timed out; falling back to heuristic classification"
                );
                return None;
            }
        };

        match crate::agent::intent::parse_llm_intent_decision(&raw) {
            Some(decision) => {
                tracing::debug!(
                    target: "agent.intent",
                    decision = decision.decision.as_str(),
                    resume_task_seq = ?decision.resume_task_seq,
                    task_intent = decision.task_intent.as_str(),
                    confidence = decision.confidence,
                    reason = %decision.reason,
                    "llm intent decision for turn"
                );
                Some(decision)
            }
            None => {
                tracing::warn!(
                    target: "agent.intent",
                    raw = %Self::truncate_snippet(&raw, 200),
                    "llm intent analysis returned unparseable output; falling back to heuristic"
                );
                None
            }
        }
    }

    pub async fn resolve_auto_coding_mode(
        &self,
        user_message: &str,
    ) -> crate::agent::coding_mode::CodingMode {
        if let Some(decision) = self.analyze_intent_llm(user_message).await {
            let mode = decision.coding_mode();
            if let Ok(mut guard) = self.pending_intent_decision.lock() {
                *guard = Some((
                    user_message.to_string(),
                    std::time::Instant::now(),
                    decision,
                ));
            }
            return mode;
        }
        crate::agent::intent::auto_select_coding_mode(user_message)
    }

    fn take_pending_intent_decision(
        &self,
        user_message: &str,
    ) -> Option<crate::agent::intent::LlmIntentDecision> {
        const PENDING_DECISION_FRESH_WINDOW: std::time::Duration =
            std::time::Duration::from_secs(3);
        let mut guard = self.pending_intent_decision.lock().ok()?;
        match guard.take() {
            Some((msg, stashed_at, decision))
                if msg == user_message
                    || stashed_at.elapsed() < PENDING_DECISION_FRESH_WINDOW =>
            {
                Some(decision)
            }
            _ => None,
        }
    }

    fn rollback_failed_turn_history(&mut self, rollback_len: usize) {
        if self.history.len() > rollback_len {
            let dropped = self.history.len() - rollback_len;
            self.history.truncate(rollback_len);
            tracing::warn!(
                target: "agent.turn",
                dropped,
                rollback_len,
                "turn failed; rolled back the failed user turn from history so it does not pollute subsequent context (consistent with legacy truncate semantics; the failure is still surfaced to the session via the returned error)"
            );
        }
    }

    fn replace_history_from_flat(
        history: &mut Vec<ConversationMessage>,
        flat: Vec<ChatMessage>,
    ) {
        let mut out: Vec<ConversationMessage> = Vec::with_capacity(flat.len());
        let mut tool_batch: Vec<crate::providers::traits::ToolResultMessage> = Vec::new();
        for msg in flat {
            if msg.role != "tool" && !tool_batch.is_empty() {
                out.push(ConversationMessage::ToolResults(std::mem::take(
                    &mut tool_batch,
                )));
            }
            match msg.role.as_str() {
                "tool" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg.content) {
                        if let (Some(id), Some(content_v)) = (
                            v.get("tool_call_id").and_then(|x| x.as_str()),
                            v.get("content"),
                        ) {
                            let content_str = match content_v {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            tool_batch.push(crate::providers::traits::ToolResultMessage {
                                tool_call_id: id.to_string(),
                                content: content_str,
                            });
                            continue;
                        }
                    }
                    out.push(ConversationMessage::Chat(msg));
                }
                "assistant" => {
                    if let Ok(v) =
                        serde_json::from_str::<serde_json::Value>(msg.content.trim())
                    {
                        if let Some(tc_arr) =
                            v.get("tool_calls").and_then(|x| x.as_array())
                        {
                            if !tc_arr.is_empty() {
                                let text = v
                                    .get("content")
                                    .and_then(|x| x.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(String::from);
                                let reasoning = v
                                    .get("reasoning_content")
                                    .and_then(|x| x.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(String::from);
                                let tool_calls: Vec<
                                    crate::providers::traits::ToolCall,
                                > = tc_arr
                                    .iter()
                                    .filter_map(|tc| {
                                        serde_json::from_value(tc.clone()).ok()
                                    })
                                    .collect();
                                out.push(ConversationMessage::AssistantToolCalls {
                                    text,
                                    tool_calls,
                                    reasoning_content: reasoning,
                                });
                                continue;
                            }
                        }
                    }
                    out.push(ConversationMessage::Chat(msg));
                }
                _ => {
                    out.push(ConversationMessage::Chat(msg));
                }
            }
        }
        if !tool_batch.is_empty() {
            out.push(ConversationMessage::ToolResults(tool_batch));
        }
        crate::agent::dangling_tool_repair::drop_payloadless_assistant_messages(&mut out);
        *history = out;
    }

    pub fn history(&self) -> &[ConversationMessage] {
        &self.history
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn set_mode_tool_filter(&mut self, filter: Option<std::collections::HashSet<String>>) {

        self.mode_tool_filter = filter;
        self.mode_filter_dirty = true;
    }

    pub fn current_coding_mode(&self) -> Option<crate::agent::coding_mode::CodingMode> {
        self.current_coding_mode
    }

    pub fn set_coding_mode(&mut self, mode: crate::agent::coding_mode::CodingMode) {
        let prev = self.current_coding_mode;
        self.current_coding_mode = Some(mode);

        let filter = mode
            .allowed_tools()
            .map(|set| set.into_iter().map(String::from).collect());
        self.set_mode_tool_filter(filter);

        if let Some(prev_mode) = prev {
            if prev_mode != mode {
                let contract = mode.system_prompt_injection();
                let body = format!(
                    "[Mode Switch] Now operating in {} mode.\n{}",
                    mode.label(),
                    contract.trim_start()
                );
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::system(body)));

                if matches!(mode, crate::agent::coding_mode::CodingMode::Plan) {
                    let reset_body = "[Plan-Mode Reset] Disregard any prior \"Step N completed\", \"开始执行 Step N\", \"Starting step …\", \"executing\", \"已完成\" or other execution-voice framing inherited from earlier turns or other modes. You are now ONLY drafting/refining a plan document; no step has been executed yet, no work is in progress, and the user has not clicked Build. Speak strictly in planning voice (\"will\", \"propose\", \"draft\", \"would\"); never claim any todo is finished, never narrate progress. If the user has not asked you anything new, simply wait  -  do not start a fake execution recap.";
                    self.history.push(ConversationMessage::Chat(
                        ChatMessage::system(reset_body.to_string()),
                    ));
                }

                tracing::info!(
                    target: "agent.mode",
                    from = %prev_mode,
                    to = %mode,
                    "coding mode switched mid-turn  -  contract pushed to history"
                );
            }
        }
    }

    pub fn arm_plan_execution(&self, plan_path: impl Into<String>) {
        *self.plan_execution_armed.lock() = Some(plan_path.into());
    }

    pub fn mark_resuming_from_ask(&self) {
        self.resuming_from_ask
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn cancel_token(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub fn cancel_signal(&self) -> tokio_util::sync::CancellationToken {
        self.cancel_signal.load_full().as_ref().clone()
    }

    pub fn cancel_signal_handle(
        &self,
    ) -> Arc<arc_swap::ArcSwap<tokio_util::sync::CancellationToken>> {
        Arc::clone(&self.cancel_signal)
    }

    pub fn request_cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.cancel_signal.load_full().cancel();
    }

    pub fn reset_cancel(&self) {
        self.cancelled
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let token = self.cancel_signal.load_full();
        if token.is_cancelled() {
            self.cancel_signal
                .store(Arc::new(tokio_util::sync::CancellationToken::new()));
        }
    }

    pub fn set_max_iterations_override(&mut self, max: usize) {
        self.config.max_tool_iterations = if max == 0 {
            self.baseline_max_tool_iterations
        } else {
            max
        };
    }

    fn apply_mode_filter(&mut self) {
        if !self.mode_filter_dirty {
            return;
        }
        let specs: Vec<ToolSpec> = if let Some(ref allowed) = self.mode_tool_filter {
            self.tools
                .iter()
                .filter(|t| allowed.contains(t.name()))
                .map(|t| t.spec())
                .collect()
        } else {
            self.tools.iter().map(|t| t.spec()).collect()
        };
        self.tool_specs = std::sync::Arc::new(crate::tools::dedupe_tool_specs(&specs));
        self.rebuild_tool_index();
        self.mode_filter_dirty = false;
    }

    fn effective_provider_config(&self) -> std::sync::Arc<crate::config::Config> {
        let base = self.shared_config.load_ref();
        let Some((provider_id, model)) = self.runtime_selection_override.as_ref() else {
            return base;
        };
        if base.default_provider.as_deref() == Some(provider_id.as_str())
            && base.default_model.as_deref() == Some(model.as_str())
        {
            return base;
        }
        let resolved_profile_id = if base.model_providers.contains_key(provider_id.as_str()) {
            Some(provider_id.clone())
        } else {
            base.model_providers
                .iter()
                .find(|(pid, profile)| {
                    pid.eq_ignore_ascii_case(provider_id)
                        || profile
                            .preset_id
                            .as_deref()
                            .map(|p| p.eq_ignore_ascii_case(provider_id))
                            .unwrap_or(false)
                })
                .map(|(pid, _)| pid.clone())
        };
        let mut cfg = (*base).clone();
        if let Some(pid) = resolved_profile_id {
            if cfg.apply_model_provider_profile(&pid) {
                cfg.default_model = Some(model.clone());
                return std::sync::Arc::new(cfg);
            }
        }
        if cfg.default_provider.as_deref() == Some(provider_id.as_str()) {
            cfg.default_model = Some(model.clone());
            return std::sync::Arc::new(cfg);
        }
        tracing::warn!(
            target = "runtime_model_switch",
            provider = %provider_id,
            model = %model,
            "session runtime selection points to an unknown provider profile; falling back to global defaults"
        );
        base
    }

    fn sync_config_from_store(&mut self) -> ConfigChange {
        let config = self.effective_provider_config();

        self.temperature = config.default_temperature;
        let resolved_model = providers::resolve_default_model(&config);
        let new_model_opt: Option<String> = match resolved_model {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(target = "config", "{}", e);
                None
            }
        };

        self.config.max_history_messages = config.agent.max_history_messages;
        self.config.recent_turn_window = config.agent.recent_turn_window;
        self.config.recent_window_max_turns = config.agent.recent_window_max_turns;
        self.config.recent_window_token_ratio = config.agent.recent_window_token_ratio;
        self.config.recent_window_summary_enabled = config.agent.recent_window_summary_enabled;
        self.config.recent_window_summary_batch_turns = config.agent.recent_window_summary_batch_turns;
        self.intent_analysis_config = config.intent_analysis.clone();

        let new_provider = config
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".to_string());
        let new_api_key = config.api_key.clone().unwrap_or_default();
        let new_api_url = config.api_url.clone().unwrap_or_default();

        let provider_changed = new_provider != self.cached_provider;
        let api_key_changed = !self.cached_api_key.constant_time_eq(&new_api_key);
        let api_url_changed = new_api_url != self.cached_api_url;
        let model_changed = match new_model_opt.as_ref() {
            Some(m) => m != &self.model_name,
            None => false,
        };

        if provider_changed || api_key_changed || api_url_changed {
            tracing::info!(
                "Provider config changed: provider={}->{}, api_key={}, api_url={}",
                self.cached_provider,
                new_provider,
                if api_key_changed {
                    "(changed)"
                } else {
                    "(unchanged)"
                },
                if api_url_changed {
                    "(changed)"
                } else {
                    "(unchanged)"
                }
            );
            self.cached_provider = new_provider.clone();
            self.cached_api_key = crate::security::secret_string::SecretString::new(new_api_key);
            self.cached_api_url = new_api_url.clone();

            if let Some(new_model) = new_model_opt.clone() {
                if model_changed {
                    self.model_name = new_model;
                }
            }

            tracing::debug!(
                "Config synced (hard): provider={}, model={}, temperature={}",
                self.cached_provider,
                self.model_name,
                self.temperature
            );
            ConfigChange::Hard
        } else if model_changed
            || (self.temperature - config.default_temperature).abs() > f64::EPSILON
        {
            if let Some(new_model) = new_model_opt {
                self.model_name = new_model;
            }
            tracing::debug!(
                "Config synced (soft): model={}, temperature={}",
                self.model_name,
                self.temperature
            );
            ConfigChange::Soft
        } else {
            ConfigChange::None
        }
    }

    async fn build_critic_eval_provider(
        config: &Config,
        provider_name: &str,
        provider_runtime_options: &providers::ProviderRuntimeOptions,
    ) -> Option<std::sync::Arc<dyn Provider>> {
        let eval_model = config
            .self_eval
            .evaluator_model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        let eval_provider_id =
            crate::tools::media::credentials::provider_for_model(config, eval_model)?;
        if eval_provider_id == provider_name {
            return None;
        }
        let resolved =
            crate::tools::media::credentials::resolve(config, Some(&eval_provider_id), eval_model);
        let eval_wire_name =
            providers::resolve_runtime_provider_name(&eval_provider_id, config);
        match providers::create_resilient_provider_with_options_async(
            eval_wire_name,
            resolved.api_key.clone(),
            Some(resolved.base_url.clone()),
            config.reliability.clone(),
            provider_runtime_options.clone(),
        )
        .await
        {
            Ok(p) => Some(std::sync::Arc::from(p)),
            Err(e) => {
                tracing::warn!(
                    provider = eval_provider_id.as_str(),
                    model = eval_model,
                    error = %e,
                    "failed to build dedicated evaluator provider; reusing main provider"
                );
                None
            }
        }
    }

    pub async fn reload_provider(&mut self) -> Result<()> {

        let config = self.effective_provider_config();

        let provider_name_raw = config
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".to_string());
        let provider_name = providers::resolve_runtime_provider_name(&provider_name_raw, &config);
        let api_key = config.api_key.clone().unwrap_or_default();
        let api_url = config.api_url.clone().unwrap_or_default();
        let model_name = providers::resolve_default_model(&config)?;
        let reliability = config.reliability.clone();
        let model_routes = config.model_routes.clone();
        let provider_runtime_options = providers::provider_runtime_options_from_config(&config);

        let provider_name_for_blocking = provider_name;
        let api_key_for_blocking = api_key.clone();
        let api_url_for_blocking = api_url.clone();
        let model_name_for_blocking = model_name.clone();
        let reliability_for_blocking = reliability;
        let model_routes_for_blocking = model_routes;
        let options_for_blocking = provider_runtime_options;
        let new_provider = tokio::task::spawn_blocking(move || {
            providers::create_routed_provider_with_options(
                &provider_name_for_blocking,
                if api_key_for_blocking.is_empty() {
                    None
                } else {
                    Some(api_key_for_blocking.as_str())
                },
                if api_url_for_blocking.is_empty() {
                    None
                } else {
                    Some(api_url_for_blocking.as_str())
                },
                &reliability_for_blocking,
                &model_routes_for_blocking,
                &model_name_for_blocking,
                &options_for_blocking,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("Provider reload task failed: {}", e))?
        .map_err(|e| anyhow::anyhow!("Failed to create provider: {}", e))?;

        self.provider = new_provider;
        let model_actually_changed = model_name != self.model_name;
        self.model_name = model_name;

        self.cached_provider = provider_name_raw;
        self.cached_api_key = crate::security::secret_string::SecretString::new(api_key);
        self.cached_api_url = api_url;

        if model_actually_changed {
            self.refresh_history_system_prompt();
        }

        tracing::info!("Provider reloaded successfully");
        Ok(())
    }

    pub fn signal_runtime_model_switch(&mut self, provider: String, model: String) {
        let trimmed_provider = provider.trim();
        let trimmed_model = model.trim();
        if !trimmed_provider.is_empty() && !trimmed_model.is_empty() {
            self.runtime_selection_override =
                Some((trimmed_provider.to_string(), trimmed_model.to_string()));
        }
        if !trimmed_provider.is_empty() {
            self.cached_provider = trimmed_provider.to_string();
        }
        if !trimmed_model.is_empty() && trimmed_model != self.model_name {
            tracing::info!(
                target = "runtime_model_switch",
                old_model = %self.model_name,
                new_model = %trimmed_model,
                provider = %trimmed_provider,
                "UI-initiated model switch: updating in-memory model_name"
            );
            self.model_name = trimmed_model.to_string();
            self.refresh_history_system_prompt();
        }
    }

    pub async fn apply_runtime_config_now(&mut self) -> Result<()> {
        let config = self.effective_provider_config();
        let new_provider = config
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".to_string());
        let new_api_key = config.api_key.clone().unwrap_or_default();
        let new_api_url = config.api_url.clone().unwrap_or_default();
        let new_model = providers::resolve_default_model(&config).ok();
        drop(config);

        let model_unchanged = match new_model.as_deref() {
            Some(m) => m == self.model_name,
            None => true,
        };
        let unchanged = new_provider == self.cached_provider
            && self.cached_api_key.constant_time_eq(&new_api_key)
            && new_api_url == self.cached_api_url
            && model_unchanged;
        if unchanged {
            return Ok(());
        }

        self.reload_provider().await?;
        self.refresh_history_system_prompt();
        Ok(())
    }

    fn refresh_history_system_prompt(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let new_prompt = match self.build_system_prompt() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target = "runtime_model_switch",
                    error = %e,
                    "failed to rebuild system prompt after model switch"
                );
                return;
            }
        };
        if let Some(ConversationMessage::Chat(chat)) = self.history.first_mut() {
            if chat.role == "system" && chat.content != new_prompt {
                chat.content = new_prompt;
            }
        }
    }

    pub fn tool_specs(&self) -> &[ToolSpec] {
        &self.tool_specs
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> crate::agent::dispatcher::ToolExecutionResult {
        let parsed = crate::agent::dispatcher::ParsedToolCall {
            name: name.to_string(),
            arguments,
            tool_call_id: Some(uuid::Uuid::new_v4().to_string()),
            parse_error: false,
        };
        self.execute_tool_call(&parsed).await
    }

    pub fn set_memory_session_id(&mut self, session_id: Option<String>) {
        self.memory_session_id = session_id;
    }

    pub fn current_workspace_dir(&self) -> &std::path::Path {
        &self.workspace_dir
    }

    pub fn set_session_workspace_dir(&mut self, path: std::path::PathBuf) {
        if path.as_os_str().is_empty() {
            return;
        }
        tracing::info!(path = %path.display(), "agent: using per-session workspace directory");
        if let Some(ref p) = self.desktop_security_policy {
            p.retarget_session_workspace_root(path.clone());
            self.security_summary = Some(p.prompt_summary());
        }
        crate::security::register_workspace_root(&path);
        self.workspace_dir = path.clone();

        crate::agent::token::optimizer::ensure_workspace_optimizer(path.clone());

        self.reload_skills_for_workspace(&path);
    }

    pub fn reload_skills_for_workspace(&mut self, workspace_dir: &std::path::Path) {
        let config = self.shared_config.load();
        let new_skills =
            crate::skills::load_skills_with_config(workspace_dir, config.as_ref());

        self.tools
            .retain(|t| !t.name().contains('.') && t.name() != "read_skill");
        if let Some(ref policy) = self.desktop_security_policy {
            crate::tools::register_skill_tools(
                &mut self.tools,
                &new_skills,
                std::sync::Arc::clone(policy),
            );
        }

        if matches!(
            config.skills.prompt_injection_mode,
            crate::config::SkillsPromptInjectionMode::Compact
        ) {
            self.tools.push(Box::new(crate::tools::ReadSkillTool::new(
                workspace_dir.to_path_buf(),
                config.skills.open_skills_enabled,
                config.skills.open_skills_dir.clone(),
                config.skills.disabled_skills.clone(),
            )));
        }

        let specs: Vec<crate::tools::ToolSpec> =
            self.tools.iter().map(|t| t.spec()).collect();
        self.tool_specs = std::sync::Arc::new(crate::tools::dedupe_tool_specs(&specs));
        self.rebuild_tool_index();
        self.mode_filter_dirty = true;
        self.skills = new_skills;
    }

    pub fn set_rbac_session(
        &mut self,
        engine: Option<Arc<crate::security::rbac::RbacEngine>>,
        identity: Option<crate::security::rbac::CallerIdentity>,
    ) {
        self.rbac_engine = engine;
        self.rbac_identity = identity;
    }

    pub fn set_hook_runner(
        &mut self,
        runner: Option<std::sync::Arc<crate::hooks::HotHookRunner>>,
    ) {
        self.hook_runner = runner;
    }

    fn compute_tools_signature(config: &crate::config::Config) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        config.mcp.enabled.hash(&mut hasher);
        config.mcp.deferred_loading.hash(&mut hasher);
        for s in &config.mcp.servers {
            s.name.hash(&mut hasher);
            s.command.hash(&mut hasher);
            s.url.hash(&mut hasher);
            s.enabled.hash(&mut hasher);
            for a in &s.args {
                a.hash(&mut hasher);
            }

            let mut env_pairs: Vec<(&String, &String)> = s.env.iter().collect();
            env_pairs.sort_unstable();
            for (k, v) in env_pairs {
                k.hash(&mut hasher);
                v.hash(&mut hasher);
            }
            let mut hdr_pairs: Vec<(&String, &String)> = s.headers.iter().collect();
            hdr_pairs.sort_unstable();
            for (k, v) in hdr_pairs {
                k.hash(&mut hasher);
                v.hash(&mut hasher);
            }
        }
        for t in &config.custom_tools.tools {
            t.name.hash(&mut hasher);
            t.description.hash(&mut hasher);
        }
        config.web_search.enabled.hash(&mut hasher);
        config.web_fetch.enabled.hash(&mut hasher);

        let mut agent_keys: Vec<&String> = config.agents.keys().collect();
        agent_keys.sort();
        for k in agent_keys {
            k.hash(&mut hasher);
            if let Some(cfg) = config.agents.get(k) {
                cfg.provider.hash(&mut hasher);
                cfg.model.hash(&mut hasher);
                cfg.agentic.hash(&mut hasher);
                cfg.max_depth.hash(&mut hasher);
            }
        }

        for t in &config.custom_tools.tools {
            t.enabled.hash(&mut hasher);
            t.command.hash(&mut hasher);
            for a in &t.args {
                a.hash(&mut hasher);
            }
            t.cwd.hash(&mut hasher);
            t.timeout_secs.hash(&mut hasher);
            t.schema.to_string().hash(&mut hasher);
        }

        let mut disabled = config.skills.disabled_skills.clone();
        disabled.sort();
        for d in &disabled {
            d.hash(&mut hasher);
        }
        format!("{:?}", config.skills.prompt_injection_mode).hash(&mut hasher);
        config.skills.allow_scripts.hash(&mut hasher);
        config.skills.open_skills_enabled.hash(&mut hasher);
        config.skills.open_skills_dir.hash(&mut hasher);

        config.lsp.enabled.hash(&mut hasher);
        for entry in &config.lsp.servers {
            entry.id.hash(&mut hasher);
            entry.language_id.hash(&mut hasher);
            entry.enabled.hash(&mut hasher);
            entry.managed.hash(&mut hasher);
            entry.command.hash(&mut hasher);
            for a in &entry.args {
                a.hash(&mut hasher);
            }

            serde_json::to_string(&entry.install_state)
                .unwrap_or_default()
                .hash(&mut hasher);
        }
        hasher.finish()
    }

    fn refresh_delegate_agents_from_config(&mut self, config: &crate::config::Config) {
        for tool in &self.tools {
            if let Some(any_ref) = tool.as_any() {
                if let Some(dt) = any_ref.downcast_ref::<crate::tools::DelegateTool>() {
                    dt.refresh_agents(config.agents.clone());
                    tracing::info!(
                        target: "agent.delegate",
                        agents = config.agents.len(),
                        "DelegateTool subagent table refreshed from live config"
                    );
                    break;
                }
            }
        }
    }

    pub async fn sync_tools_from_config_if_changed(&mut self) {
        let config_arc = self.shared_config.load_ref();
        let signature = Self::compute_tools_signature(&config_arc);
        if signature == self.cached_tools_signature {
            return;
        }
        tracing::info!(
            target: "agent.tools",
            old = self.cached_tools_signature,
            new = signature,
            "tools signature changed; reloading MCP / custom / web tools"
        );
        self.reload_mcp_tools_inner(&config_arc).await;
        self.reload_custom_tools_inner(&config_arc);
        self.reload_web_tools_inner(&config_arc);
        self.refresh_delegate_agents_from_config(&config_arc);

        let workspace_for_skills = self
            .desktop_security_policy
            .as_ref()
            .map(|p| p.workspace_root_handle().read().clone())
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| config_arc.workspace_dir.clone());
        self.reload_skills_for_workspace(&workspace_for_skills);

        let specs: Vec<crate::tools::ToolSpec> =
            self.tools.iter().map(|t| t.spec()).collect();
        self.tool_specs = std::sync::Arc::new(crate::tools::dedupe_tool_specs(&specs));
        self.rebuild_tool_index();
        self.mode_filter_dirty = true;
        self.cached_tools_signature = signature;
    }

    fn rebuild_tool_index(&mut self) {
        self.tool_index = self
            .tools
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name().to_string(), i))
            .collect();
    }

    fn compute_mcp_signature(config: &crate::config::Config) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        config.mcp.enabled.hash(&mut hasher);
        config.mcp.deferred_loading.hash(&mut hasher);
        serde_json::to_string(&config.mcp.servers)
            .unwrap_or_default()
            .hash(&mut hasher);
        hasher.finish()
    }

    async fn attach_mcp_registry(
        &mut self,
        registry: std::sync::Arc<crate::tools::McpRegistry>,
        deferred_loading: bool,
    ) {
        crate::tools::mcp::client::register_global_registry(std::sync::Arc::clone(&registry));
        if deferred_loading {
            let deferred_set = crate::tools::DeferredMcpToolSet::from_registry(
                std::sync::Arc::clone(&registry),
            )
            .await;
            let activated = std::sync::Arc::new(parking_lot::Mutex::new(
                crate::tools::ActivatedToolSet::new(),
            ));
            self.activated_tools = Some(std::sync::Arc::clone(&activated));
            self.tools.push(Box::new(crate::tools::ToolSearchTool::new(
                deferred_set,
                activated,
            )));
        } else {
            let names = registry.tool_names();
            let mut registered = 0usize;
            for name in names {
                if let Some(def) = registry.get_tool_def(&name).await {
                    let wrapper: std::sync::Arc<dyn crate::tools::Tool> =
                        std::sync::Arc::new(crate::tools::McpToolWrapper::new(
                            name,
                            def,
                            std::sync::Arc::clone(&registry),
                        ));
                    self.tools
                        .push(Box::new(crate::tools::ArcToolRef(wrapper)));
                    registered += 1;
                }
            }
            tracing::info!(
                registered,
                servers = registry.server_count(),
                "MCP reload: tools registered"
            );
        }
    }

    pub async fn try_attach_pending_mcp(&mut self) {
        let config_arc = self.shared_config.load_ref();
        let current_sig = Self::compute_mcp_signature(&config_arc);
        let pending = {
            let mut guard = self.pending_mcp_registry.lock();
            match guard.take() {
                Some((sig, registry)) if sig == current_sig => Some(registry),
                Some(other) => {
                    *guard = Some(other);
                    None
                }
                None => None,
            }
        };
        let Some(registry) = pending else {
            return;
        };
        let has_mcp_tools = self.tools.iter().any(|t| {
            let name = t.name();
            name == "tool_search"
                || (name.contains("__")
                    && name.split_once("__").map_or(false, |(h, _)| !h.is_empty()))
                || name.starts_with("mcp_")
        });
        if has_mcp_tools {
            return;
        }
        tracing::info!(
            target: "agent.tools",
            servers = registry.server_count(),
            "attaching MCP registry pre-warmed in the background"
        );
        self.attach_mcp_registry(registry, config_arc.mcp.deferred_loading)
            .await;
        let specs: Vec<crate::tools::ToolSpec> = self.tools.iter().map(|t| t.spec()).collect();
        self.tool_specs = std::sync::Arc::new(crate::tools::dedupe_tool_specs(&specs));
        self.rebuild_tool_index();
        self.mode_filter_dirty = true;
    }

    async fn reload_mcp_tools_inner(&mut self, config: &crate::config::Config) {

        self.tools.retain(|t| {
            let name = t.name();
            !(name == "tool_search"
                || (name.contains("__") && name.split_once("__").map_or(false, |(h, _)| !h.is_empty()))
                || name.starts_with("mcp_"))
        });
        if !config.mcp.enabled || config.mcp.servers.is_empty() {
            return;
        }
        let mcp_sig = Self::compute_mcp_signature(config);
        let prewarmed = {
            let mut guard = self.pending_mcp_registry.lock();
            match guard.take() {
                Some((sig, registry)) if sig == mcp_sig => Some(registry),
                Some(other) => {
                    *guard = Some(other);
                    None
                }
                None => None,
            }
        };
        if let Some(registry) = prewarmed {
            self.attach_mcp_registry(registry, config.mcp.deferred_loading)
                .await;
            return;
        }
        const MCP_RELOAD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
        match tokio::time::timeout(
            MCP_RELOAD_DEADLINE,
            crate::tools::McpRegistry::connect_all(&config.mcp.servers),
        )
        .await
        {
            Ok(Ok(registry)) => {
                self.attach_mcp_registry(
                    std::sync::Arc::new(registry),
                    config.mcp.deferred_loading,
                )
                .await;
            }
            Ok(Err(e)) => {
                tracing::error!("MCP reload failed to initialise registry: {e:#}");
            }
            Err(_) => {
                tracing::warn!(
                    deadline_secs = MCP_RELOAD_DEADLINE.as_secs(),
                    "MCP reload exceeded deadline; continuing this turn without MCP tools and \
                     pre-warming the registry in the background"
                );
                let pending_slot = std::sync::Arc::clone(&self.pending_mcp_registry);
                let servers = config.mcp.servers.clone();
                let _ = crate::runtime::spawn_supervised("agent.mcp.prewarm", async move {
                    match crate::tools::McpRegistry::connect_all(&servers).await {
                        Ok(registry) => {
                            *pending_slot.lock() =
                                Some((mcp_sig, std::sync::Arc::new(registry)));
                        }
                        Err(e) => {
                            tracing::error!(
                                "background MCP pre-warm failed to initialise registry: {e:#}"
                            );
                        }
                    }
                });
            }
        }
    }

    fn reload_custom_tools_inner(&mut self, config: &crate::config::Config) {

        self.tools.retain(|t| !t.name().starts_with("custom_"));

        let workspace_root = match self.desktop_security_policy.as_ref() {
            Some(policy) => policy.workspace_root_handle(),
            None => std::sync::Arc::new(parking_lot::RwLock::new(config.workspace_dir.clone())),
        };

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for tool in self.tools.iter() {
            seen.insert(tool.name().to_string());
        }
        for def in &config.custom_tools.tools {
            if !def.enabled {
                continue;
            }
            let validation_errors = def.validate();
            if !validation_errors.is_empty() {
                tracing::warn!(
                    target: "agent.custom_tools",
                    name = %def.name,
                    errors = ?validation_errors,
                    "custom_tools: skipping invalid entry on hot reload"
                );
                continue;
            }
            let registered = format!("custom_{}", def.name.trim());
            if !seen.insert(registered.clone()) {
                tracing::warn!(
                    target: "agent.custom_tools",
                    name = %registered,
                    "custom_tools: duplicate tool name on hot reload, skipping"
                );
                continue;
            }
            self.tools
                .push(Box::new(crate::tools::custom_tool::CustomTool::from_def(
                    def,
                    std::sync::Arc::clone(&workspace_root),
                )));
        }
    }

    fn reload_web_tools_inner(&mut self, config: &crate::config::Config) {

        let want_web_search = config.web_search.enabled;
        let has_web_search = self.tools.iter().any(|t| t.name() == "web_search_tool");
        if want_web_search && !has_web_search {
            self.tools.push(Box::new(
                crate::tools::WebSearchTool::new_with_config(
                    config.web_search.provider.clone(),
                    config.web_search.brave_api_key.clone(),
                    config.web_search.searxng_instance_url.clone(),
                    config.web_search.max_results,
                    config.web_search.timeout_secs,
                    config.config_path.clone(),
                    config.secrets.encrypt,
                ),
            ));
        } else if !want_web_search && has_web_search {
            self.tools.retain(|t| t.name() != "web_search_tool");
        }

        let want_web_fetch = config.web_fetch.enabled;
        let has_web_fetch = self.tools.iter().any(|t| t.name() == "web_fetch");
        if want_web_fetch && !has_web_fetch {
            if let Some(ref policy) = self.desktop_security_policy {
                self.tools.push(Box::new(crate::tools::WebFetchTool::new(
                    std::sync::Arc::clone(policy),
                    config.web_fetch.allowed_domains.clone(),
                    config.web_fetch.blocked_domains.clone(),
                    config.web_fetch.max_response_size,
                    config.web_fetch.timeout_secs,
                    config.web_fetch.firecrawl.clone(),
                    config.web_fetch.allowed_private_hosts.clone(),
                )));
            }
        } else if !want_web_fetch && has_web_fetch {
            self.tools.retain(|t| t.name() != "web_fetch");
        }
    }

    pub fn add_node_tools_from_registry(
        &mut self,
        registry: std::sync::Arc<crate::gateway::nodes::NodeRegistry>,
    ) {
        for (node_id, _, cap) in registry.all_capabilities() {
            let prefixed = crate::tools::NodeTool::tool_name(&node_id, &cap.name);
            if let Some(ref allow) = self.allowed_tools {
                if !allow.iter().any(|a| a == &prefixed) {
                    continue;
                }
            }
            self.tools.push(Box::new(crate::tools::NodeTool::new(
                node_id.clone(),
                cap.name.clone(),
                cap.description.clone(),
                cap.parameters.clone(),
                std::sync::Arc::clone(&registry),
            )));
        }
        let specs: Vec<ToolSpec> = self.tools.iter().map(|t| t.spec()).collect();
        self.tool_specs = std::sync::Arc::new(crate::tools::dedupe_tool_specs(&specs));
        self.rebuild_tool_index();
    }

    pub fn seed_history(&mut self, messages: &[ChatMessage]) {
        if self.history.is_empty() {
            if let Ok(sys) = self.build_system_prompt() {
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::system(sys)));
            }
        }
        let mirrored =
            crate::providers::sanitize::mirror_tool_ids_in_chat_messages(messages.to_vec());
        let cleaned =
            crate::providers::sanitize::clean_empty_assistant_tool_calls_in_chat_messages(mirrored);
        let mut expanded =
            super::sqlite_gateway_hydrate::hydrate_gateway_sqlite_messages(&cleaned);
        Self::repair_orphan_tool_result_messages(&mut expanded);
        crate::agent::dangling_tool_repair::drop_payloadless_assistant_messages(&mut expanded);
        let tail_interrupted =
            crate::agent::dangling_tool_repair::tail_signals_interrupted_turn(&expanded);
        if tail_interrupted {
            self.last_turn_interrupted = true;
        }
        self.activate_deferred_tools_from_history(&expanded);
        self.history.extend(expanded);
        if tail_interrupted && !self.has_unfinished_task() {
            self.capture_unfinished_task_from(&self.history);
        }
    }

    fn is_user_turn_boundary(m: &crate::providers::traits::ChatMessage) -> bool {
        if m.role != "user" {
            return false;
        }
        let trimmed = m.content.trim_start();
        !trimmed.starts_with("[Tool results]") && !trimmed.starts_with("[Recovered")
    }

    fn focus_history_for_new_turn(
        history: &[crate::providers::traits::ChatMessage],
        model: &str,
        min_turns: usize,
        max_turns: usize,
        token_ratio: f64,
    ) -> Option<RecentFocusView> {
        let n = history.len();
        let leading_system_end = history
            .iter()
            .position(|m| m.role != "system")
            .unwrap_or(n);
        let boundaries: Vec<usize> = history
            .iter()
            .enumerate()
            .filter(|(_, m)| Self::is_user_turn_boundary(m))
            .map(|(i, _)| i)
            .collect();
        if boundaries.is_empty() {
            return None;
        }

        let min_turns = min_turns.max(1);
        let max_turns = max_turns.max(min_turns);
        let window_tokens = crate::constants::api_limits::context_window_for_model(model) as f64;
        let token_budget = (window_tokens * token_ratio.clamp(0.01, 1.0)).max(1.0) as usize;
        let system_tokens =
            crate::providers::traits::estimate_total_tokens(&history[..leading_system_end]);

        let total_b = boundaries.len();
        let current_start = boundaries[total_b - 1];

        let (current_kept, current_dropped) =
            Self::bound_current_turn(&history[current_start..], token_budget);
        let mut acc_tokens = crate::providers::traits::estimate_total_tokens(&current_kept);
        let mut chosen = total_b - 1;
        let mut turns = 1usize;
        let floor_cap = token_budget.saturating_mul(2);
        for bi in (0..total_b - 1).rev() {
            let turn_start = boundaries[bi];
            let turn_end = boundaries[bi + 1];
            let turn_tokens =
                crate::providers::traits::estimate_total_tokens(&history[turn_start..turn_end]);
            let next_turns = turns + 1;
            if next_turns > max_turns {
                break;
            }
            let prospective = acc_tokens + turn_tokens;
            let within_budget = prospective <= token_budget;
            let within_floor = next_turns <= min_turns && prospective <= floor_cap;
            if !within_budget && !within_floor {
                break;
            }
            chosen = bi;
            acc_tokens = prospective;
            turns = next_turns;
        }

        if let Some(note_pos) = history[..current_start].iter().rposition(|m| {
            m.role == "assistant"
                && crate::agent::dangling_tool_repair::is_turn_close_note(&m.content)
        }) {
            if let Some(interrupted_bi) = boundaries.iter().rposition(|&b| b < note_pos) {
                if interrupted_bi < chosen {
                    let pinned_tokens = crate::providers::traits::estimate_total_tokens(
                        &history[boundaries[interrupted_bi]..current_start],
                    );
                    let current_kept_tokens =
                        crate::providers::traits::estimate_total_tokens(&current_kept);
                    let projected = system_tokens + pinned_tokens + current_kept_tokens;
                    if projected <= floor_cap {
                        chosen = interrupted_bi;
                        turns = total_b - chosen;
                        acc_tokens = pinned_tokens + current_kept_tokens;
                        tracing::debug!(
                            target: "agent.recent_window",
                            interrupted_boundary = boundaries[chosen],
                            "pinning most-recent interrupted turn into the focus window so its real \
                             transcript stays visible for resume"
                        );
                    } else {
                        tracing::debug!(
                            target: "agent.recent_window",
                            projected,
                            cap = floor_cap,
                            "interrupted turn too large to pin whole; relying on the captured \
                             progress digest instead of forcing it into the window"
                        );
                    }
                }
            }
        }

        let start_idx = boundaries[chosen];
        if start_idx <= leading_system_end && current_dropped.is_empty() {
            return None;
        }

        let mut retained_summary_parts: Vec<String> = Vec::new();
        let mut dropped: Vec<crate::providers::traits::ChatMessage> = Vec::new();
        let mut dropped_turns = 0usize;
        for m in &history[leading_system_end..start_idx] {
            if m.content.trim_start().starts_with("[CONTEXT SUMMARY") {
                retained_summary_parts.push(m.content.clone());
            } else {
                if Self::is_user_turn_boundary(m) {
                    dropped_turns += 1;
                }
                dropped.push(m.clone());
            }
        }
        let current_dropped_len = current_dropped.len();
        dropped.extend(current_dropped);
        let retained_summary = if retained_summary_parts.is_empty() {
            None
        } else {
            Some(retained_summary_parts.join("\n\n"))
        };

        let mut messages: Vec<crate::providers::traits::ChatMessage> = Vec::with_capacity(
            leading_system_end + (current_start - start_idx) + current_kept.len(),
        );
        messages.extend_from_slice(&history[..leading_system_end]);
        messages.extend_from_slice(&history[start_idx..current_start]);
        messages.extend(current_kept);

        let chars_before: usize = history.iter().map(|m| m.content.len()).sum();
        let chars_after: usize = messages.iter().map(|m| m.content.len()).sum();
        Self::log_window_tail(&messages);
        tracing::info!(
            target: "agent.recent_window",
            kept_turns = turns,
            dropped_turns,
            current_turn_dropped = current_dropped_len,
            token_budget,
            est_window_tokens = system_tokens + acc_tokens,
            chars_before,
            chars_after,
            "focusing LLM context on leading system prompt + token-bounded recent turns; older \
             history stays in memory/persistence and is available on demand via sessions tools"
        );

        Some(RecentFocusView {
            messages,
            leading_system_end,
            retained_summary,
            dropped,
            dropped_turns,
        })
    }

    fn bound_current_turn(
        slice: &[crate::providers::traits::ChatMessage],
        token_budget: usize,
    ) -> (
        Vec<crate::providers::traits::ChatMessage>,
        Vec<crate::providers::traits::ChatMessage>,
    ) {
        if slice.len() <= 1
            || crate::providers::traits::estimate_total_tokens(slice) <= token_budget
        {
            return (slice.to_vec(), Vec::new());
        }
        let head = slice[0].clone();
        let head_tokens =
            crate::providers::traits::estimate_total_tokens(std::slice::from_ref(&head));
        let tail_budget = token_budget.saturating_sub(head_tokens);
        let mut acc = 0usize;
        let mut tail_start = slice.len();
        for i in (1..slice.len()).rev() {
            let t = crate::providers::traits::estimate_total_tokens(std::slice::from_ref(&slice[i]));
            if acc + t > tail_budget && tail_start < slice.len() {
                break;
            }
            acc += t;
            tail_start = i;
        }
        let dropped: Vec<crate::providers::traits::ChatMessage> =
            slice[1..tail_start].to_vec();
        let mut kept = Vec::with_capacity(1 + (slice.len() - tail_start));
        kept.push(head);
        kept.extend_from_slice(&slice[tail_start..]);
        (kept, dropped)
    }

    fn log_window_tail(messages: &[crate::providers::traits::ChatMessage]) {
        if !tracing::enabled!(target: "agent.recent_window", tracing::Level::DEBUG) {
            return;
        }
        let start = messages.len().saturating_sub(20);
        for (offset, m) in messages[start..].iter().enumerate() {
            let prefix: String = m.content.chars().take(60).collect();
            tracing::debug!(
                target: "agent.recent_window",
                idx = start + offset,
                role = %m.role,
                len = m.content.len(),
                prefix = %prefix.replace('\n', " "),
                "window tail message"
            );
        }
    }

    fn fold_summary_into_system(
        messages: &mut Vec<crate::providers::traits::ChatMessage>,
        leading_system_end: usize,
        text: &str,
    ) {
        let block = format!(
            "[CONTEXT SUMMARY \u{2014} earlier conversation in this session, condensed for \
             continuity. The full transcript stays in memory and is available on demand via \
             sessions_outline + sessions_history.]\n{text}"
        );
        if leading_system_end > 0 {
            if let Some(sys) = messages.get_mut(leading_system_end - 1) {
                if !sys.content.trim_end().is_empty() {
                    sys.content.push_str("\n\n");
                }
                sys.content.push_str(&block);
                return;
            }
        }
        messages.insert(0, crate::providers::traits::ChatMessage::system(block));
    }

    fn fingerprint_dropped_head(dropped: &[crate::providers::traits::ChatMessage]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let head = dropped.len().min(8);
        head.hash(&mut hasher);
        for m in &dropped[..head] {
            m.role.hash(&mut hasher);
            m.content.hash(&mut hasher);
        }
        hasher.finish()
    }

    async fn build_rolling_summary(
        &self,
        dropped: &[crate::providers::traits::ChatMessage],
        dropped_turns: usize,
        model: &str,
    ) -> Option<String> {
        if !self.config.recent_window_summary_enabled || dropped.is_empty() || dropped_turns == 0 {
            return None;
        }
        let batch = self.config.recent_window_summary_batch_turns.max(1);
        let fingerprint = Self::fingerprint_dropped_head(dropped);
        {
            let guard = self
                .rolling_summary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((fp, covered, text)) = guard.as_ref() {
                if *fp == fingerprint && !text.is_empty() {
                    let stale = *covered > dropped_turns || dropped_turns - *covered >= batch;
                    if stale {
                        let source_budget =
                            self.config.context_compression.source_max_chars.max(1);
                        let mut acc_chars = 0usize;
                        let mut slice_start = dropped.len();
                        for (i, m) in dropped.iter().enumerate().rev() {
                            acc_chars = acc_chars.saturating_add(m.content.len());
                            slice_start = i;
                            if acc_chars >= source_budget {
                                break;
                            }
                        }
                        let mut refresh = self
                            .rolling_summary_refresh
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        *refresh = Some(RollingSummaryRefreshJob {
                            recent_dropped: dropped[slice_start..].to_vec(),
                            fingerprint,
                            dropped_turns,
                            model: model.to_string(),
                        });
                    }
                    return Some(text.clone());
                }
            }
        }
        let source_budget = self.config.context_compression.source_max_chars.max(1);
        let mut acc_chars = 0usize;
        let mut slice_start = dropped.len();
        for (i, m) in dropped.iter().enumerate().rev() {
            acc_chars = acc_chars.saturating_add(m.content.len());
            slice_start = i;
            if acc_chars >= source_budget {
                break;
            }
        }
        let recent_dropped = &dropped[slice_start..];

        {
            let mut refresh = self
                .rolling_summary_refresh
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *refresh = Some(RollingSummaryRefreshJob {
                recent_dropped: recent_dropped.to_vec(),
                fingerprint,
                dropped_turns,
                model: model.to_string(),
            });
        }
        let placeholder = Self::degraded_rolling_summary_placeholder(recent_dropped, dropped_turns);
        {
            let mut guard = self
                .rolling_summary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let keep_existing = guard
                .as_ref()
                .is_some_and(|(fp, covered, t)| *fp == fingerprint && *covered > 0 && !t.is_empty());
            if !keep_existing {
                *guard = Some((fingerprint, 0, placeholder.clone()));
            }
            guard
                .as_ref()
                .filter(|(fp, _, _)| *fp == fingerprint)
                .map(|(_, _, t)| t.clone())
                .or(Some(placeholder))
        }
    }

    fn degraded_rolling_summary_placeholder(
        recent_dropped: &[crate::providers::traits::ChatMessage],
        dropped_turns: usize,
    ) -> String {
        const PER_MSG_CHARS: usize = 500;
        const MAX_MSGS: usize = 12;
        let mut lines: Vec<String> = Vec::new();
        let start = recent_dropped.len().saturating_sub(MAX_MSGS);
        for m in &recent_dropped[start..] {
            let body = m.content.trim();
            if body.is_empty() {
                continue;
            }
            let snippet = crate::util::truncate_str_bytes(body, PER_MSG_CHARS);
            let ellipsis = if body.len() > snippet.len() { "\u{2026}" } else { "" };
            lines.push(format!("- {}: {}{}", m.role, snippet.replace('\n', " "), ellipsis));
        }
        format!(
            "[CONTEXT SUMMARY \u{2014} {dropped_turns} earlier turn(s), full summary pending] \
             (degraded excerpt; a complete structured summary is being generated in the \
             background and will replace this next turn)\n{}",
            lines.join("\n")
        )
    }

    fn focus_history_for_plan_execution(
        history: Vec<crate::providers::traits::ChatMessage>,
    ) -> Vec<crate::providers::traits::ChatMessage> {
        let Some(last_user_idx) = history.iter().rposition(|m| m.role == "user") else {
            return history;
        };
        let first_system_idx = history.iter().position(|m| m.role == "system");
        let dropped = history
            .len()
            .saturating_sub(1 + usize::from(first_system_idx.is_some()));
        if dropped == 0 {
            return history;
        }
        let chars_before: usize = history.iter().map(|m| m.content.len()).sum();
        let system_chars = first_system_idx.map(|i| history[i].content.len()).unwrap_or(0);
        let trigger_chars = history[last_user_idx].content.len();
        let mut focused: Vec<crate::providers::traits::ChatMessage> = Vec::with_capacity(2);
        if let Some(i) = first_system_idx {
            focused.push(history[i].clone());
        }
        focused.push(history[last_user_idx].clone());
        let chars_after: usize = focused.iter().map(|m| m.content.len()).sum();
        tracing::info!(
            target: "agent.plan_execution",
            dropped,
            chars_before,
            chars_after,
            system_chars,
            trigger_chars,
            "plan execution armed: focusing context on the base system prompt + plan trigger \
             only, dropping the entire conversation backlog (including accumulated system \
             reminders / summaries) so the model executes the plan on a clean, minimal context \
             instead of being overwhelmed or answering stale questions"
        );
        focused
    }

    fn activate_deferred_tools_from_history(&self, history: &[ConversationMessage]) {
        if self.activated_tools.is_none() {
            return;
        }
        let surface = self.surface;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut builtin_pending: Vec<(String, crate::tools::ToolSpec)> = Vec::new();
        let mut mcp_pending: Vec<String> = Vec::new();
        for msg in history {
            if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = msg {
                for tc in tool_calls {
                    if !seen.insert(tc.name.clone()) {
                        continue;
                    }
                    if tc.name.contains("__") {
                        mcp_pending.push(tc.name.clone());
                        continue;
                    }
                    let entry = crate::tools::handler::tier::classify(&tc.name, surface);
                    if !matches!(
                        entry.tier,
                        crate::tools::handler::tier::BuiltinToolTier::OnDemand
                    ) {
                        continue;
                    }
                    if let Some(spec) = self
                        .tools
                        .iter()
                        .find(|t| t.name() == tc.name)
                        .map(|t| t.spec())
                    {
                        builtin_pending.push((tc.name.clone(), spec));
                    }
                }
            }
        }
        if builtin_pending.is_empty() && mcp_pending.is_empty() {
            return;
        }
        let mut activated_now: Vec<String> = Vec::new();
        if !builtin_pending.is_empty() {
            if let Some(ref activated_arc) = self.activated_tools {
                let mut guard = activated_arc.lock();
                for (name, spec) in builtin_pending {
                    if guard.is_activated(&name) {
                        continue;
                    }
                    guard.activate_spec(name.clone(), spec);
                    activated_now.push(name);
                }
            }
        }
        if !mcp_pending.is_empty() {
            if let Some(tool_search_tool) = self
                .tool_index
                .get("tool_search")
                .map(|&i| &self.tools[i])
            {
                if let Some(any_ref) = tool_search_tool.as_any() {
                    if let Some(ts) =
                        any_ref.downcast_ref::<crate::tools::ToolSearchTool>()
                    {
                        for name in mcp_pending {
                            if ts.activate_from_history(&name) {
                                activated_now.push(name);
                            }
                        }
                    }
                }
            }
        }
        if !activated_now.is_empty() {
            tracing::info!(
                target: "agent.tool_search.replay",
                count = activated_now.len(),
                names = ?activated_now,
                "replayed deferred tool activations from hydrated history"
            );
        }
    }

    pub async fn from_config(
        config: &Config,
        denied_tools: Option<Vec<String>>,
        shared_config: Option<crate::config::live::LiveConfig>,
    ) -> Result<Self> {
        if crate::services::governance::credential_vault::try_get_credential_vault().is_none() {
            let anchor = if config.workspace_dir.as_os_str().is_empty() {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            } else {
                config.workspace_dir.clone()
            };
            if let Err(err) = crate::services::governance::credential_vault::init_credential_vault(&anchor) {
                tracing::warn!(error = %err, "credential vault initialisation failed for agent session");
            }
        }

        if config.evolution.enabled && crate::evolution::try_global().is_none() {
            if let Err(err) = crate::evolution::init_global(
                config.workspace_dir.clone(),
                config.evolution.clone(),
            ) {
                tracing::warn!(error = %err, "evolution engine initialisation failed for agent session");
            }
        }

        let observer: Arc<dyn Observer> =
            Arc::from(observability::create_observer(&config.observability));
        let runtime: Arc<dyn runtime::RuntimeAdapter> =
            Arc::from(runtime::create_runtime(&config.runtime)?);
        let security = Arc::new(SecurityPolicy::from_config(
            &config.autonomy,
            &config.workspace_dir,
        ));

        let memory: Arc<dyn Memory> = Arc::from(
            memory::create_memory_with_storage_and_routes_async(
                config.memory.clone(),
                config.embedding_routes.clone(),
                Some(config.storage.provider.config.clone()),
                config.workspace_dir.clone(),
                config.api_key.clone(),
            )
            .await?,
        );

        let composio_key = if config.composio.enabled {
            config.composio.api_key.as_deref()
        } else {
            None
        };
        let composio_entity_id = if config.composio.enabled {
            Some(config.composio.entity_id.as_str())
        } else {
            None
        };

        let (
            mut tools,
            delegate_handle,
            _reaction_handle,
            _channel_map_handle,
            _ask_user_handle,
            _escalate_handle,
            plan_mode_flag,
        ) = tools::all_tools_with_runtime(
            Arc::new(config.clone()),
            &security,
            runtime,
            memory.clone(),
            composio_key,
            composio_entity_id,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &config.workspace_dir,
            &config.agents,
            config.api_key.as_deref(),
            config,
            None,
        );

        let mut activated_tools: Option<Arc<parking_lot::Mutex<tools::ActivatedToolSet>>> = None;
        let (builtin_deferred_enabled, mcp_deferred_enabled) =
            crate::tools::deferred_loading_effective(config);
        if config.mcp.enabled && !config.mcp.servers.is_empty() {
            tracing::info!(
                "Initializing MCP client  -  {} server(s) configured",
                config.mcp.servers.len()
            );
            match tools::McpRegistry::connect_all(&config.mcp.servers).await {
                Ok(registry) => {
                    let registry = std::sync::Arc::new(registry);
                    crate::tools::mcp::client::register_global_registry(std::sync::Arc::clone(
                        &registry,
                    ));
                    if mcp_deferred_enabled {
                        let deferred_set = tools::DeferredMcpToolSet::from_registry(
                            std::sync::Arc::clone(&registry),
                        )
                        .await;
                        tracing::info!(
                            "MCP deferred: {} tool stub(s) from {} server(s)",
                            deferred_set.len(),
                            registry.server_count()
                        );
                        let activated =
                            Arc::new(parking_lot::Mutex::new(tools::ActivatedToolSet::new()));
                        activated_tools = Some(Arc::clone(&activated));
                        tools.push(Box::new(tools::ToolSearchTool::new(
                            deferred_set,
                            activated,
                        )));
                    } else {
                        let names = registry.tool_names();
                        let mut registered = 0usize;
                        for name in names {
                            if let Some(def) = registry.get_tool_def(&name).await {
                                let wrapper: std::sync::Arc<dyn tools::Tool> =
                                    std::sync::Arc::new(tools::McpToolWrapper::new(
                                        name,
                                        def,
                                        std::sync::Arc::clone(&registry),
                                    ));
                                if let Some(ref handle) = delegate_handle {
                                    handle.write().push(std::sync::Arc::clone(&wrapper));
                                }
                                tools.push(Box::new(tools::ArcToolRef(wrapper)));
                                registered += 1;
                            }
                        }
                        tracing::info!(
                            "MCP: {} tool(s) registered from {} server(s)",
                            registered,
                            registry.server_count()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("MCP registry failed to initialize: {e:#}");
                }
            }
        }

        if builtin_deferred_enabled {
            let mut deferred_section_unused = String::new();
            let workspace_key = crate::session::workspace_key_from_path(
                &config.workspace_dir,
                "default",
            );
            let options = crate::tools::BuiltinDeferredRegistrationOptions {
                workspace_key: workspace_key.clone(),
                allowlist: Vec::new(),
                gate: None,
                config: Some(config),
            };
            let deferred_builtin_set =
                crate::tools::apply_builtin_deferred_registration_with_options(
                    &mut tools,
                    &mut deferred_section_unused,
                    crate::tools::ToolSurfaceBaseline::Both,
                    &mut activated_tools,
                    options,
                );

            if let (Some(activated_handle), Some(svc)) =
                (activated_tools.as_ref(), crate::services::try_get_services())
            {
                match svc.tool_activation_store.load(&workspace_key).await {
                    Ok(names) => {
                        if !names.is_empty() {
                            let mut guard = activated_handle.lock();
                            for name in &names {
                                if guard.is_activated(name) {
                                    continue;
                                }
                                if let Some(spec) = deferred_builtin_set.tool_spec(name) {
                                    guard.activate_spec(name.clone(), spec);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "tool_activation_store",
                            workspace_key = %workspace_key,
                            error = %e,
                            "failed to preload activated tools"
                        );
                    }
                }
            }
        }

        let provider_name_raw = config.default_provider.as_deref().unwrap_or("openrouter");
        let provider_name =
            providers::resolve_runtime_provider_name(provider_name_raw, config);

        let model_name = providers::resolve_default_model(config)?;

        let provider_runtime_options = providers::provider_runtime_options_from_config(config);

        let provider: Box<dyn Provider> = providers::create_routed_provider_with_options_async(
            provider_name.clone(),
            config.api_key.clone(),
            config.api_url.clone(),
            config.reliability.clone(),
            config.model_routes.clone(),
            model_name.clone(),
            provider_runtime_options.clone(),
        )
        .await?;

        if config.self_eval.enabled {
            match providers::create_resilient_provider_with_options_async(
                provider_name.clone(),
                config.api_key.clone(),
                config.api_url.clone(),
                config.reliability.clone(),
                provider_runtime_options.clone(),
            )
            .await
            {
                Ok(critic_provider) => {
                    let critic_eval_provider = Self::build_critic_eval_provider(
                        config,
                        &provider_name,
                        &provider_runtime_options,
                    )
                    .await;
                    crate::agent::flows::set_global_critic_context(
                        crate::agent::self_assess::critic::CriticContext::new(
                            std::sync::Arc::from(critic_provider),
                            model_name.clone(),
                            config.self_eval.clone(),
                        )
                        .with_eval_provider(critic_eval_provider),
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "failed to build critic provider for GUI session; self-eval gate disabled this session"
                    );
                }
            }
        }

        let dispatcher_choice = config.agent.tool_dispatcher.as_str();
        let tool_dispatcher: Box<dyn ToolDispatcher> = match dispatcher_choice {
            "native" => Box::new(NativeToolDispatcher),
            "xml" => Box::new(XmlToolDispatcher),
            _ if provider.supports_native_tools() => Box::new(NativeToolDispatcher),
            _ => Box::new(XmlToolDispatcher),
        };

        let route_model_by_hint: HashMap<String, String> = config
            .model_routes
            .iter()
            .map(|route| (route.hint.clone(), route.model.clone()))
            .collect();
        let available_hints: Vec<String> = route_model_by_hint.keys().cloned().collect();

        let response_cache = if config.memory.response_cache_enabled {
            let cache_ws = config.workspace_dir.clone();
            let cache_ttl = config.memory.response_cache_ttl_minutes;
            let cache_max = config.memory.response_cache_max_entries;
            let cache_hot = config.memory.response_cache_hot_entries;
            tokio::task::spawn_blocking(move || {
                crate::memory::response_cache::ResponseCache::with_hot_cache(
                    &cache_ws, cache_ttl, cache_max, cache_hot,
                )
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .map(Arc::new)
        } else {
            None
        };

        crate::agent::token::optimizer::ensure_global_optimizer_from_config(config);

        crate::token_saver::set_enabled(config.token_saver.enabled);
        crate::token_saver::set_global(config.token_saver.to_runtime_ctx());
        crate::guardrails::ensure_global_guardrails(config.guardrails.clone());

        let experience_replay = if config.experience.enabled {
            Some(crate::agent::reward::experience::ExperienceReplay::new(
                &config.experience,
            ))
        } else {
            None
        };

        if let Some(ref deny_list) = denied_tools {
            let deny_set: std::collections::HashSet<_> = deny_list.iter().cloned().collect();
            tools.retain(|t| !deny_set.contains(t.name()));
        }

        let loaded_skills = {
            let skills_workspace = config.workspace_dir.clone();
            let skills_config = config.clone();
            tokio::task::spawn_blocking(move || {
                crate::skills::load_skills_with_config(&skills_workspace, &skills_config)
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "skill loading task failed; continuing without skills");
                Vec::new()
            })
        };

        Agent::builder()
            .provider(provider)
            .tools(tools)
            .memory(memory)
            .observer(observer)
            .response_cache(response_cache)
            .tool_dispatcher(tool_dispatcher)
            .memory_loader(Box::new(DefaultMemoryLoader::new(
                5,
                config.memory.min_relevance_score,
            )))
            .prompt_builder(SystemPromptBuilder::with_defaults())
            .config(config.agent.clone())
            .model_name(model_name)
            .temperature(config.default_temperature)
            .workspace_dir(config.workspace_dir.clone())
            .classification_config(config.query_classification.clone())
            .available_hints(available_hints)
            .route_model_by_hint(route_model_by_hint)
            .identity_config(config.identity.clone())
            .skills(loaded_skills)
            .skills_prompt_mode(config.skills.prompt_injection_mode)
            .auto_save(config.memory.auto_save)
            .security_summary(Some(security.prompt_summary()))
            .autonomy_level(config.autonomy.level)
            .activated_tools(activated_tools)
            .surface(crate::tools::ToolSurfaceBaseline::Both)
            .user_profile_config(config.user_profile.clone())
            .skill_evolution_config(config.skill_evolution.clone())
            .prompt_optimizer_config(config.prompt_optimizer.clone())
            .experience_replay(experience_replay)
            .plan_mode_config(config.plan_mode.clone())
            .intent_analysis_config(config.intent_analysis.clone())
            .shared_config(shared_config.unwrap_or_else(crate::config::live::LiveConfig::default))
            .cached_provider_config(
                provider_name_raw.to_string(),
                config.api_key.clone().unwrap_or_default(),
                config.api_url.clone().unwrap_or_default(),
            )
            .desktop_security_policy(Some(Arc::clone(&security)))
            .plan_mode_flag(plan_mode_flag)
            .build()
    }

    fn trim_history(&mut self) {
        let max_messages = self.config.max_history_messages;

        const MAX_CHARS: usize = 400_000;

        let lead_system_end = self
            .history
            .iter()
            .position(
                |m| !matches!(m, ConversationMessage::Chat(chat) if chat.role == "system"),
            )
            .unwrap_or(self.history.len());

        let mut lead: Vec<ConversationMessage> =
            self.history.drain(0..lead_system_end).collect();
        let mut body: Vec<ConversationMessage> = self.history.drain(..).collect();

        if body.len() > max_messages {
            let drop_count = body.len() - max_messages;
            body.drain(0..drop_count);
        }

        let lead_chars: usize = lead.iter().map(|m| Self::msg_char_len(m)).sum();
        let mut acc = lead_chars;
        let mut keep_from = body.len();
        for i in (0..body.len()).rev() {
            let prospective = acc + Self::msg_char_len(&body[i]);
            if prospective > MAX_CHARS && keep_from < body.len() {
                break;
            }
            acc = prospective;
            keep_from = i;
        }
        if keep_from > 0 {
            body.drain(0..keep_from);
        }

        lead.append(&mut body);
        self.history = lead;
        Self::repair_orphan_tool_result_messages(&mut self.history);
    }

    fn repair_orphan_tool_result_messages(history: &mut Vec<ConversationMessage>) {
        Self::upgrade_native_json_assistants_in_place(history);
        Self::collapse_empty_assistant_tool_calls(history);

        let mut out = Vec::with_capacity(history.len());
        let mut recovered_tool_result_batches = 0usize;
        let mut recovered_chat_tool_rows = 0usize;
        for msg in history.drain(..) {
            match &msg {
                ConversationMessage::ToolResults(rows) => {
                    let preceded = out
                        .last()
                        .is_some_and(|p| matches!(p, ConversationMessage::AssistantToolCalls { .. }));
                    if preceded {
                        out.push(msg);
                    } else {
                        recovered_tool_result_batches += 1;
                        tracing::debug!(
                            target: "agent.history_repair",
                            batch_len = rows.len(),
                            "recovered orphaned ToolResults as synthetic user transcript (missing assistant preamble)"
                        );
                        out.push(Self::recover_tool_results_batch_as_user(rows));
                    }
                }
                ConversationMessage::Chat(c) if c.role == "tool" => {
                    let preceded = out.last().is_some_and(|p| match p {
                        ConversationMessage::AssistantToolCalls { .. } => true,
                        ConversationMessage::Chat(pc) => pc.role == "tool",
                        _ => false,
                    });
                    if preceded {
                        out.push(msg);
                    } else {
                        recovered_chat_tool_rows += 1;
                        tracing::debug!(
                            target: "agent.history_repair",
                            "recovered orphaned Chat(role=tool) as synthetic user transcript (missing assistant preamble)"
                        );
                        out.push(Self::recover_chat_tool_as_user(c));
                    }
                }
                _ => out.push(msg),
            }
        }
        if recovered_tool_result_batches > 0 || recovered_chat_tool_rows > 0 {
            tracing::info!(
                target: "agent.history_repair",
                tool_result_batches = recovered_tool_result_batches,
                chat_tool_rows = recovered_chat_tool_rows,
                "recovered orphaned tool transcript rows as synthetic user transcript"
            );
        }
        *history = out;
        crate::agent::dangling_tool_repair::ensure_assistant_tool_replies_inplace(history);
    }

    fn collapse_empty_assistant_tool_calls(history: &mut [ConversationMessage]) {
        for m in history.iter_mut() {
            let collapsed = match &*m {
                ConversationMessage::AssistantToolCalls {
                    text,
                    tool_calls,
                    reasoning_content,
                } if tool_calls.is_empty() => Some(Self::collapsed_native_style_assistant_blob(
                    text.as_deref(),
                    reasoning_content.as_deref(),
                )),
                _ => None,
            };
            if let Some(content) = collapsed {
                *m = ConversationMessage::Chat(ChatMessage::assistant(content));
            }
        }
    }

    fn collapsed_native_style_assistant_blob(
        text: Option<&str>,
        reasoning_content: Option<&str>,
    ) -> String {
        let mut map = serde_json::Map::new();
        if let Some(t) = text.filter(|s| !s.is_empty()) {
            map.insert(
                "content".to_string(),
                serde_json::Value::String(t.to_string()),
            );
        }
        if let Some(r) = reasoning_content.filter(|s| !s.is_empty()) {
            map.insert(
                "reasoning_content".to_string(),
                serde_json::Value::String(r.to_string()),
            );
        }
        if map.is_empty() {
            return String::new();
        }
        serde_json::Value::Object(map).to_string()
    }

    fn recover_tool_results_batch_as_user(
        rows: &[crate::providers::ToolResultMessage],
    ) -> ConversationMessage {
        let mut buf = String::from(
            "[Recovered tool batch; assistant tool_calls preamble was missing in transcript]\n\n",
        );
        use std::fmt::Write as _;
        for tr in rows {
            let _ = writeln!(
                &mut buf,
                "### {}\n{}",
                tr.tool_call_id,
                tr.content.trim_end()
            );
            let _ = writeln!(&mut buf);
        }
        ConversationMessage::Chat(ChatMessage::user(buf))
    }

    fn recover_chat_tool_as_user(tool: &ChatMessage) -> ConversationMessage {
        let fallback_body = tool.content.clone();
        let parsed = serde_json::from_str::<serde_json::Value>(&tool.content).ok();
        let id = parsed
            .as_ref()
            .and_then(|v| v.get("tool_call_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let body = parsed
            .as_ref()
            .and_then(|v| v.get("content"))
            .map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or(fallback_body);
        let msg = format!(
            "[Recovered tool output; assistant tool_calls preamble was missing in transcript]\n\
             tool_call_id={id}\n\
             {body}",
        );
        ConversationMessage::Chat(ChatMessage::user(msg))
    }

    fn upgrade_native_json_assistants_in_place(history: &mut [ConversationMessage]) {
        for m in history.iter_mut() {
            if let ConversationMessage::Chat(c) = m {
                if c.role != "assistant" {
                    continue;
                }
                if let Some(up) = Self::try_chat_as_native_assistant_tool_calls(c) {
                    *m = up;
                }
            }
        }
    }

    fn try_chat_as_native_assistant_tool_calls(chat: &ChatMessage) -> Option<ConversationMessage> {
        let v: serde_json::Value = serde_json::from_str(chat.content.trim()).ok()?;
        if !v.is_object() {
            return None;
        }
        let tc_val = v.get("tool_calls")?;
        let tool_calls: Vec<ToolCall> = serde_json::from_value(tc_val.clone()).ok()?;
        if tool_calls.is_empty() {
            return None;
        }
        let text = match v.get("content") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.clone())
                }
            }
            Some(_) => return None,
        };
        let reasoning_content = v
            .get("reasoning_content")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Some(ConversationMessage::AssistantToolCalls {
            text,
            tool_calls,
            reasoning_content,
        })
    }

    fn msg_char_len(msg: &ConversationMessage) -> usize {
        match msg {
            ConversationMessage::Chat(chat) => chat.content.len(),
            ConversationMessage::ToolResults(rows) => rows
                .iter()
                .map(|r| r.content.len() + r.tool_call_id.len())
                .sum::<usize>()
                .max(1),
            ConversationMessage::AssistantToolCalls {
                text,
                tool_calls,
                reasoning_content,
            } => {
                text.as_ref().map(String::len).unwrap_or(0)
                    + reasoning_content.as_ref().map(String::len).unwrap_or(0)
                    + tool_calls
                        .iter()
                        .map(|tc| tc.name.len() + tc.arguments.len() + tc.id.len())
                        .sum::<usize>()
            }
        }
    }

    fn build_system_prompt(&self) -> Result<String> {
        let live_cfg = self.shared_config.load();
        let coding_mode_label = self.current_coding_mode.map(|m| m.label());
        let allowed_tool_names = self.current_coding_mode.and_then(|m| m.allowed_tools());

        let instructions = self.tool_dispatcher.prompt_instructions(&self.tools);
        let ctx = PromptContext {
            workspace_dir: &self.workspace_dir,
            model_name: &self.model_name,
            tools: &self.tools,
            allowed_tool_names: allowed_tool_names.clone(),
            skills: &self.skills,
            skills_prompt_mode: self.skills_prompt_mode,
            identity_config: Some(&self.identity_config),
            dispatcher_instructions: &instructions,
            tool_descriptions: self.tool_descriptions.as_ref(),
            security_summary: self.security_summary.clone(),
            autonomy_level: self.autonomy_level,
            global_directives: live_cfg.agent.global_directives.as_slice(),
            coding_mode_label,
        };
        let mut prompt = self.prompt_builder.build(&ctx)?;

        let user_profile = crate::agent::user::profile::UserProfile::new(
            &self.workspace_dir,
            self.user_profile_config.clone(),
        );
        if let Some(profile_text) = user_profile.prompt_injection() {
            prompt.push_str(&profile_text);
        }

        let skill_engine =
            crate::agent::skill_evolution::ensure_global_engine(&self.skill_evolution_config);
        if let Some(skill_text) = skill_engine.prompt_injection() {
            prompt.push_str(&skill_text);
        }

        let prompt_optimizer =
            crate::agent::prompt::optimizer::ensure_global_optimizer(&self.prompt_optimizer_config);
        if let Some(po_text) = prompt_optimizer.prompt_injection() {
            prompt.push_str(&po_text);
        }

        if let Some(ref replay) = self.experience_replay {
            if let Some(exp_text) = replay.prompt_injection(None) {
                prompt.push_str(&exp_text);
            }
        }

        if self.plan_mode_config.enabled {
            prompt.push_str(
                "\n\n## Plan Mode\n\n\
                 You are operating in plan mode. For complex multi-step tasks, \
                 create a structured plan before executing. Break tasks into \
                 ordered steps, track progress, and update status as you work.\n\
                 - Mark items [x] completed, [!] failed, or [-] skipped.\n\
                 - Auto-activate planning for queries requiring more than \
                 ",
            );
            prompt.push_str(&self.plan_mode_config.auto_activate_threshold.to_string());
            prompt.push_str(" tool calls.\n");
        }

        if let Some(ref mode) = self.current_coding_mode {
            prompt.push_str(&mode.system_prompt_injection());
        }

        if live_cfg.buddy.enabled {
            prompt.push_str("\n\n");
            prompt.push_str(&crate::buddy::prompt::buddy_system_prompt(
                &live_cfg.buddy.name,
                &live_cfg.buddy.personality,
            ));
        }

        if let Some(theme) = crate::util::get_runtime_var("SEN_THEME") {
            let theme = theme.trim();
            if !theme.is_empty()
                && theme != crate::constants::output_styles::STYLE_DEFAULT
            {
                if let Some(style) = crate::constants::output_styles::builtin_output_styles()
                    .into_iter()
                    .find(|s| s.name == theme)
                {
                    if !style.system_prompt_addition.is_empty() {
                        prompt.push_str("\n\n");
                        prompt.push_str(&style.system_prompt_addition);
                    }
                }
            }
        }

        let max_chars = live_cfg.agent.max_system_prompt_chars;
        if max_chars > 0 && prompt.len() > max_chars {
            let mut take = max_chars.saturating_sub(160);
            while take > 0 && !prompt.is_char_boundary(take) {
                take -= 1;
            }
            if take > 0 {
                let truncated = &prompt[..take];
                prompt = format!(
                    "{truncated}\n\n[system prompt truncated to {max_chars} chars to preserve token budget]\n"
                );
            }
        }

        Ok(prompt)
    }

    async fn execute_tool_call(&self, call: &ParsedToolCall) -> ToolExecutionResult {
        let start = Instant::now();

        if self.cancel_signal.load_full().is_cancelled()
            || self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
        {
            return ToolExecutionResult {
                name: call.name.clone(),
                output: "[Cancelled by user]".to_string(),
                success: false,
                tool_call_id: call.tool_call_id.clone(),
            };
        }

        if let (Some(engine), Some(identity)) = (&self.rbac_engine, &self.rbac_identity) {
            let auth = engine.authorize_tool(identity, &call.name);
            if !auth.allowed {
                let reason = auth.reason.unwrap_or_else(|| "access denied".to_string());
                return ToolExecutionResult {
                    name: call.name.clone(),
                    output: format!("RBAC denied: {reason}"),
                    success: false,
                    tool_call_id: call.tool_call_id.clone(),
                };
            }
        } else if self.rbac_engine.is_some() != self.rbac_identity.is_some() {
            tracing::error!(
                tool = call.name,
                "RBAC partially configured (engine={}, identity={}); denying tool execution (fail-closed)",
                self.rbac_engine.is_some(),
                self.rbac_identity.is_some(),
            );
            return ToolExecutionResult {
                name: call.name.clone(),
                output: format!(
                    "RBAC denied: access control is partially configured (engine={}, identity={}); refusing to run tool '{}' without a complete RBAC setup",
                    self.rbac_engine.is_some(),
                    self.rbac_identity.is_some(),
                    call.name
                ),
                success: false,
                tool_call_id: call.tool_call_id.clone(),
            };
        }

        let effective_coding_mode = crate::agent::coding_mode::scoped_coding_mode()
            .or_else(|| {
                let svc = crate::services::try_get_services()?;
                let session = crate::session::current_session_context()?;
                svc.session_coding_mode(&format!("gw_{}", session.session_id))
                    .or_else(|| svc.session_coding_mode(&session.session_id))
            })
            .or(self.current_coding_mode)
            .or_else(|| Some(crate::agent::coding_mode::active_coding_mode()));

        if let Some(mode) = effective_coding_mode {
            if let Some(allowed) = mode.allowed_tools() {
                if !allowed.contains(call.name.as_str()) {
                    let mut listed: Vec<&str> = allowed.iter().copied().collect();
                    listed.sort_unstable();
                    let preview: String = listed
                        .iter()
                        .take(12)
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let extra = if listed.len() > 12 {
                        format!(", ... ({} more)", listed.len() - 12)
                    } else {
                        String::new()
                    };
                    let label = mode.label();
                    let hint = if matches!(mode, crate::agent::coding_mode::CodingMode::Plan) {
                        " To produce or update the plan document call \
                         `update_plan(action=\"set\"|\"add\"|\"save\", ...)`. \
                         When planning is finished call `exit_plan_mode` so \
                         the user can press the golden Build button (or \
                         reply '同意'/'Build'/'execute') to switch to Agent \
                         mode for execution."
                    } else {
                        ""
                    };
                    let denial_message = format!(
                        "Tool '{}' is not permitted in {} mode.{} \
                         Allowed tools: {}{}",
                        call.name, label, hint, preview, extra
                    );
                    crate::agent::mode::effects::record_mode_intercept(
                        crate::agent::mode::effects::ModeInterceptReason::ToolNotAllowed,
                        &crate::agent::mode::effects::ModeInterceptContext {
                            mode,
                            channel: Some("desktop"),
                            provider: Some(self.cached_provider.as_str()),
                            model: None,
                            turn_id: None,
                            tool: Some(call.name.as_str()),
                            tool_call_id: call.tool_call_id.as_deref(),
                            iteration: None,
                            message: Some(&denial_message),
                        },
                    );
                    return ToolExecutionResult {
                        name: call.name.clone(),
                        output: denial_message,
                        success: false,
                        tool_call_id: call.tool_call_id.clone(),
                    };
                }
            }
            if let Some(reason) =
                crate::agent::mode::effects::mode_blocks_tool(mode, call.name.as_str())
            {
                crate::agent::mode::effects::record_mode_intercept(
                    crate::agent::mode::effects::ModeInterceptReason::ReadOnlyPolicy,
                    &crate::agent::mode::effects::ModeInterceptContext {
                        mode,
                        channel: Some("desktop"),
                        provider: Some(self.cached_provider.as_str()),
                        model: None,
                        turn_id: None,
                        tool: Some(call.name.as_str()),
                        tool_call_id: call.tool_call_id.as_deref(),
                        iteration: None,
                        message: Some(&reason),
                    },
                );
                return ToolExecutionResult {
                    name: call.name.clone(),
                    output: reason,
                    success: false,
                    tool_call_id: call.tool_call_id.clone(),
                };
            }
        }

        let coding_label = effective_coding_mode.map(|m| m.label().to_string());
        let coding_label_lc = coding_label.as_deref().map(str::to_ascii_lowercase);
        let perm_mode_lc = crate::gateway::ws::desktop::active_permission_mode();
        let tool_lc = call.name.to_ascii_lowercase();
        let guardrail_ctx = crate::guardrails::GuardrailContext {
            coding_mode: coding_label_lc.as_deref(),
            permission_mode: Some(&perm_mode_lc),
            tool_name: Some(&tool_lc),
        };
        let mode_auto_approved = effective_coding_mode
            .map(crate::agent::mode::effects::mode_auto_approves)
            .unwrap_or(false)
            && crate::approval::session_surface_approval_manager()
                .is_none_or(|m| m.mode_auto_approve_allows(&call.name));
        match crate::guardrails::evaluate_tool_guardrails(&call.name, Some(&guardrail_ctx)) {
            crate::guardrails::GuardrailDecision::Allow => {}
            crate::guardrails::GuardrailDecision::Deny(reason) => {
                return ToolExecutionResult {
                    name: call.name.clone(),
                    output: format!("Blocked by guardrails: {reason}"),
                    success: false,
                    tool_call_id: call.tool_call_id.clone(),
                };
            }
            crate::guardrails::GuardrailDecision::RequireApproval(reason) => {
                let outcome = if mode_auto_approved {
                    GuardrailApprovalOutcome::Approved
                } else {
                    self.request_guardrail_approval(&call.name, &call.arguments, &reason)
                        .await
                };
                match outcome {
                    GuardrailApprovalOutcome::Approved => {}
                    GuardrailApprovalOutcome::Denied => {
                        return ToolExecutionResult {
                            name: call.name.clone(),
                            output: format!(
                                "Blocked by guardrails: approval required but not granted ({reason})"
                            ),
                            success: false,
                            tool_call_id: call.tool_call_id.clone(),
                        };
                    }
                    GuardrailApprovalOutcome::Cancelled => {
                        return ToolExecutionResult {
                            name: call.name.clone(),
                            output: "[Cancelled by user]".to_string(),
                            success: false,
                            tool_call_id: call.tool_call_id.clone(),
                        };
                    }
                }
            }
        }

        let mut hook_call_name = call.name.clone();
        let mut hook_call_args = call.arguments.clone();
        if let Some(ref runner) = self.hook_runner {
            match runner
                .run_before_tool_call(hook_call_name.clone(), hook_call_args.clone())
                .await
            {
                crate::hooks::HookResult::Continue((n, a)) => {
                    hook_call_name = n;
                    hook_call_args = a;
                }
                crate::hooks::HookResult::RequireApproval((n, a), message) => {
                    hook_call_name = n;
                    hook_call_args = a;
                    let reason = message.unwrap_or_else(|| {
                        "manual approval required by hooks.json".to_string()
                    });
                    let outcome = self
                        .request_guardrail_approval(
                            &hook_call_name,
                            &hook_call_args,
                            &format!("hooks.json requires approval: {reason}"),
                        )
                        .await;
                    match outcome {
                        GuardrailApprovalOutcome::Approved => {}
                        GuardrailApprovalOutcome::Denied => {
                            let denied = format!(
                                "Denied by user (hooks.json requested approval: {reason})"
                            );
                            let result = crate::tools::ToolResult {
                                success: false,
                                output: denied.clone(),
                                error: Some(denied.clone()),
                            };
                            runner
                                .fire_after_tool_call(&call.name, &result, start.elapsed())
                                .await;
                            return ToolExecutionResult {
                                name: call.name.clone(),
                                output: denied,
                                success: false,
                                tool_call_id: call.tool_call_id.clone(),
                            };
                        }
                        GuardrailApprovalOutcome::Cancelled => {
                            return ToolExecutionResult {
                                name: call.name.clone(),
                                output: "[Cancelled by user]".to_string(),
                                success: false,
                                tool_call_id: call.tool_call_id.clone(),
                            };
                        }
                    }
                }
                crate::hooks::HookResult::Cancel(reason) => {
                    let result = crate::tools::ToolResult {
                        success: false,
                        output: format!("Cancelled by hook: {reason}"),
                        error: Some(reason.clone()),
                    };
                    runner
                        .fire_after_tool_call(&call.name, &result, start.elapsed())
                        .await;
                    return ToolExecutionResult {
                        name: call.name.clone(),
                        output: format!("Cancelled by hook: {reason}"),
                        success: false,
                        tool_call_id: call.tool_call_id.clone(),
                    };
                }
            }
        }
        let effective_call = if hook_call_name == call.name && hook_call_args == call.arguments {
            None
        } else {
            Some(ParsedToolCall {
                name: hook_call_name,
                arguments: hook_call_args,
                tool_call_id: call.tool_call_id.clone(),
                parse_error: call.parse_error,
            })
        };
        let dispatch_call: &ParsedToolCall = effective_call.as_ref().unwrap_or(call);

        if effective_call.is_some() {
            let dispatch_tool_lc = dispatch_call.name.to_ascii_lowercase();
            let renamed_ctx = crate::guardrails::GuardrailContext {
                coding_mode: coding_label_lc.as_deref(),
                permission_mode: Some(&perm_mode_lc),
                tool_name: Some(&dispatch_tool_lc),
            };
            match crate::guardrails::evaluate_tool_guardrails(
                &dispatch_call.name,
                Some(&renamed_ctx),
            ) {
                crate::guardrails::GuardrailDecision::Allow => {}
                crate::guardrails::GuardrailDecision::Deny(reason) => {
                    return ToolExecutionResult {
                        name: call.name.clone(),
                        output: format!(
                            "Blocked by guardrails after hook modification of '{}': {reason}",
                            dispatch_call.name
                        ),
                        success: false,
                        tool_call_id: call.tool_call_id.clone(),
                    };
                }
                crate::guardrails::GuardrailDecision::RequireApproval(reason) => {
                    let outcome = if mode_auto_approved {
                        GuardrailApprovalOutcome::Approved
                    } else {
                        self.request_guardrail_approval(
                            &dispatch_call.name,
                            &dispatch_call.arguments,
                            &reason,
                        )
                        .await
                    };
                    match outcome {
                        GuardrailApprovalOutcome::Approved => {}
                        GuardrailApprovalOutcome::Denied => {
                            return ToolExecutionResult {
                                name: call.name.clone(),
                                output: format!(
                                    "Blocked by guardrails after hook modification of '{}': approval required but not granted ({reason})",
                                    dispatch_call.name
                                ),
                                success: false,
                                tool_call_id: call.tool_call_id.clone(),
                            };
                        }
                        GuardrailApprovalOutcome::Cancelled => {
                            return ToolExecutionResult {
                                name: call.name.clone(),
                                output: "[Cancelled by user]".to_string(),
                                success: false,
                                tool_call_id: call.tool_call_id.clone(),
                            };
                        }
                    }
                }
            }
        }

        {
            let web_search_enabled = crate::services::try_get_services()
                .map(|svc| svc.config().web_search.enabled)
                .unwrap_or(true);
            match crate::agent::web_search_url_guard::evaluate_browser_or_web_fetch_call(
                dispatch_call.name.as_str(),
                &dispatch_call.arguments,
                web_search_enabled,
            ) {
                crate::agent::web_search_url_guard::GuardDecision::Allow => {}
                crate::agent::web_search_url_guard::GuardDecision::AllowWithFallbackTrace => {
                    tracing::info!(
                        tool = %dispatch_call.name,
                        "Permitting search-engine URL as fallback; web_search recently failed"
                    );
                }
                crate::agent::web_search_url_guard::GuardDecision::Refuse(refusal) => {
                    tracing::warn!(
                        tool = %dispatch_call.name,
                        "Blocked search-engine URL misuse; web_search has not been tried yet"
                    );
                    self.observer.record_event(&ObserverEvent::ToolCall {
                        tool: call.name.clone(),
                        duration: start.elapsed(),
                        success: false,
                    });
                    if let Some(ref runner) = self.hook_runner {
                        let hook_result = crate::tools::ToolResult {
                            success: false,
                            output: refusal.clone(),
                            error: Some(refusal.clone()),
                        };
                        runner
                            .fire_after_tool_call(&call.name, &hook_result, start.elapsed())
                            .await;
                    }
                    return ToolExecutionResult {
                        name: call.name.clone(),
                        output: refusal,
                        success: false,
                        tool_call_id: call.tool_call_id.clone(),
                    };
                }
            }
        }

        async fn run_tool(
            tool: &dyn Tool,
            call: &ParsedToolCall,
            observer: &Arc<dyn Observer>,
        ) -> (String, bool) {
            let start = Instant::now();
            match crate::agent::loop_::execute_tool_panic_safe(
                tool,
                &call.name,
                call.arguments.clone(),
            )
            .await
            {
                Ok(r) => {
                    observer.record_event(&ObserverEvent::ToolCall {
                        tool: call.name.clone(),
                        duration: start.elapsed(),
                        success: r.success,
                    });
                    if r.success {
                        let scrubbed = crate::agent::profile::pii_sanitize::scrub_tool_output(&call.name, &r.output);
                        let fallback = scrubbed.clone();
                        let call_name_owned = call.name.clone();
                        let out = tokio::task::spawn_blocking(move || {
                            crate::agent::token::optimizer::compress_output(
                                &call_name_owned,
                                &scrubbed,
                            )
                        })
                        .await
                        .unwrap_or_else(|join_err| {
                            tracing::warn!(
                                tool = %call.name,
                                error = %join_err,
                                "tool output compression task failed; falling back to uncompressed output"
                            );
                            fallback
                        });
                        (out, true)
                    } else {
                        let reason = r.error.unwrap_or(r.output);
                        (
                            format!("Error: {}", crate::agent::profile::pii_sanitize::scrub_credentials(&reason)),
                            false,
                        )
                    }
                }
                Err(e) => {
                    observer.record_event(&ObserverEvent::ToolCall {
                        tool: call.name.clone(),
                        duration: start.elapsed(),
                        success: false,
                    });
                    (format!("Error executing {}: {e}", call.name), false)
                }
            }
        }

        tracing::debug!(
            target: "agent.tool",
            tool = %dispatch_call.name,
            id = ?dispatch_call.tool_call_id,
            "tool execution start"
        );
        let cancel_handle = self.cancel_signal.load_full().as_ref().clone();
        let (output, success) = if let Some(tool) = self
            .tool_index
            .get(dispatch_call.name.as_str())
            .map(|&i| &self.tools[i])
        {
            tokio::select! {
                biased;
                _ = cancel_handle.cancelled() => {
                    ("[Cancelled by user]".to_string(), false)
                }
                res = run_tool(tool.as_ref(), dispatch_call, &self.observer) => res,
            }
        } else if let Some(activated_arc) = self.activated_tools.as_ref() {
            let activated_opt = activated_arc.lock().get_resolved(&dispatch_call.name);
            if let Some(tool) = activated_opt {
                tokio::select! {
                    biased;
                    _ = cancel_handle.cancelled() => {
                        ("[Cancelled by user]".to_string(), false)
                    }
                    res = run_tool(tool.as_ref(), dispatch_call, &self.observer) => res,
                }
            } else {
                (format!("Unknown tool: {}", dispatch_call.name), false)
            }
        } else {
            (format!("Unknown tool: {}", dispatch_call.name), false)
        };

        let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::debug!(
            target: "agent.tool",
            tool = %dispatch_call.name,
            id = ?dispatch_call.tool_call_id,
            success,
            wall_ms,
            "tool execution end"
        );
        crate::agent::profile::runtime_hooks::publish_tool_event(&dispatch_call.name, success, wall_ms);

        if crate::agent::web_search_url_guard::is_web_search_tool_name(&dispatch_call.name) {
            if success {
                crate::agent::web_search_url_guard::record_web_search_success();
            } else {
                crate::agent::web_search_url_guard::record_web_search_failure();
            }
        }

        if let Some(ref runner) = self.hook_runner {
            let hook_result = crate::tools::ToolResult {
                success,
                output: output.clone(),
                error: if success { None } else { Some(output.clone()) },
            };
            runner
                .fire_after_tool_call(&call.name, &hook_result, start.elapsed())
                .await;
        }

        ToolExecutionResult {
            name: call.name.clone(),
            output,
            success,
            tool_call_id: call.tool_call_id.clone(),
        }
    }

    fn build_turn_companion(user_message: &str, expanded_user: &str, context: &str) -> String {
        let now = chrono::Local::now();
        let (year, month, day) = (now.year(), now.month(), now.day());
        let (hour, minute, second) = (now.hour(), now.minute(), now.second());
        let tz = now.format("%Z");
        let date_str =
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {tz}");

        let mut companion = format!(
            "[CURRENT REQUEST CONTEXT - this note belongs to the user message directly above it. \
             If (and only if) that message is the LAST user message in this conversation, it is \
             the one request you must act on right now: take it literally and at face value; \
             continue or resume earlier / unfinished work ONLY when it explicitly says so (e.g. \
             \"继续\", \"continue\", \"接着\", \"go on\") or directly references that earlier \
             task; if it is a greeting (e.g. \"你好\", \"hi\", \"在吗\"), small talk, a short \
             acknowledgement, or any new or unrelated request, respond to IT directly and do NOT \
             resume, re-run, or push forward any earlier task on your own - even if your own \
             previous message offered to continue, and even if earlier work was left unfinished. \
             Never treat a short or ambiguous message as implicit consent to keep going. For any \
             EARLIER user message, this note is historical context only.]\
             \n\n[MESSAGE DATE & TIME: {date_str}]"
        );

        if !context.is_empty() {
            companion.push_str("\n\n");
            companion.push_str(context);
        }

        if expanded_user != user_message {
            let appended_only = expanded_user
                .strip_prefix(user_message)
                .map(str::trim)
                .filter(|extra| !extra.is_empty());
            if let Some(extra) = appended_only {
                companion.push_str(
                    "\n\n[ATTACHED CONTEXT - resolved from the references in the user message \
                     above]\n",
                );
                companion.push_str(extra);
            } else if let Some(idx) = expanded_user.find("<context ") {
                let attachments = expanded_user[idx..].trim();
                if !attachments.is_empty() {
                    companion.push_str(
                        "\n\n[ATTACHED CONTEXT - resolved from the @references in the user \
                         message above]\n",
                    );
                    companion.push_str(attachments);
                }
            } else {
                companion.push_str(
                    "\n\n[EXPANDED REQUEST - the user message above with its references \
                     resolved]\n",
                );
                companion.push_str(expanded_user);
            }
        }

        companion
    }

    fn resolve_window_model(&self, model: &str) -> String {
        if let Some(hint) = model.strip_prefix("route:") {
            if let Some(real) = self.route_model_by_hint.get(hint) {
                return real.clone();
            }
            return self.model_name.clone();
        }
        model.to_string()
    }

    fn classify_model(&self, user_message: &str) -> String {
        if let Some(decision) =
            super::classifier::classify_with_decision(&self.classification_config, user_message)
        {
            if self.available_hints.contains(&decision.hint) {
                let resolved_model = self
                    .route_model_by_hint
                    .get(&decision.hint)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                tracing::info!(
                    target: "query_classification",
                    hint = decision.hint.as_str(),
                    model = resolved_model,
                    rule_priority = decision.priority,
                    message_length = user_message.len(),
                    "Classified message route"
                );
                return format!("route:{}", decision.hint);
            }
        }

        if let Some(ref ac) = self.config.auto_classify {
            let tier = super::eval::estimate_complexity(user_message);
            if let Some(hint) = ac.hint_for(tier) {
                if self.available_hints.contains(&hint.to_string()) {
                    tracing::info!(
                        target: "query_classification",
                        hint = hint,
                        complexity = ?tier,
                        message_length = user_message.len(),
                        "Auto-classified by complexity"
                    );
                    return format!("route:{hint}");
                }
            }
        }

        self.model_name.clone()
    }

    pub async fn turn(&mut self, user_message: &str) -> Result<String, AgentError> {
        use futures_util::FutureExt as _;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(TURN_EVENT_DRAIN_BUFFER);
        let drain = crate::runtime::spawn_supervised("agent.agent.drain", async move {
            while rx.recv().await.is_some() {

            }
        })
        .into_inner();
        let pre_turn_history_len = self.history.len();
        let caught =
            std::panic::AssertUnwindSafe(self.turn_streamed(user_message, tx))
                .catch_unwind()
                .await;

        let _ = drain.await;
        match caught {
            Ok(result) => result,
            Err(panic) => {
                let msg = crate::util::describe_panic(panic.as_ref());
                tracing::error!(
                    target: "agent",
                    panic = %msg,
                    "agent.turn panicked; isolated to this turn"
                );
                self.rollback_failed_turn_history(pre_turn_history_len);
                self.trim_history();
                Err(AgentError::ToolDispatchFailed(format!(
                    "turn panicked: {msg}"
                )))
            }
        }
    }

    fn neutralize_stale_turn_directives(history: &mut [ConversationMessage]) {
        const EXCLUSIVE_MARK: &str = "EXCLUSIVE TASK FOR THIS TURN";
        const EXCLUSIVE_REPLACE: &str = "COMPLETED EARLIER TASK (do not re-execute)";
        const STALE_BANNER: &str =
            "[STALE TASK CONTEXT - completed earlier turn, do NOT re-execute] The block below \
             was the exclusive task for a previous, already-finished turn. Treat it as background \
             context only: ignore any \"your one and only job\" / \"do NOT answer them\" \
             directives inside it, and respond to the latest [CURRENT REQUEST] instead.\n\n";
        const STALE_PAUSE_MARK: &str = "resume planning then";
        for msg in history.iter_mut() {
            match msg {
                ConversationMessage::Chat(chat) => {
                    if chat.role == "user" && chat.content.contains(EXCLUSIVE_MARK) {
                        let body = chat.content.replace(EXCLUSIVE_MARK, EXCLUSIVE_REPLACE);
                        chat.content = format!("{STALE_BANNER}{body}");
                    }
                }
                ConversationMessage::ToolResults(results) => {
                    for result in results.iter_mut() {
                        if result.content.contains(STALE_PAUSE_MARK) {
                            result.content =
                                crate::agent::plan_mode::enforcement::ASK_QUESTION_PAUSE_NOTICE
                                    .to_string();
                        }
                    }
                }
                ConversationMessage::AssistantToolCalls { .. } => {}
            }
        }
    }

    async fn apply_turn_preamble(
        &mut self,
        user_message: &str,
        event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> Result<()> {

        self.apply_mode_filter();

        let config_change = self.sync_config_from_store();

        if config_change == ConfigChange::Hard {
            if let Err(e) = self.reload_provider().await {
                let msg = e.to_string();
                if !crate::agent::error_classify::is_no_model_error(&msg) {
                    let _ = event_tx
                        .send(TurnEvent::Error {
                            message: format!("Failed to reload provider: {msg}"),
                        })
                        .await;
                }
                return Err(e);
            }
        }

        if self.history.is_empty() {
            let system_prompt = self.build_system_prompt()?;
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(
                    system_prompt,
                )));
        } else {
            self.refresh_history_system_prompt();
        }

        for msg in self.history.iter_mut() {
            if let ConversationMessage::Chat(chat) = msg {
                chat.strip_ephemeral_context();
            }
        }

        let plan_armed = self.plan_execution_armed.lock().is_some();
        let is_design_trigger = user_message
            .trim_start()
            .starts_with(crate::agent::designer::pipeline::DESIGN_TASK_PREFIX);

        let resuming_from_ask = self
            .resuming_from_ask
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        if !plan_armed && !is_design_trigger && !resuming_from_ask {
            Self::neutralize_stale_turn_directives(&mut self.history);
        }

        let intent_pass_active =
            self.intent_analysis_config.enabled && !plan_armed && !is_design_trigger;
        let pending_decision = if intent_pass_active {
            self.take_pending_intent_decision(user_message)
        } else {
            None
        };
        let has_unfinished_for_intent = self.has_unfinished_task();

        let memory_fut = async {
            if plan_armed || is_design_trigger {
                return String::new();
            }
            let raw = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.memory_loader.load_context(
                    self.memory.as_ref(),
                    user_message,
                    self.memory_session_id.as_deref(),
                ),
            )
            .await
            {
                Ok(result) => result.unwrap_or_default(),
                Err(_) => {
                    tracing::warn!(
                        target: "agent.memory",
                        "memory recall timed out; continuing turn without recalled context"
                    );
                    String::new()
                }
            };
            Self::cap_memory_context(raw)
        };

        let intent_fut = async {
            if !intent_pass_active {
                return None;
            }
            if pending_decision.is_some() {
                return pending_decision.clone();
            }
            if !has_unfinished_for_intent {
                return None;
            }
            self.analyze_intent_llm(user_message).await
        };

        let expansion_fut = crate::agent::context::expansion::expand_input(
            user_message,
            &self.workspace_dir,
            crate::context::builder::FocusPathRegistry::current(),
            String::new(),
        );

        let (context, llm_decision, expanded_user) =
            tokio::join!(memory_fut, intent_fut, expansion_fut);

        if self.auto_save && !plan_armed && !is_design_trigger {
            let autosave_key =
                crate::agent::loop_::autosave_content_key("user_msg", user_message);
            let memory = Arc::clone(&self.memory);
            let user_message_owned = user_message.to_string();
            let memory_session_id = self.memory_session_id.clone();
            let _ = crate::runtime::spawn_supervised("agent.memory.autosave", async move {
                let _ = memory
                    .store(
                        &autosave_key,
                        &user_message_owned,
                        MemoryCategory::Conversation,
                        memory_session_id.as_deref(),
                    )
                    .await;
            });
        }

        crate::agent::dangling_tool_repair::drop_payloadless_assistant_messages(&mut self.history);
        let has_unfinished = self.has_unfinished_task();
        crate::agent::dangling_tool_repair::close_orphan_user_turns(
            &mut self.history,
            has_unfinished,
        );

        let was_interrupted = std::mem::take(&mut self.last_turn_interrupted);
        if was_interrupted && !self.has_unfinished_task() {
            crate::agent::dangling_tool_repair::note_interrupted_turn(&mut self.history);
        }

        let mut enriched = Self::build_turn_companion(user_message, &expanded_user, &context);

        if let Ok(guard) = self.unfinished_task.lock() {
            if let Some(task) = guard.as_ref() {
                tracing::debug!(
                    target: "agent.intent",
                    seq = task.seq,
                    request = %task.request,
                    "injecting most-recent unfinished-task note into the turn envelope"
                );
                enriched.push_str("\n\n");
                enriched.push_str(&Self::unfinished_task_note(task));
            }
        }

        self.last_turn_resumed = false;

        if !intent_pass_active && !plan_armed && !is_design_trigger {
            let resumed =
                matches!(
                    crate::agent::intent::classify_conversation_intent(user_message),
                    crate::agent::intent::ConversationIntent::Continue
                );
            self.last_turn_resumed = self.has_unfinished_task() && resumed;
        }

        if intent_pass_active {
            let has_unfinished = self.has_unfinished_task();
            let effective_decision = llm_decision.as_ref().map(|decision| {
                let mut effective = decision.clone();
                if matches!(
                    effective.decision,
                    crate::agent::intent::IntentDecision::Resume
                ) {
                    let candidate_seq = self
                        .unfinished_task
                        .lock()
                        .ok()
                        .and_then(|g| g.as_ref().map(|t| t.seq));
                    if let Some(candidate_seq) = candidate_seq {
                        let seq_ok = effective
                            .resume_task_seq
                            .is_none_or(|s| s == candidate_seq);
                        if !seq_ok
                            || effective.confidence < self.intent_analysis_config.min_confidence
                        {
                            tracing::debug!(
                                target: "agent.intent",
                                confidence = effective.confidence,
                                resume_task_seq = ?effective.resume_task_seq,
                                candidate_seq,
                                "downgrading low-confidence or seq-mismatched resume decision to clarify"
                            );
                            effective.decision = crate::agent::intent::IntentDecision::Clarify;
                        }
                    }
                }
                effective
            });
            let (note, resumed) = match &effective_decision {
                Some(decision) => {
                    tracing::debug!(
                        target: "agent.intent",
                        decision = decision.decision.as_str(),
                        resume_task_seq = ?decision.resume_task_seq,
                        "injecting llm conversation-signal for this turn"
                    );
                    (
                        crate::agent::intent::llm_conversation_signal_note(decision, has_unfinished),
                        matches!(
                            decision.decision,
                            crate::agent::intent::IntentDecision::Resume
                        ),
                    )
                }
                None => {
                    let conv_intent =
                        crate::agent::intent::classify_conversation_intent(user_message);
                    tracing::debug!(
                        target: "agent.intent",
                        conversation_intent = conv_intent.as_str(),
                        "injecting heuristic conversation-signal for this turn (llm unavailable)"
                    );
                    (
                        crate::agent::intent::conversation_signal_note(conv_intent, has_unfinished),
                        matches!(conv_intent, crate::agent::intent::ConversationIntent::Continue),
                    )
                }
            };
            self.last_turn_resumed = has_unfinished && resumed;
            if let Some(note) = note {
                enriched.push_str("\n\n");
                enriched.push_str(note);
            }
        }

        if intent_pass_active {
            let intent_note = match &llm_decision {
                Some(decision)
                    if decision.confidence >= self.intent_analysis_config.min_confidence =>
                {
                    decision.intent_note()
                }
                Some(_) => None,
                None => {
                    let analysis = crate::agent::intent::analyze_intent(user_message);
                    if analysis.is_confident(self.intent_analysis_config.min_confidence) {
                        analysis.intent_note()
                    } else {
                        None
                    }
                }
            };
            if self.intent_analysis_config.enrich_preamble {
                if let Some(note) = intent_note {
                    enriched.push_str("\n\n");
                    enriched.push_str(note);
                }
            }
            if self.intent_analysis_config.enforce_plan_threshold
                && self.plan_mode_config.enabled
                && matches!(
                    crate::agent::eval::estimate_complexity(user_message),
                    crate::agent::eval::ComplexityTier::Complex
                )
            {
                enriched.push_str(&format!(
                    "\n\nThis task likely requires more than {} steps. Create a structured \
                     plan with the todo tool before executing, and update progress as you go.",
                    self.plan_mode_config.auto_activate_threshold
                ));
            }
        }

        self.history.push(ConversationMessage::Chat(
            ChatMessage::user(user_message).with_turn_companion(enriched),
        ));

        self.history = crate::agent::dangling_tool_repair::repair_dangling_tool_calls(
            std::mem::take(&mut self.history),
        );
        Ok(())
    }

    async fn apply_gui_model_switch(&mut self, event_tx: &tokio::sync::mpsc::Sender<TurnEvent>) {
        let switch_state = crate::agent::loop_::get_model_switch_state();
        let switch_opt = switch_state.lock().clone();

        tracing::debug!(
            "Model switch state check: {:?}, current model={}, provider={}",
            switch_opt,
            self.model_name,
            self.cached_provider
        );

        let Some((provider, model)) = switch_opt else {
            return;
        };
        tracing::info!(
            "Model switch detected in turn_streamed: provider={}, model={}",
            provider,
            model
        );
        let old_model = self.model_name.clone();
        self.runtime_selection_override = Some((provider.clone(), model.clone()));
        self.model_name = model.clone();
        self.cached_provider = provider.clone();

        tracing::info!("Model switch requires provider reload, reloading...");
        if let Err(e) = self.reload_provider().await {
            tracing::error!("Failed to reload provider during model switch: {}", e);
            let _ = event_tx
                .send(TurnEvent::Error {
                    message: format!("Provider reload failed: {}", e),
                })
                .await;
        }

        if old_model != self.model_name && !old_model.is_empty() && !self.model_name.is_empty() {
            let old_marker = format!("| Model: {old_model}");
            let new_marker = format!("| Model: {}", self.model_name);
            for entry in self.history.iter_mut() {
                if let ConversationMessage::Chat(msg) = entry {
                    if msg.role == "system" && msg.content.contains(&old_marker) {
                        msg.content = msg.content.replacen(&old_marker, &new_marker, 1);
                        break;
                    }
                }
            }
        }

        crate::agent::loop_::clear_model_switch_request();
    }

    async fn request_guardrail_approval(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        reason: &str,
    ) -> GuardrailApprovalOutcome {
        use crate::session::SessionEventKind;

        let bus = crate::gateway::ws::gateway_approval_bus();
        if bus.receiver_count() == 0 {
            tracing::warn!(
                tool = tool_name,
                reason,
                "guardrail requires approval but no approval surface is connected; denying"
            );
            return GuardrailApprovalOutcome::Denied;
        }

        let request_id = format!("guardrail_{}", uuid::Uuid::new_v4().simple());
        let mut rx = bus.subscribe();
        let request_payload = serde_json::json!({
            "kind": "guardrail_approval",
            "reason": reason,
            "input": arguments,
        });
        crate::approval::register_pending_gateway_approval_with_replay(
            request_id.clone(),
            serde_json::json!({
                "type": "permission_request",
                "requestId": request_id,
                "toolName": tool_name,
                "input": request_payload,
                "description": reason,
            }),
        );
        let sink = crate::gateway::ws::gateway_approval_sink_handle();
        sink.emit_kind(SessionEventKind::ApprovalRequested {
            id: request_id.clone(),
            tool_name: tool_name.to_string(),
            arguments: request_payload,
            issued_at: chrono::Utc::now(),
        });

        let cancel_token = self.cancel_signal();
        let verdict =
            crate::approval::wait_for_session_decision(&request_id, &mut rx, Some(&cancel_token))
                .await;
        let _ = crate::approval::drop_pending_gateway_approval(&request_id);
        match verdict {
            crate::approval::SessionApprovalVerdict::Decision(response) => match response {
                crate::approval::ApprovalResponse::Yes
                | crate::approval::ApprovalResponse::Always => {
                    GuardrailApprovalOutcome::Approved
                }
                crate::approval::ApprovalResponse::No => GuardrailApprovalOutcome::Denied,
            },
            crate::approval::SessionApprovalVerdict::Cancelled => {
                tracing::warn!(
                    tool = tool_name,
                    request_id = %request_id,
                    "guardrail approval cancelled by user; cancelling turn"
                );
                GuardrailApprovalOutcome::Cancelled
            }
            crate::approval::SessionApprovalVerdict::TimedOut => {
                tracing::warn!(
                    tool = tool_name,
                    request_id = %request_id,
                    "guardrail approval timed out; denying"
                );
                GuardrailApprovalOutcome::Denied
            }
        }
    }

    fn record_failed_turn_reinforcement(&self, error_message: &str) {
        let Some(engine) = crate::agent::reward::reinforcement::global_reinforcement_engine()
        else {
            return;
        };
        let record = crate::agent::reward::reinforcement::TurnRecord {
            turn_index: engine.total_turns(),
            timestamp: chrono::Utc::now(),
            reward: -1.0,
            model_used: self.model_name.clone(),
            temperature_used: self.temperature,
            query_category: "turn_error".to_string(),
            tools_used: Vec::new(),
            response_length: error_message.len(),
        };
        let _ = engine.record_turn(record);
    }

    async fn apply_bootstrap_model_override(
        &mut self,
        event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) {
        let Some(bs) = crate::bootstrap::try_get_state() else {
            return;
        };
        let Some(requested) = bs.read(|s| s.main_loop_model_override.clone()) else {
            return;
        };
        let resolved = {
            let config = self.shared_config.load();
            crate::agent::loop_::resolve_model_override_target(&requested, &config)
        };
        let Some((provider_override, target_model)) = resolved else {
            let _ = event_tx
                .send(TurnEvent::StatusUpdate {
                    action: "model_override".to_string(),
                    detail: "No usable fast model configuration found: add a model_routes entry with hint=\"fast\", or set agent_runtime.fast_apply_model.".to_string(),
                })
                .await;
            bs.write(|s| s.main_loop_model_override = None);
            return;
        };
        let target_provider = provider_override.unwrap_or_else(|| self.cached_provider.clone());
        if target_model == self.model_name && target_provider == self.cached_provider {
            return;
        }
        {
            let switch_state = crate::agent::loop_::get_model_switch_state();
            *switch_state.lock() = Some((target_provider, target_model));
        }
        self.apply_gui_model_switch(event_tx).await;
    }

    pub async fn run_single(&mut self, message: &str) -> Result<String> {
        self.turn(message).await.map_err(|e| e.into())
    }

    pub async fn run_interactive(&mut self) -> Result<()> {
        println!("🦀 SenWeaverCoding Interactive Mode");
        println!("Type /quit to exit.\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::channels::CliChannel::new();

        let listen_handle =
            crate::runtime::spawn_supervised("agent.repl.cli_listener", async move {
                let _ = crate::channels::Channel::listen(&cli, tx).await;
            })
            .into_inner();

        while let Some(msg) = rx.recv().await {
            let response = match self.turn(&msg.content).await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("\nError: {e}\n");
                    continue;
                }
            };
            println!("\n{response}\n");
        }

        listen_handle.abort();
        Ok(())
    }
}

pub(crate) struct GuiHooksFromAgent {
    response_cache: Option<Arc<crate::memory::response_cache::ResponseCache>>,
    memory: Arc<dyn Memory>,
    memory_session_id: Option<String>,
    auto_save: bool,
    classification_config: crate::config::QueryClassificationConfig,
    default_model: String,
    temperature: f64,
    experience_replay: Option<crate::agent::reward::experience::ExperienceReplay>,
    observer: Arc<dyn Observer>,
    cached_provider: String,
}

impl GuiHooksFromAgent {
    pub fn from_agent(agent: &Agent) -> Self {
        Self {
            response_cache: agent.response_cache.clone(),
            memory: agent.memory.clone(),
            memory_session_id: agent.memory_session_id.clone(),
            auto_save: agent.auto_save,
            classification_config: agent.classification_config.clone(),
            default_model: agent.model_name.clone(),
            temperature: agent.temperature,
            experience_replay: agent.experience_replay.clone(),
            observer: agent.observer.clone(),
            cached_provider: agent.cached_provider.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::ResponseCacheHook for GuiHooksFromAgent {
    fn build_key(&self, messages: &[ChatMessage], model: &str) -> Option<String> {
        if self.temperature != 0.0 {
            return None;
        }
        self.response_cache.as_ref().map(|_| {
            let system = messages
                .iter()
                .find(|m| m.role == "system")
                .map(|m| m.content.as_str());
            let parts = messages
                .iter()
                .filter(|m| m.role != "system")
                .flat_map(|m| [m.role.as_str(), ":", m.content.as_str(), "\u{1f}"]);
            crate::memory::response_cache::ResponseCache::cache_key_parts(model, system, parts)
        })
    }

    async fn try_hit(&self, key: &str, _user_message: &str) -> Option<String> {
        let cache = self.response_cache.as_ref()?;
        let provider_label = self.cached_provider.clone();
        let model_label = self.default_model.clone();
        let cache = Arc::clone(cache);
        let key_owned = key.to_string();
        let lookup = tokio::task::spawn_blocking(move || cache.get(&key_owned)).await;
        match lookup {
            Ok(Ok(Some(cached))) => {
                self.observer.record_event(&ObserverEvent::CacheHit {
                    cache_type: "response".into(),
                    tokens_saved: 0,
                });
                self.observer.record_metric(
                    &crate::observability::traits::ObserverMetric::ResponseCacheOutcome {
                        provider: provider_label,
                        model: model_label,
                        hit: true,
                    },
                );
                Some(cached)
            }
            _ => {
                self.observer.record_event(&ObserverEvent::CacheMiss {
                    cache_type: "response".into(),
                });
                self.observer.record_metric(
                    &crate::observability::traits::ObserverMetric::ResponseCacheOutcome {
                        provider: provider_label,
                        model: model_label,
                        hit: false,
                    },
                );
                None
            }
        }
    }

    async fn write_back(&self, key: &str, model: &str, response: &str, output_tokens: u32) {
        if let Some(cache) = &self.response_cache {
            let cache = Arc::clone(cache);
            let key = key.to_string();
            let model = model.to_string();
            let response = response.to_string();
            let _ = tokio::task::spawn_blocking(move || {
                cache.put(&key, &model, &response, output_tokens)
            })
            .await;
        }
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::MemorySessionHook for GuiHooksFromAgent {
    async fn on_turn_start(&self, _user_message: &str) {}

    async fn on_turn_end(&self, assistant_message: &str, _tools_used: &[String]) {
        if !self.auto_save {
            return;
        }
        let key =
            crate::agent::loop_::autosave_content_key("assistant_msg", assistant_message);
        let _ = self
            .memory
            .store(
                &key,
                assistant_message,
                MemoryCategory::Conversation,
                self.memory_session_id.as_deref(),
            )
            .await;
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::TurnPreambleHook for GuiHooksFromAgent {
    async fn apply(
        &self,
        _user_message: &str,
        _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> Result<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::GuiModelSwitchHook for GuiHooksFromAgent {
    async fn poll(&self, _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>) -> Option<String> {
        None
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::IterationContextBudgetHook for GuiHooksFromAgent {
    async fn prepare(
        &self,
        _iteration: usize,
        _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) {
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::ExperienceRecorderHook for GuiHooksFromAgent {
    async fn record(&self, summary: &crate::agent::loop_::traits::TurnExperienceSummary) {
        let Some(ref replay) = self.experience_replay else {
            return;
        };
        if !replay.collection_enabled() {
            return;
        }
        let refs: Vec<(&str, bool)> = summary
            .tool_results
            .iter()
            .map(|(n, s)| (n.as_str(), *s))
            .collect();
        let dims = crate::agent::self_assess::eval::heuristic_eval(
            &summary.user_query,
            &summary.assistant_response,
            &refs,
        );
        let reward = (dims.aggregate() * 2.0) - 1.0;
        let query_category =
            crate::agent::classifier::classify(&self.classification_config, &summary.user_query)
                .unwrap_or_else(|| "general".to_string());
        let experience = crate::agent::reward::experience::Experience {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: self.memory_session_id.clone().unwrap_or_default(),
            timestamp: chrono::Utc::now(),
            user_query: summary.user_query.clone(),
            assistant_response: summary.assistant_response.clone(),
            tools_used: summary.tools_used.clone(),
            model: self.default_model.clone(),
            reward,
            query_category,
            replay_count: 0,
        };
        replay.store(experience);
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_::traits::PlanModeNudgeHook for GuiHooksFromAgent {
    async fn try_inject(
        &self,
        _iteration: usize,
        _history: &mut Vec<ChatMessage>,
        _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> bool {
        false
    }
}

pub async fn run(
    config: Config,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    let start = Instant::now();

    let mut effective_config = config;
    if let Some(p) = provider_override {
        effective_config.default_provider = Some(p);
    }
    if let Some(m) = model_override {
        effective_config.default_model = Some(m);
    }
    effective_config.default_temperature = temperature;

    let mut agent = Agent::from_config(&effective_config, None, None).await?;

    let provider_name = effective_config
        .default_provider
        .as_deref()
        .unwrap_or("openrouter")
        .to_string();
    let model_name = providers::resolve_default_model(&effective_config)?;

    agent.observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.clone(),
        model: model_name.clone(),
    });

    if let Some(msg) = message {
        let response = agent.run_single(&msg).await?;
        println!("{response}");
    } else {
        agent.run_interactive().await?;
    }

    agent.observer.record_event(&ObserverEvent::AgentEnd {
        provider: provider_name,
        model: model_name,
        duration: start.elapsed(),
        tokens_used: None,
        cost_usd: None,
    });

    Ok(())
}
