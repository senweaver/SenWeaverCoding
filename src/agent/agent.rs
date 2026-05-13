// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::agent::dispatcher::{
    NativeToolDispatcher, ParsedToolCall, ToolDispatcher, ToolExecutionResult, XmlToolDispatcher,
};
use crate::agent::loop_control::LoopControlState;
use crate::agent::memory_loader::{DefaultMemoryLoader, MemoryLoader};
use crate::agent::prompt::{PromptContext, SystemPromptBuilder};
use crate::config::Config;
use crate::error::AgentError;
use crate::i18n::ToolDescriptions;
use crate::memory::{self, Memory, MemoryCategory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::{self, ChatMessage, ChatRequest, ConversationMessage, Provider, ToolCall};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools::{self, Tool, ToolSpec};
use anyhow::Result;
use chrono::{Datelike, Timelike};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

pub(crate) const TURN_EVENT_DRAIN_BUFFER: usize = 1024;

#[derive(Debug, Clone)]
pub enum TurnEvent {

    Chunk { delta: String },

    Thinking { delta: String },

    ToolCall {
        name: String,
        args: serde_json::Value,
    },

    ToolResult { name: String, output: String, success: bool },

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
        report: crate::services::pii_sanitizer::SanitizationReport,
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

pub struct Agent {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,

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

    user_profile_config: crate::agent::user_profile::UserProfileConfig,
    skill_evolution_config: crate::agent::skill_evolution::SkillEvolutionConfig,
    prompt_optimizer_config: crate::agent::prompt_optimizer::PromptOptimizerConfig,

    rbac_engine: Option<std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<crate::security::rbac::CallerIdentity>,
    experience_replay: Option<crate::agent::experience::ExperienceReplay>,
    plan_mode_config: crate::agent::plan_mode::PlanModeConfig,

    mode_tool_filter: Option<std::collections::HashSet<String>>,

    mode_filter_dirty: bool,

    current_coding_mode: Option<crate::agent::coding_mode::CodingMode>,

    baseline_max_tool_iterations: usize,

    cancelled: Arc<std::sync::atomic::AtomicBool>,

    cancel_signal: Arc<arc_swap::ArcSwap<tokio_util::sync::CancellationToken>>,

    shared_config: crate::config::live::LiveConfig,

    cached_provider: String,

    cached_api_key: crate::security::secret_string::SecretString,
    cached_api_url: String,

    last_usage: Option<crate::providers::TokenUsage>,

    desktop_security_policy: Option<Arc<SecurityPolicy>>,

    plan_execution_armed: parking_lot::Mutex<Option<String>>,

    hook_runner: Option<std::sync::Arc<crate::hooks::HotHookRunner>>,

    cached_tools_signature: u64,

    merged_specs_cache: parking_lot::Mutex<Option<MergedSpecsCacheEntry>>,
}

struct MergedSpecsCacheEntry {
    activation_revision: u64,
    base_ptr: usize,
    merged: std::sync::Arc<Vec<ToolSpec>>,
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
    user_profile_config: Option<crate::agent::user_profile::UserProfileConfig>,
    skill_evolution_config: Option<crate::agent::skill_evolution::SkillEvolutionConfig>,
    prompt_optimizer_config: Option<crate::agent::prompt_optimizer::PromptOptimizerConfig>,
    rbac_engine: Option<std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<crate::security::rbac::CallerIdentity>,
    experience_replay: Option<crate::agent::experience::ExperienceReplay>,
    plan_mode_config: Option<crate::agent::plan_mode::PlanModeConfig>,
    shared_config: Option<crate::config::live::LiveConfig>,

    cached_provider: Option<String>,
    cached_api_key: Option<String>,
    cached_api_url: Option<String>,
    desktop_security_policy: Option<Arc<SecurityPolicy>>,
    hook_runner: Option<std::sync::Arc<crate::hooks::HotHookRunner>>,
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
            shared_config: None,
            cached_provider: None,
            cached_api_key: None,
            cached_api_url: None,
            desktop_security_policy: None,
            hook_runner: None,
        }
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
        cfg: crate::agent::user_profile::UserProfileConfig,
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
        cfg: crate::agent::prompt_optimizer::PromptOptimizerConfig,
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
                    "no_model_configured: AgentBuilder.model_name is required; 请先在提供商设置页添加至少一个模型 (please add at least one model in Provider settings)"
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
            cached_provider: self.cached_provider.unwrap_or_default(),
            cached_api_key: crate::security::secret_string::SecretString::new(
                self.cached_api_key.unwrap_or_default(),
            ),
            cached_api_url: self.cached_api_url.unwrap_or_default(),
            last_usage: None,
            desktop_security_policy: self.desktop_security_policy,
            plan_execution_armed: parking_lot::Mutex::new(None),
            hook_runner: self.hook_runner,
            cached_tools_signature: 0,
            merged_specs_cache: parking_lot::Mutex::new(None),
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
        replay: Option<crate::agent::experience::ExperienceReplay>,
    ) -> Self {
        self.experience_replay = replay;
        self
    }

    pub fn plan_mode_config(mut self, cfg: crate::agent::plan_mode::PlanModeConfig) -> Self {
        self.plan_mode_config = Some(cfg);
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

    fn merge_token_usage_into(
        acc: &mut Option<crate::providers::traits::TokenUsage>,
        delta: &crate::providers::traits::TokenUsage,
    ) {
        let entry = acc.get_or_insert_with(crate::providers::traits::TokenUsage::default);
        let merge_field = |dst: &mut Option<u64>, src: Option<u64>| {
            if let Some(v) = src {
                *dst = Some(dst.unwrap_or(0).saturating_add(v));
            }
        };
        merge_field(&mut entry.input_tokens, delta.input_tokens);
        merge_field(&mut entry.output_tokens, delta.output_tokens);
        merge_field(&mut entry.cached_input_tokens, delta.cached_input_tokens);
        merge_field(
            &mut entry.cache_creation_input_tokens,
            delta.cache_creation_input_tokens,
        );
    }

    fn current_tool_specs_with_activated(&self) -> std::sync::Arc<Vec<ToolSpec>> {
        let Some(ref activated_arc) = self.activated_tools else {
            return std::sync::Arc::clone(&self.tool_specs);
        };

        let (revision, extra_empty) = {
            let guard = activated_arc.lock();
            (guard.revision(), guard.is_empty())
        };
        if extra_empty {
            return std::sync::Arc::clone(&self.tool_specs);
        }

        let base_ptr = std::sync::Arc::as_ptr(&self.tool_specs) as usize;

        {
            let cache = self.merged_specs_cache.lock();
            if let Some(entry) = cache.as_ref() {
                if entry.activation_revision == revision && entry.base_ptr == base_ptr {
                    return std::sync::Arc::clone(&entry.merged);
                }
            }
        }

        let extra = activated_arc.lock().tool_specs();
        if extra.is_empty() {
            return std::sync::Arc::clone(&self.tool_specs);
        }
        let base = self.tool_specs.as_ref();
        let mut merged: Vec<ToolSpec> = Vec::with_capacity(base.len() + extra.len());
        let mut existing: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(base.len() + extra.len());
        for spec in base.iter() {
            if existing.insert(spec.name.clone()) {
                merged.push(spec.clone());
            }
        }
        for spec in extra {
            if existing.insert(spec.name.clone()) {
                merged.push(spec);
            }
        }
        let merged_arc = std::sync::Arc::new(merged);

        let mut cache = self.merged_specs_cache.lock();
        *cache = Some(MergedSpecsCacheEntry {
            activation_revision: revision,
            base_ptr,
            merged: std::sync::Arc::clone(&merged_arc),
        });

        merged_arc
    }

    pub async fn turn_streamed(
        &mut self,
        user_message: &str,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> Result<String, AgentError> {

        self.sync_tools_from_config_if_changed().await;

        let user_message_owned = user_message.to_string();
        let user_message_for_turn = if let Some(ref runner) = self.hook_runner {
            match runner.run_before_prompt_build(user_message_owned.clone()).await {
                crate::hooks::HookResult::Continue(rewritten) => rewritten,
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

        let mut _turn_metrics_n1v2 = crate::agent::executor_core::TurnMetricsGuard::start();

        let _ = event_tx
            .send(TurnEvent::ProgressTick {
                iteration: 0,
                max_iterations: self.config.max_tool_iterations,
                tokens_used: 0,
            })
            .await;

        self.apply_turn_preamble(user_message, &event_tx).await?;

        let mut tools_used_this_turn: Vec<String> = Vec::new();
        let mut tool_results_this_turn: Vec<(String, bool)> = Vec::new();

        self.apply_gui_model_switch(&event_tx).await;

        let effective_model = self.classify_model(user_message);

        let pacing_snapshot = self.shared_config.load().pacing.clone();
        let loop_detector_cfg = crate::agent::loop_detector::LoopDetectorConfig {
            enabled: pacing_snapshot.loop_detection_enabled,
            window_size: pacing_snapshot.loop_detection_window_size,
            max_repeats: pacing_snapshot.loop_detection_max_repeats,
        };
        let mut loop_state = LoopControlState::new(
            loop_detector_cfg,
            pacing_snapshot.loop_detection_identical_output_threshold,
        )
        .with_callback(Box::new(|msg: &str| {
            tracing::info!(
                target: "agent.loop_detection",
                notification = msg,
                "loop detector notification"
            );

            if let Some(svc) = crate::services::try_get_services() {
                svc.agent_metrics.inc(
                    "sen_loop_detection_notifications_total",
                    crate::observability::agent_metrics::LabelSet::new(vec![]),
                );
            }
        }));

        let cancel = self.cancel_signal.load_full().as_ref().clone();

        let mut _pacing_gov = crate::agent::executor_core::PacingGovernor::new(
            self.config.max_tool_iterations.max(1),
            None,
            None,
        );

        let mut plan_nudge_state =
            crate::agent::plan_mode_enforcement::PlanModeNudgeState::new();

        let mut plan_exec_state = match self.take_plan_execution_arm() {
            Some(path) => {
                crate::agent::plan_execution_enforcement::PlanExecutionNudgeState::armed(path)
            }
            None => crate::agent::plan_execution_enforcement::PlanExecutionNudgeState::new(),
        };

        let mut awaiting_user_input = false;

        let mut turn_aggregated_usage: Option<crate::providers::traits::TokenUsage> = None;

        for iteration in 0..self.config.max_tool_iterations {

            let _ = event_tx
                .send(TurnEvent::ProgressTick {
                    iteration: iteration + 1,
                    max_iterations: self.config.max_tool_iterations,
                    tokens_used: 0,
                })
                .await;

            if self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
                || cancel.is_cancelled()
            {

                let _ = event_tx
                    .send(TurnEvent::Cancelling {
                        reason: "user_requested".into(),
                    })
                    .await;
                return Ok(String::new());
            }

            if let Some(crate::cost::types::BudgetCheck::Exceeded {
                current_usd,
                limit_usd,
                period,
            }) = crate::agent::loop_::check_tool_loop_budget(None)
            {
                let reason = format!(
                    "cost {:.4} USD exceeded {:.4} USD limit ({:?})",
                    current_usd, limit_usd, period
                );
                let _ = event_tx
                    .send(TurnEvent::Cancelling {
                        reason: format!("budget_exceeded: {reason}"),
                    })
                    .await;
                return Ok(format!("[Budget exceeded: {reason}]"));
            }

            if let Err(exceeded) = _pacing_gov.tick() {
                let _ = event_tx
                    .send(TurnEvent::Cancelling {
                        reason: format!("pacing_exceeded: {exceeded}"),
                    })
                    .await;
                return Ok(format!("[Pacing exceeded: {exceeded}]"));
            }

            self.prepare_iteration_context_budget(iteration, &event_tx)
                .await;

            if plan_exec_state.inline_progress_reminder_due(iteration) {
                let msg =
                    crate::agent::plan_execution_enforcement::inline_progress_reminder_message(
                        &plan_exec_state,
                    );
                tracing::info!(
                    target: "agent.plan_execution",
                    iteration = iteration,
                    reminder_count = plan_exec_state.inline_reminder_count + 1,
                    done = plan_exec_state.terminal_count,
                    total = plan_exec_state.total_steps,
                    "injecting inline plan-progress reminder mid-turn"
                );
                self.history.push(ConversationMessage::Chat(
                    ChatMessage::system(msg),
                ));
                plan_exec_state.last_update_iter = Some(iteration);
                plan_exec_state.inline_reminder_count =
                    plan_exec_state.inline_reminder_count.saturating_add(1);
            }

            let messages = self.tool_dispatcher.to_provider_messages(&self.history);
            let merged_tool_specs = self.current_tool_specs_with_activated();

            let cache_key = self.build_response_cache_key(&messages, &effective_model);
            if let Some(cached) = self.try_response_cache_hit(&cache_key, user_message).await {
                _turn_metrics_n1v2.mark_ok();
                return Ok(cached);
            }

            use futures_util::StreamExt;

            let stream_opts = crate::providers::traits::StreamOptions::new(true);
            let mut stream = self.provider.stream_chat(
                crate::providers::ChatRequest {
                    messages: &messages,
                    tools: if self.tool_dispatcher.should_send_tool_specs() {
                        Some(merged_tool_specs.as_slice())
                    } else {
                        None
                    },
                },
                &effective_model,
                self.temperature,
                stream_opts,
            );

            let mut streamed_text = String::new();
            let mut streamed_reasoning = String::new();
            let mut streamed_tool_calls: Vec<crate::providers::traits::ToolCall> = Vec::new();

            let mut streamed_usage: Option<crate::providers::traits::TokenUsage> = None;
            let mut got_stream = false;

            let mut seen_streaming_tool_sigs: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            let mut cancelled_during_stream = false;

            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        cancelled_during_stream = true;
                        break;
                    }
                    next = stream.next() => {
                        let Some(item) = next else { break };
                        match item {
                            Ok(event) => match event {
                                crate::providers::traits::StreamEvent::TextDelta(chunk) => {
                                    if let Some(reasoning) = chunk.reasoning {
                                        if !reasoning.is_empty() {

                                            streamed_reasoning.push_str(&reasoning);
                                            let _ = event_tx
                                                .send(TurnEvent::Thinking { delta: reasoning })
                                                .await;
                                        }
                                    }
                                    if !chunk.delta.is_empty() {
                                        got_stream = true;
                                        streamed_text.push_str(&chunk.delta);
                                        let _ = event_tx
                                            .send(TurnEvent::Chunk { delta: chunk.delta })
                                            .await;
                                    }
                                }
                                crate::providers::traits::StreamEvent::ToolCall(tc) => {
                                    got_stream = true;
                                    let sig = format!("{}|{}", tc.name, tc.arguments);
                                    if seen_streaming_tool_sigs.insert(sig) {
                                        let _ = event_tx
                                            .send(TurnEvent::ToolCall {
                                                name: tc.name.clone(),
                                                args: serde_json::from_str(&tc.arguments)
                                                    .unwrap_or_default(),
                                            })
                                            .await;
                                        streamed_tool_calls.push(tc);
                                    } else {
                                        tracing::debug!(
                                            target: "agent.stream",
                                            tool = %tc.name,
                                            "suppressed duplicate streaming tool_call (model emitted identical signature in same turn)"
                                        );
                                    }
                                }
                                crate::providers::traits::StreamEvent::PreExecutedToolCall {
                                    name,
                                    args,
                                } => {
                                    let _ = event_tx
                                        .send(TurnEvent::ToolCall {
                                            name,
                                            args: serde_json::from_str(&args).unwrap_or_default(),
                                        })
                                        .await;
                                }
                                crate::providers::traits::StreamEvent::PreExecutedToolResult {
                                    name,
                                    output,
                                } => {
                                    let success = !crate::agent::tool_event_status::output_indicates_error(&output);
                                    let _ = event_tx
                                        .send(TurnEvent::ToolResult { name, output, success })
                                        .await;
                                }
                                crate::providers::traits::StreamEvent::Usage(usage) => {

                                    streamed_usage = Some(usage);
                                }
                                crate::providers::traits::StreamEvent::Final => break,
                            },
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "Stream error from provider; ending stream"
                                );
                                break;
                            }
                        }
                    }
                }
            }
            drop(stream);

            if cancelled_during_stream
                || cancel.is_cancelled()
                || self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
            {
                let trimmed = streamed_text.trim();
                if !trimmed.is_empty() {
                    self.history.push(ConversationMessage::Chat(
                        ChatMessage::assistant(streamed_text.clone()),
                    ));
                }
                let _ = event_tx
                    .send(TurnEvent::Cancelling {
                        reason: "user_requested".into(),
                    })
                    .await;
                return Ok(streamed_text);
            }

            let response = if got_stream {

                let reasoning_content = if !streamed_reasoning.is_empty() {
                    Some(streamed_reasoning)
                } else if !streamed_tool_calls.is_empty() {
                    Some(
                        "(chain-of-thought unavailable — model emitted tool calls without a CoT stream)"
                            .to_string(),
                    )
                } else {
                    None
                };
                crate::providers::ChatResponse {
                    text: Some(streamed_text),
                    tool_calls: streamed_tool_calls,

                    usage: streamed_usage.take(),
                    reasoning_content,
                }
            } else {
                let chat_future = self.provider.chat(
                    ChatRequest {
                        messages: &messages,
                        tools: if self.tool_dispatcher.should_send_tool_specs() {
                            Some(merged_tool_specs.as_slice())
                        } else {
                            None
                        },
                    },
                    &effective_model,
                    self.temperature,
                );
                let timeout_secs: u64 = pacing_snapshot
                    .step_timeout_secs
                    .filter(|s| *s > 0)
                    .unwrap_or(600);
                match tokio::time::timeout(
                    std::time::Duration::from_secs(timeout_secs),
                    chat_future,
                )
                .await
                {
                    Ok(Ok(resp)) => resp,
                    Ok(Err(err)) => return Err(err.into()),
                    Err(_) => {
                        return Err(AgentError::LoopOverflow(timeout_secs as usize));
                    }
                }
            };

            if let Some(usage) = response.usage.as_ref() {
                let provider_name = self.cached_provider.as_str();
                let _ = crate::agent::scope_record_tool_loop_cost_usage(
                    provider_name,
                    &effective_model,
                    usage,
                );
                Self::merge_token_usage_into(&mut turn_aggregated_usage, usage);
            }

            let (text, calls) = self.tool_dispatcher.parse_response(&response);
            if calls.is_empty() {
                let final_text = if text.is_empty() {
                    response.text.unwrap_or_default()
                } else {
                    text
                };

                let in_plan_mode =
                    crate::agent::plan_mode_enforcement::detect_plan_mode_active(None);

                if matches!(
                    crate::agent::plan_mode_enforcement::evaluate_plan_mode_exit(
                        in_plan_mode,
                        &plan_nudge_state,
                        awaiting_user_input,
                    ),
                    crate::agent::plan_mode_enforcement::PlanModeExitDecision::InjectNudge
                ) {
                    let nudge_n = plan_nudge_state.nudge_count + 1;
                    tracing::info!(
                        target: "agent.plan_mode",
                        nudge_count = nudge_n,
                        "Plan mode (turn_streamed): model exited without exit_plan_mode; injecting nudge"
                    );

                    let _ = event_tx
                        .send(TurnEvent::StatusUpdate {
                            action: "Plan reminder".to_string(),
                            detail: format!(
                                "nudge {nudge_n} — asking model to call exit_plan_mode"
                            ),
                        })
                        .await;

                    Self::push_terminal_assistant_message(
                        &mut self.history,
                        final_text.clone(),
                        response.reasoning_content.clone(),
                    );
                    let msg = crate::agent::plan_mode_enforcement::nudge_message(
                        &plan_nudge_state,
                    );
                    self.history.push(ConversationMessage::Chat(
                        ChatMessage::system(msg),
                    ));
                    plan_nudge_state.nudge_count += 1;
                    continue;
                }

                if matches!(
                    crate::agent::plan_execution_enforcement::evaluate_plan_execution_exit(
                        &plan_exec_state,
                        awaiting_user_input,
                    ),
                    crate::agent::plan_execution_enforcement::PlanExecutionExitDecision::InjectNudge
                ) {
                    let nudge_n = plan_exec_state.nudge_count + 1;
                    tracing::info!(
                        target: "agent.plan_execution",
                        nudge_count = nudge_n,
                        total_steps = plan_exec_state.total_steps,
                        terminal_count = plan_exec_state.terminal_count,
                        "plan execution turn_streamed: model tried to exit with \
                         pending steps; injecting continuation nudge"
                    );

                    let _ = event_tx
                        .send(TurnEvent::StatusUpdate {
                            action: "Plan reminder".to_string(),
                            detail: format!(
                                "nudge {nudge_n} — {done}/{total} done, \
                                 {remaining} still pending",
                                done = plan_exec_state.terminal_count,
                                total = plan_exec_state.total_steps,
                                remaining = plan_exec_state.remaining(),
                            ),
                        })
                        .await;

                    Self::push_terminal_assistant_message(
                        &mut self.history,
                        final_text.clone(),
                        response.reasoning_content.clone(),
                    );
                    let msg = crate::agent::plan_execution_enforcement::nudge_message(
                        &plan_exec_state,
                    );
                    self.history.push(ConversationMessage::Chat(
                        ChatMessage::system(msg),
                    ));
                    plan_exec_state.nudge_count += 1;
                    continue;
                }

                if let (Some(cache), Some(key)) = (&self.response_cache, &cache_key) {
                    let token_count = response
                        .usage
                        .as_ref()
                        .and_then(|u| u.output_tokens)
                        .unwrap_or(0);
                    #[allow(clippy::cast_possible_truncation)]
                    let _ = cache.put(key, &effective_model, &final_text, token_count as u32);
                }

                if !got_stream && !final_text.is_empty() {
                    let _ = event_tx
                        .send(TurnEvent::Chunk {
                            delta: final_text.clone(),
                        })
                        .await;
                }

                Self::push_terminal_assistant_message(
                    &mut self.history,
                    final_text.clone(),
                    response.reasoning_content.clone(),
                );
                self.trim_history();

                self.finish_turn_experience(
                    user_message,
                    &final_text,
                    &tools_used_this_turn,
                    &tool_results_this_turn,
                );

                self.last_usage = turn_aggregated_usage
                    .clone()
                    .or_else(|| response.usage.clone());

                crate::evolution::record_provider_model(
                    Some(self.cached_provider.as_str()),
                    Some(effective_model.as_str()),
                );
                crate::evolution::set_response_text(&final_text);
                if let Some(ref reasoning) = response.reasoning_content {
                    crate::evolution::set_thinking_text(reasoning);
                }
                if let Some(ref usage) = self.last_usage {
                    let input = usage.input_tokens.unwrap_or(0);
                    let output = usage.output_tokens.unwrap_or(0);
                    crate::evolution::record_cost(
                        input,
                        output,
                        input.saturating_add(output),
                        0.0,
                    );
                }

                _turn_metrics_n1v2.mark_ok();
                return Ok(final_text);
            }

            self.history.push(ConversationMessage::AssistantToolCalls {
                text: response.text.clone(),
                tool_calls: response.tool_calls.clone(),
                reasoning_content: response.reasoning_content.clone(),
            });

            let mut deduped_calls = Vec::new();
            for call in &calls {
                let sig_args = call.arguments.to_string();
                if loop_state.record_tool_signature(&call.name, &sig_args) {
                    let _ = event_tx
                        .send(TurnEvent::ToolResult {
                            name: call.name.clone(),
                            output: "[Deduplicated] Already executed with identical arguments."
                                .into(),
                            success: true,
                        })
                        .await;
                } else {
                    deduped_calls.push(call.clone());
                }
            }

            if !got_stream {
                for call in &deduped_calls {
                    let _ = event_tx
                        .send(TurnEvent::ToolCall {
                            name: call.name.clone(),
                            args: call.arguments.clone(),
                        })
                        .await;
                }
            }

            let (calls_to_execute, prefab_denials) =
                Self::gate_tool_calls(&deduped_calls, &event_tx).await;

            let exec_results = self.execute_tools(&calls_to_execute).await;

            let mut results: Vec<ToolExecutionResult> =
                Vec::with_capacity(deduped_calls.len());
            let mut exec_iter = exec_results.into_iter();
            for (i, _call) in deduped_calls.iter().enumerate() {
                if let Some(denial) =
                    prefab_denials.iter().find(|(idx, _)| *idx == i).map(|(_, r)| r.clone())
                {
                    results.push(denial);
                } else if let Some(r) = exec_iter.next() {
                    results.push(r);
                }
            }
            let mut results = results;

            let mut pending_post_tool_system_messages: Vec<String> = Vec::new();

            for r in results.iter_mut() {
                if crate::agent::plan_mode_enforcement::is_ask_question_pause(&r.name, &r.output) {
                    awaiting_user_input = true;
                    r.output = crate::agent::plan_mode_enforcement::ASK_QUESTION_PAUSE_NOTICE
                        .to_string();
                }
            }

            let mut plan_finalized_this_iter = false;
            for (idx, r) in results.iter().enumerate() {
                tools_used_this_turn.push(r.name.clone());
                tool_results_this_turn.push((r.name.clone(), r.success));
                if r.success && r.name == "exit_plan_mode" {
                    plan_nudge_state.note_exit_plan_mode_success();
                    plan_finalized_this_iter = true;
                }

                if let Some(call) = deduped_calls.get(idx) {
                    plan_exec_state.observe_update_plan_call_at(
                        &r.name,
                        &call.arguments,
                        &r.output,
                        r.success,
                        Some(iteration),
                    );
                }
            }

            if !plan_finalized_this_iter
                && crate::agent::plan_mode_enforcement::detect_plan_mode_active(None)
                && !awaiting_user_input
            {
                plan_nudge_state.nudge_count += 1;
                let msg = crate::agent::plan_mode_enforcement::nudge_message(
                    &plan_nudge_state,
                );
                pending_post_tool_system_messages.push(msg.to_string());
            }

            for (idx, result) in results.iter().enumerate() {
                let args = deduped_calls.get(idx).map(|c| &c.arguments);
                let error_excerpt = if result.success {
                    None
                } else {
                    Some(result.output.as_str())
                };
                crate::evolution::record_tool_outcome(
                    &result.name,
                    result.success,
                    None,
                    None,
                    args,
                    Some(result.output.as_str()),
                    error_excerpt,
                );
                let _ = event_tx
                    .send(TurnEvent::ToolResult {
                        name: result.name.clone(),
                        output: result.output.clone(),
                        success: result.success,
                    })
                    .await;
            }

            {
                let results_with_args: Vec<(String, serde_json::Value, String)> = results
                    .iter()
                    .zip(deduped_calls.iter())
                    .map(|(r, c)| (r.name.clone(), c.arguments.clone(), r.output.clone()))
                    .collect();

                let loop_result = loop_state.record_tool_results_with_args(&results_with_args);
                match loop_result {
                    Err(msg) => {
                        return Err(AgentError::ToolDispatchFailed(msg));
                    }
                    Ok(Some(msg)) => {
                        pending_post_tool_system_messages.push(msg);
                    }
                    Ok(None) => {}
                }
            }

            {
                let had_file_edit = results.iter().any(|r| {
                    r.success
                        && crate::agent::mode_effects::is_file_mutation_tool(r.name.as_str())
                });
                if had_file_edit {
                    for r in results.iter().filter(|r| {
                        r.success
                            && crate::agent::mode_effects::is_file_mutation_tool(r.name.as_str())
                    }) {
                        let path = extract_file_edit_path(&r.name, &r.output);
                        if !path.is_empty() {
                            let (additions, deletions) = count_diff_lines(&r.output);
                            let diff = if r.output.len() < 4_096 {
                                Some(r.output.clone())
                            } else {
                                None
                            };

                            let edit_batch_id = {
                                let history =
                                    crate::tools::edit_history::EditHistory::new(
                                        self.workspace_dir.clone(),
                                    );
                                history.latest_batch_id_for(std::path::Path::new(&path))
                            };
                            let _ = event_tx
                                .send(TurnEvent::FileEdit {
                                    path,
                                    additions,
                                    deletions,
                                    diff,
                                    edit_batch_id,
                                })
                                .await;
                        }
                    }

                    if let Some(mode) = self.current_coding_mode {
                        if let Some(nudge) =
                            crate::agent::mode_effects::file_mod_auto_verify_nudge(mode)
                        {
                            pending_post_tool_system_messages.push(nudge.to_string());
                        }
                    }
                }
            }

            let formatted = self.tool_dispatcher.format_results(&results);
            self.history.push(formatted);

            if cancel.is_cancelled()
                || self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
            {
                let _ = event_tx
                    .send(TurnEvent::Cancelling {
                        reason: "user_requested".into(),
                    })
                    .await;
                return Ok(String::new());
            }

            for body in pending_post_tool_system_messages.drain(..) {
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::system(body)));
            }

            if let Some(mode) = self.current_coding_mode {
                if let Some(msg) = crate::agent::mode_effects::post_tool_batch_message(mode) {
                    self.history
                        .push(ConversationMessage::Chat(ChatMessage::system(
                            msg.to_string(),
                        )));
                }
            }

            self.trim_history();

            let pair_break_mode = self
                .current_coding_mode
                .filter(|m| m.breaks_turn_after_tool_batch());
            if let Some(intercepted_mode) = pair_break_mode {
                tracing::info!(
                    target: "agent.pair_mode",
                    "turn_streamed pausing: Pair Checkpoint after tool batch"
                );
                let pair_text = "_Pair Checkpoint: tool batch complete. Pausing for your \
                    input — type to continue or redirect, or send the next instruction._"
                    .to_string();
                crate::agent::mode_effects::record_mode_intercept(
                    crate::agent::mode_effects::ModeInterceptReason::PairCheckpoint,
                    &crate::agent::mode_effects::ModeInterceptContext {
                        mode: intercepted_mode,
                        channel: Some("desktop"),
                        provider: Some(self.cached_provider.as_str()),
                        model: None,
                        turn_id: None,
                        tool: None,
                        tool_call_id: None,
                        iteration: None,
                        message: Some("Pair Checkpoint pause"),
                    },
                );
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::system(
                        "[Pair Checkpoint] Turn paused after tool batch. The runtime returned \
                         control to the user. The next user message will resume execution."
                            .to_string(),
                    )));
                let _ = event_tx
                    .send(TurnEvent::StatusUpdate {
                        action: "Pair Checkpoint".to_string(),
                        detail: pair_text.clone(),
                    })
                    .await;
                self.finish_turn_experience(
                    user_message,
                    &pair_text,
                    &tools_used_this_turn,
                    &tool_results_this_turn,
                );
                _turn_metrics_n1v2.mark_ok();
                return Ok(pair_text);
            }

            if plan_finalized_this_iter {
                tracing::info!(
                    target: "agent.plan_mode",
                    "turn_streamed halting: exit_plan_mode succeeded; \
                     waiting for user's Build → Switch click"
                );
                let halt_text = "_Plan finalised. Waiting for the user to click \
                    **Build** in the plan card to switch to Agent mode and start \
                    executing._"
                    .to_string();
                let _ = event_tx
                    .send(TurnEvent::StatusUpdate {
                        action: "Plan ready".to_string(),
                        detail: halt_text.clone(),
                    })
                    .await;
                self.finish_turn_experience(
                    user_message,
                    &halt_text,
                    &tools_used_this_turn,
                    &tool_results_this_turn,
                );
                _turn_metrics_n1v2.mark_ok();
                return Ok(halt_text);
            }

            if awaiting_user_input {
                tracing::info!(
                    target: "agent.plan_mode",
                    "turn_streamed pausing: ask_question is awaiting user reply (plan nudge suppressed)"
                );
                let pause_text =
                    "_Waiting for the user's reply to the clarifying question(s) above._"
                        .to_string();

                let _ = event_tx
                    .send(TurnEvent::StatusUpdate {
                        action: "Awaiting answer".to_string(),
                        detail: pause_text.clone(),
                    })
                    .await;
                self.finish_turn_experience(
                    user_message,
                    &pause_text,
                    &tools_used_this_turn,
                    &tool_results_this_turn,
                );
                _turn_metrics_n1v2.mark_ok();
                return Ok(pause_text);
            }
        }

        Err(AgentError::LoopOverflow(self.config.max_tool_iterations))
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
                    let reset_body = "[Plan-Mode Reset] Disregard any prior \"Step N completed\", \"开始执行 Step N\", \"Starting step …\", \"executing\", \"已完成\" or other execution-voice framing inherited from earlier turns or other modes. You are now ONLY drafting/refining a plan document; no step has been executed yet, no work is in progress, and the user has not clicked Build. Speak strictly in planning voice (\"will\", \"propose\", \"draft\", \"would\"); never claim any todo is finished, never narrate progress. If the user has not asked you anything new, simply wait — do not start a fake execution recap.";
                    self.history.push(ConversationMessage::Chat(
                        ChatMessage::system(reset_body.to_string()),
                    ));
                }

                tracing::info!(
                    target: "agent.mode",
                    from = %prev_mode,
                    to = %mode,
                    "coding mode switched mid-turn — contract pushed to history"
                );
            }
        }
    }

    pub fn arm_plan_execution(&self, plan_path: impl Into<String>) {
        *self.plan_execution_armed.lock() = Some(plan_path.into());
    }

    pub(crate) fn take_plan_execution_arm(&self) -> Option<String> {
        self.plan_execution_armed.lock().take()
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
        self.mode_filter_dirty = false;
    }

    fn sync_config_from_store(&mut self) -> ConfigChange {
        let config = self.shared_config.load();

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
        } else if model_changed || self.temperature != config.default_temperature {
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

    pub async fn reload_provider(&mut self) -> Result<()> {

        let config = self.shared_config.load_ref();

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

        let new_provider = providers::create_routed_provider_with_options(
            &provider_name,
            if api_key.is_empty() {
                None
            } else {
                Some(&api_key)
            },
            if api_url.is_empty() {
                None
            } else {
                Some(&api_url)
            },
            &reliability,
            &model_routes,
            &model_name,
            &provider_runtime_options,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create provider: {}", e))?;

        self.provider = new_provider;
        self.model_name = model_name;

        self.cached_provider = provider_name_raw;
        self.cached_api_key = crate::security::secret_string::SecretString::new(api_key);
        self.cached_api_url = api_url;

        tracing::info!("Provider reloaded successfully");
        Ok(())
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
        self.workspace_dir = path.clone();

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
        self.mode_filter_dirty = true;
        self.cached_tools_signature = signature;
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
        match crate::tools::McpRegistry::connect_all(&config.mcp.servers).await {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                if config.mcp.deferred_loading {
                    let deferred_set =
                        crate::tools::DeferredMcpToolSet::from_registry(std::sync::Arc::clone(
                            &registry,
                        ))
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
            Err(e) => {
                tracing::error!("MCP reload failed to initialise registry: {e:#}");
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
        let has_web_search = self.tools.iter().any(|t| t.name() == "web_search");
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
            self.tools.retain(|t| t.name() != "web_search");
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
    }

    pub fn seed_history(&mut self, messages: &[ChatMessage]) {
        if self.history.is_empty() {
            if let Ok(sys) = self.build_system_prompt() {
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::system(sys)));
            }
        }
        let mut expanded =
            super::sqlite_gateway_hydrate::hydrate_gateway_sqlite_messages(messages);
        Self::repair_orphan_tool_result_messages(&mut expanded);
        self.activate_deferred_tools_from_history(&expanded);
        self.history.extend(expanded);
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
                    let entry = crate::tools::tool_tier::classify(&tc.name, surface);
                    if !matches!(
                        entry.tier,
                        crate::tools::tool_tier::BuiltinToolTier::OnDemand
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
            if let Some(tool_search_tool) =
                self.tools.iter().find(|t| t.name() == "tool_search")
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
        if crate::services::credential_vault::try_get_credential_vault().is_none() {
            let anchor = if config.workspace_dir.as_os_str().is_empty() {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            } else {
                config.workspace_dir.clone()
            };
            if let Err(err) = crate::services::credential_vault::init_credential_vault(&anchor) {
                tracing::warn!(error = %err, "credential vault initialisation failed for agent session");
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

        let memory: Arc<dyn Memory> = Arc::from(memory::create_memory_with_storage_and_routes(
            &config.memory,
            &config.embedding_routes,
            Some(&config.storage.provider.config),
            &config.workspace_dir,
            config.api_key.as_deref(),
        )?);

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
            _plan_mode_flag,
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
                "Initializing MCP client — {} server(s) configured",
                config.mcp.servers.len()
            );
            match tools::McpRegistry::connect_all(&config.mcp.servers).await {
                Ok(registry) => {
                    let registry = std::sync::Arc::new(registry);
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

        let provider: Box<dyn Provider> = providers::create_routed_provider_with_options(
            &provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &provider_runtime_options,
        )?;

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
            crate::memory::response_cache::ResponseCache::with_hot_cache(
                &config.workspace_dir,
                config.memory.response_cache_ttl_minutes,
                config.memory.response_cache_max_entries,
                config.memory.response_cache_hot_entries,
            )
            .ok()
            .map(Arc::new)
        } else {
            None
        };

        crate::agent::token_optimizer::ensure_global_optimizer_from_config(config);

        crate::token_saver::set_enabled(config.token_saver.enabled);
        crate::token_saver::set_global(config.token_saver.to_runtime_ctx());
        crate::guardrails::ensure_global_guardrails(config.guardrails.clone());

        let experience_replay = if config.experience.enabled {
            Some(crate::agent::experience::ExperienceReplay::new(
                &config.experience,
            ))
        } else {
            None
        };

        if let Some(ref deny_list) = denied_tools {
            let deny_set: std::collections::HashSet<_> = deny_list.iter().cloned().collect();
            tools.retain(|t| !deny_set.contains(t.name()));
        }

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
            .skills(crate::skills::load_skills_with_config(
                &config.workspace_dir,
                config,
            ))
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
            .shared_config(shared_config.unwrap_or_else(crate::config::live::LiveConfig::default))
            .cached_provider_config(
                provider_name_raw.to_string(),
                config.api_key.clone().unwrap_or_default(),
                config.api_url.clone().unwrap_or_default(),
            )
            .desktop_security_policy(Some(Arc::clone(&security)))
            .build()
    }

    fn trim_history(&mut self) {
        let max_messages = self.config.max_history_messages;

        const MAX_CHARS: usize = 400_000;

        let mut system_messages = Vec::new();
        let mut other_messages = Vec::new();

        for msg in self.history.drain(..) {
            match &msg {
                ConversationMessage::Chat(chat) if chat.role == "system" => {
                    system_messages.push(msg);
                }
                _ => other_messages.push(msg),
            }
        }

        if other_messages.len() > max_messages {
            let drop_count = other_messages.len() - max_messages;
            other_messages.drain(0..drop_count);
        }

        let mut total_chars: usize = system_messages.iter().map(|m| Self::msg_char_len(m)).sum();
        let mut start = 0;
        for (i, msg) in other_messages.iter().enumerate() {
            total_chars += Self::msg_char_len(msg);
            if total_chars > MAX_CHARS && i > 0 {
                start = i;
                break;
            }
        }
        if start > 0 {
            other_messages.drain(0..start);
        }

        self.history = system_messages;
        self.history.extend(other_messages);
        Self::repair_orphan_tool_result_messages(&mut self.history);
    }

    fn push_terminal_assistant_message(
        history: &mut Vec<ConversationMessage>,
        body: String,
        reasoning_content: Option<String>,
    ) {
        let reasoning_trimmed = reasoning_content
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty());
        let has_reasoning = reasoning_trimmed.is_some();
        let text_trimmed = body.trim_end().to_string();
        let text_opt = (!text_trimmed.is_empty()).then_some(text_trimmed);

        if has_reasoning {
            history.push(ConversationMessage::AssistantToolCalls {
                text: text_opt,
                tool_calls: Vec::new(),
                reasoning_content: reasoning_trimmed,
            });
            return;
        }
        if let Some(t) = text_opt {
            history.push(ConversationMessage::Chat(ChatMessage::assistant(t)));
        }
    }

    fn repair_orphan_tool_result_messages(history: &mut Vec<ConversationMessage>) {
        Self::upgrade_native_json_assistants_in_place(history);
        Self::collapse_empty_assistant_tool_calls(history);

        let mut out = Vec::with_capacity(history.len());
        for msg in history.drain(..) {
            match &msg {
                ConversationMessage::ToolResults(rows) => {
                    let preceded = out
                        .last()
                        .is_some_and(|p| matches!(p, ConversationMessage::AssistantToolCalls { .. }));
                    if preceded {
                        out.push(msg);
                    } else {
                        tracing::warn!(
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
                        tracing::warn!(
                            target: "agent.history_repair",
                            "recovered orphaned Chat(role=tool) as synthetic user transcript (missing assistant preamble)"
                        );
                        out.push(Self::recover_chat_tool_as_user(c));
                    }
                }
                _ => out.push(msg),
            }
        }
        *history = out;
        crate::agent::dangling_tool_repair::ensure_assistant_tool_replies_inplace(history);
    }

    fn collapse_empty_assistant_tool_calls(history: &mut Vec<ConversationMessage>) {
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

    fn upgrade_native_json_assistants_in_place(history: &mut Vec<ConversationMessage>) {
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
            _ => 200,
        }
    }

    fn estimate_history_tokens_internal(&self) -> usize {
        self.history
            .iter()
            .map(|m| Self::msg_char_len(m).div_ceil(4) + 4)
            .sum()
    }

    fn estimate_history_tokens_filtered(&self, system_only: bool) -> usize {
        self.history
            .iter()
            .filter(|m| {
                let is_sys = matches!(m, ConversationMessage::Chat(c) if c.role == "system");
                if system_only { is_sys } else { !is_sys }
            })
            .map(|m| Self::msg_char_len(m).div_ceil(4) + 4)
            .sum()
    }

    fn build_system_prompt(&self) -> Result<String> {
        let instructions = self.tool_dispatcher.prompt_instructions(&self.tools);
        let live_cfg = self.shared_config.load();
        let coding_mode_label = self.current_coding_mode.map(|m| m.label());
        let ctx = PromptContext {
            workspace_dir: &self.workspace_dir,
            model_name: &self.model_name,
            tools: &self.tools,
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

        let user_profile = crate::agent::user_profile::UserProfile::new(
            &self.workspace_dir,
            self.user_profile_config.clone(),
        );
        if let Some(profile_text) = user_profile.prompt_injection() {
            prompt.push_str(&profile_text);
        }

        let skill_engine =
            crate::agent::skill_evolution::SkillEvolutionEngine::new(&self.skill_evolution_config);
        if let Some(skill_text) = skill_engine.prompt_injection() {
            prompt.push_str(&skill_text);
        }

        let prompt_optimizer =
            crate::agent::prompt_optimizer::PromptOptimizer::new(&self.prompt_optimizer_config);
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

        Ok(prompt)
    }

    fn finish_turn_experience(
        &self,
        user_query: &str,
        assistant_response: &str,
        tools_used: &[String],
        tool_results: &[(String, bool)],
    ) {
        let Some(ref replay) = self.experience_replay else {
            return;
        };
        if !replay.collection_enabled() {
            return;
        }
        let refs: Vec<(&str, bool)> = tool_results.iter().map(|(n, s)| (n.as_str(), *s)).collect();
        let dims = crate::agent::self_eval::heuristic_eval(user_query, assistant_response, &refs);
        let reward = (dims.aggregate() * 2.0) - 1.0;

        let query_category =
            crate::agent::classifier::classify(&self.classification_config, user_query)
                .unwrap_or_else(|| "general".to_string());

        let experience = crate::agent::experience::Experience {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: self.memory_session_id.clone().unwrap_or_default(),
            timestamp: chrono::Utc::now(),
            user_query: user_query.to_string(),
            assistant_response: assistant_response.to_string(),
            tools_used: tools_used.to_vec(),
            model: self.model_name.clone(),
            reward,
            query_category,
            replay_count: 0,
        };
        replay.store(experience);
    }

    async fn execute_tool_call(&self, call: &ParsedToolCall) -> ToolExecutionResult {
        let start = Instant::now();

        if self.cancel_signal.load_full().is_cancelled()
            || self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
        {
            return ToolExecutionResult {
                name: call.name.clone(),
                output: "[Cancelled by user]".to_string(),
                success: true,
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
            tracing::warn!(
                tool = call.name,
                "RBAC partially configured (engine={}, identity={}); skipping authorization",
                self.rbac_engine.is_some(),
                self.rbac_identity.is_some(),
            );
        }

        if let Some(mode) = self.current_coding_mode {
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
                    crate::agent::mode_effects::record_mode_intercept(
                        crate::agent::mode_effects::ModeInterceptReason::ToolNotAllowed,
                        &crate::agent::mode_effects::ModeInterceptContext {
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
                crate::agent::mode_effects::mode_blocks_tool(mode, call.name.as_str())
            {
                crate::agent::mode_effects::record_mode_intercept(
                    crate::agent::mode_effects::ModeInterceptReason::ReadOnlyPolicy,
                    &crate::agent::mode_effects::ModeInterceptContext {
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

        let coding_label = self.current_coding_mode.map(|m| m.label().to_string());
        let coding_label_lc = coding_label.as_deref().map(str::to_ascii_lowercase);
        let perm_mode_lc =
            crate::gateway::ws_desktop::desktop_runtime_state().permission_mode();
        let tool_lc = call.name.to_ascii_lowercase();
        let guardrail_ctx = crate::guardrails::GuardrailContext {
            coding_mode: coding_label_lc.as_deref(),
            permission_mode: Some(&perm_mode_lc),
            tool_name: Some(&tool_lc),
        };
        if let Err(reason) =
            crate::guardrails::check_tool_guardrails(&call.name, Some(&guardrail_ctx))
        {
            return ToolExecutionResult {
                name: call.name.clone(),
                output: format!("Blocked by guardrails: {reason}"),
                success: false,
                tool_call_id: call.tool_call_id.clone(),
            };
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
            match tool.execute(call.arguments.clone()).await {
                Ok(r) => {
                    observer.record_event(&ObserverEvent::ToolCall {
                        tool: call.name.clone(),
                        duration: start.elapsed(),
                        success: r.success,
                    });
                    if r.success {
                        let scrubbed = crate::agent::loop_::scrub_credentials(&r.output);
                        let call_name_owned = call.name.clone();
                        let out = tokio::task::spawn_blocking(move || {
                            crate::agent::token_optimizer::compress_output(
                                &call_name_owned,
                                &scrubbed,
                            )
                        })
                        .await
                        .unwrap_or_else(|_| String::new());
                        (out, true)
                    } else {
                        let reason = r.error.unwrap_or(r.output);
                        (
                            format!("Error: {}", crate::agent::loop_::scrub_credentials(&reason)),
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
        let (output, success) = if let Some(tool) =
            self.tools.iter().find(|t| t.name() == dispatch_call.name)
        {
            tokio::select! {
                biased;
                _ = cancel_handle.cancelled() => {
                    ("[Cancelled by user]".to_string(), true)
                }
                res = run_tool(tool.as_ref(), dispatch_call, &self.observer) => res,
            }
        } else if let Some(activated_arc) = self.activated_tools.as_ref() {
            let activated_opt = activated_arc.lock().get_resolved(&dispatch_call.name);
            if let Some(tool) = activated_opt {
                tokio::select! {
                    biased;
                    _ = cancel_handle.cancelled() => {
                        ("[Cancelled by user]".to_string(), true)
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
        crate::agent::runtime_hooks::publish_tool_event(&dispatch_call.name, success, wall_ms);

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

    async fn gate_tool_calls(
        deduped_calls: &[ParsedToolCall],
        event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> (Vec<ParsedToolCall>, Vec<(usize, ToolExecutionResult)>) {
        use crate::security::permissions::{
            gate_decision, ComposerPermissionMode, GateDecision,
        };

        let mode_str = crate::gateway::ws_desktop::desktop_runtime_state().permission_mode();
        let mode = ComposerPermissionMode::from_wire(&mode_str);

        let (auto_approve, protect_browser, protect_mcp) =
            crate::services::try_get_services()
                .map(|svc| {
                    let cfg = svc.config();
                    let approve: std::collections::HashSet<String> =
                        cfg.autonomy.auto_approve.iter().cloned().collect();
                    (
                        approve,
                        cfg.autonomy.protect_browser_tools,
                        cfg.autonomy.protect_mcp_tools,
                    )
                })
                .unwrap_or_else(|| (std::collections::HashSet::new(), true, true));

        let coding_allowed: Option<std::collections::HashSet<String>> =
            crate::services::try_get_services().and_then(|svc| {
                let mode = *svc.coding_mode.read();
                mode.allowed_tools()
                    .map(|set| set.into_iter().map(String::from).collect())
            });

        let mut to_execute: Vec<ParsedToolCall> = Vec::new();
        let mut denials: Vec<(usize, ToolExecutionResult)> = Vec::new();

        let mut approval_rx: Option<
            tokio::sync::broadcast::Receiver<crate::session::SessionEvent>,
        > = None;

        for (idx, call) in deduped_calls.iter().enumerate() {

            if let Some(ref allowed) = coding_allowed {
                if !allowed.contains(call.name.as_str()) {
                    let coding_mode_label = crate::services::try_get_services()
                        .map(|svc| svc.coding_mode.read().label().to_string())
                        .unwrap_or_else(|| "current".to_string());

                    let allowed_sorted = {
                        let mut v: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
                        v.sort_unstable();
                        v
                    };
                    let allowed_list = allowed_sorted.join(", ");
                    let denial_msg = format!(
                        "Tool '{tool}' is NOT permitted in {mode} mode. The runtime refused \
                         this call before any side effect occurred — no files were touched, \
                         no commands ran.\n\n\
                         In Plan mode the deliverable is a saved `.plan.md`, NOT direct \
                         edits.  Your immediate next step is to GATHER INFORMATION with \
                         read-only tools so you can write a real plan.  DO NOT jump \
                         straight to `exit_plan_mode` with a stub — `exit_plan_mode` \
                         enforces a hard quality gate (≥600 chars, ≥3 concrete todos, \
                         `## ` sections, file-path links, ```bash``` verification block) \
                         and will reject a thin submission too.\n\n\
                         Recommended exploration sequence for a multi-file change:\n\
                           1. `dir_list` / `glob_search` to enumerate every affected file.\n\
                           2. `content_search` (or `grep`) to count occurrences of the \
                              identifier you're changing.\n\
                           3. `file_read` on the entry points (e.g. `go.mod`, `README.md`, \
                              `Dockerfile`, top-level configs) to capture the exact \
                              current values.\n\
                           4. `update_plan(action=\"set\", steps=[…])` to draft.\n\
                           5. `exit_plan_mode(plan_content=…)` with the full Cursor-style \
                              markdown.\n\n\
                         The {n} tools available in this mode are:\n\n  {list}\n\n\
                         Use `ask_question` when the user's intent is genuinely ambiguous.  \
                         Do NOT retry '{tool}' — it will keep being denied until the user \
                         clicks Build → Switch.",
                        tool = call.name,
                        mode = coding_mode_label,
                        n = allowed_sorted.len(),
                        list = allowed_list,
                    );
                    tracing::info!(
                        target: "agent.gate",
                        tool = %call.name,
                        coding_mode = %coding_mode_label,
                        "coding-mode allowlist refused tool BEFORE permission UI"
                    );
                    denials.push((
                        idx,
                        ToolExecutionResult {
                            name: call.name.clone(),
                            output: denial_msg,
                            success: false,
                            tool_call_id: call.tool_call_id.clone(),
                        },
                    ));
                    continue;
                }
            }

            let mut decision = gate_decision(
                mode,
                &call.name,
                &auto_approve,
                protect_browser,
                protect_mcp,
            );
            if matches!(decision, GateDecision::Ask) {
                if approval_rx.is_none() {
                    approval_rx =
                        Some(crate::gateway::ws::subscribe_gateway_approval_events());
                }
                let request_id = format!("perm_{}", uuid::Uuid::new_v4());

                tracing::info!(
                    target: "agent.gate",
                    %request_id,
                    tool = %call.name,
                    mode = ?mode,
                    "emitting permission request to UI"
                );
                let send_start = std::time::Instant::now();
                if let Err(e) = event_tx
                    .send(TurnEvent::PermissionRequest {
                        request_id: request_id.clone(),
                        tool_name: call.name.clone(),
                        input: call.arguments.clone(),
                        description: Some(format!("Run tool `{}`", call.name)),
                    })
                    .await
                {

                    tracing::warn!(
                        target: "agent.gate",
                        %request_id,
                        tool = %call.name,
                        error = %e,
                        "permission request send failed; turning into deny"
                    );
                    decision = GateDecision::Deny;
                } else {
                    tracing::debug!(
                        target: "agent.gate",
                        %request_id,
                        send_ms = send_start.elapsed().as_millis() as u64,
                        "permission request delivered; awaiting user response"
                    );

                    let wait_start = std::time::Instant::now();
                    let (approved, updated_input) = Self::wait_for_permission_response(
                        approval_rx.as_mut().expect("subscribed above"),
                        &request_id,
                        std::time::Duration::from_secs(600),
                    )
                    .await;

                    tracing::info!(
                        target: "agent.gate",
                        %request_id,
                        tool = %call.name,
                        approved,
                        has_updated_input = updated_input.is_some(),
                        waited_ms = wait_start.elapsed().as_millis() as u64,
                        "permission response received"
                    );

                    decision = if approved {

                        if let Some(extra) = updated_input {
                            let mut merged = call.clone();
                            merge_tool_arguments(&mut merged.arguments, &extra);
                            to_execute.push(merged);
                            continue;
                        }
                        GateDecision::Auto
                    } else {
                        GateDecision::Deny
                    };
                }
            }

            match decision {
                GateDecision::Auto => to_execute.push(call.clone()),
                GateDecision::Deny => {
                    let denial_msg = match mode {
                        ComposerPermissionMode::Plan => format!(
                            "Plan mode is active — '{}' is a write/act tool and cannot run. \
Use a read-only tool, or call `exit_plan_mode` first.",
                            call.name
                        ),
                        _ => format!(
                            "Denied by user: '{}' was not permitted under the current permission policy.",
                            call.name
                        ),
                    };
                    let denial = ToolExecutionResult {
                        name: call.name.clone(),
                        output: denial_msg.clone(),
                        success: false,
                        tool_call_id: call.tool_call_id.clone(),
                    };
                    denials.push((idx, denial));
                }
                GateDecision::Ask => {

                }
            }
        }

        (to_execute, denials)
    }

    async fn wait_for_permission_response(
        rx: &mut tokio::sync::broadcast::Receiver<crate::session::SessionEvent>,
        request_id: &str,
        timeout: std::time::Duration,
    ) -> (bool, Option<serde_json::Value>) {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return (false, None);
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(evt)) => {
                    if let crate::session::SessionEventKind::ApprovalResponded {
                        id,
                        decision,
                        updated_input,
                        ..
                    } = &evt.kind
                    {
                        if id == request_id {
                            let approved = matches!(
                                decision.to_ascii_lowercase().as_str(),
                                "yes" | "always" | "approved" | "allow"
                            );
                            return (approved, updated_input.clone());
                        }
                    }
                }
                Ok(Err(_recv_err)) => return (false, None),
                Err(_timeout) => return (false, None),
            }
        }
    }

    async fn execute_tools(&self, calls: &[ParsedToolCall]) -> Vec<ToolExecutionResult> {

        let has_tool_search = calls.iter().any(|c| c.name == "tool_search");
        if !self.config.parallel_tools || has_tool_search {
            let mut results = Vec::with_capacity(calls.len());
            for call in calls {
                results.push(self.execute_tool_call(call).await);
            }
            return results;
        }

        let futs: Vec<_> = calls
            .iter()
            .map(|call| self.execute_tool_call(call))
            .collect();
        futures_util::future::join_all(futs).await
    }

    fn build_user_envelope(user_message: &str, context: &str) -> String {
        let now = chrono::Local::now();
        let (year, month, day) = (now.year(), now.month(), now.day());
        let (hour, minute, second) = (now.hour(), now.minute(), now.second());
        let tz = now.format("%Z");
        let date_str =
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {tz}");

        if context.is_empty() {
            format!("[CURRENT DATE & TIME: {date_str}]\n\n{user_message}")
        } else {
            format!("[CURRENT DATE & TIME: {date_str}]\n\n{context}\n\n{user_message}")
        }
    }

    fn try_response_cache_lookup(
        &mut self,
        messages: &[ChatMessage],
        effective_model: &str,
        user_message: &str,
    ) -> Option<String> {
        if self.temperature != 0.0 {
            return None;
        }
        let cache = self.response_cache.as_ref()?;
        let last_user = messages
            .iter()
            .rfind(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str());
        let key = crate::memory::response_cache::ResponseCache::cache_key(
            effective_model,
            system,
            last_user,
        );

        let provider_label = self.cached_provider.clone();
        let model_label = effective_model.to_string();
        match cache.get(&key) {
            Ok(Some(cached)) => {
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
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::assistant(
                        cached.clone(),
                    )));
                self.trim_history();
                self.finish_turn_experience(user_message, &cached, &[], &[]);
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

    fn try_response_cache_store(
        &self,
        messages: &[ChatMessage],
        effective_model: &str,
        final_text: &str,
        token_count: u64,
    ) {
        if self.temperature != 0.0 {
            return;
        }
        let Some(cache) = self.response_cache.as_ref() else {
            return;
        };
        let last_user = messages
            .iter()
            .rfind(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str());
        let key = crate::memory::response_cache::ResponseCache::cache_key(
            effective_model,
            system,
            last_user,
        );
        #[allow(clippy::cast_possible_truncation)]
        let _ = cache.put(&key, effective_model, final_text, token_count as u32);
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
                return format!("hint:{}", decision.hint);
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
                    return format!("hint:{hint}");
                }
            }
        }

        self.model_name.clone()
    }

    pub async fn turn(&mut self, user_message: &str) -> Result<String, AgentError> {

        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(TURN_EVENT_DRAIN_BUFFER);
        let drain = crate::runtime::spawn_supervised("agent.agent.drain", async move {
            while rx.recv().await.is_some() {

            }
        })
        .into_inner();
        let result = self.turn_streamed(user_message, tx).await;

        let _ = drain.await;
        result
    }

    pub async fn turn_via_loop_core(&mut self, user_message: &str) -> Result<String, AgentError> {
        use crate::agent::loop_core::AgentLoopCore;
        use crate::providers::traits::ChatMessage;

        if self.history.is_empty() {
            let system_prompt = self.build_system_prompt()?;
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(
                    system_prompt,
                )));
        }
        self.history
            .push(ConversationMessage::Chat(ChatMessage::user(
                user_message.to_string(),
            )));

        let mut flat_history: Vec<ChatMessage> = self
            .history
            .iter()
            .filter_map(|m| match m {
                ConversationMessage::Chat(c) => Some(c.clone()),
                _ => None,
            })
            .collect();

        let multimodal = crate::config::MultimodalConfig::default();
        let pacing = crate::config::PacingConfig::default();
        let excluded: Vec<String> = Vec::new();
        let dedup_exempt: Vec<String> = Vec::new();

        let mut metrics_guard = crate::agent::executor_core::TurnMetricsGuard::start();

        let core = AgentLoopCore::bare(
            self.provider.as_ref(),
            &self.tools,
            self.observer.as_ref(),
            &self.cached_provider,
            &self.model_name,
            &multimodal,
            &pacing,
            &excluded,
            &dedup_exempt,
        )
        .max_iterations(self.config.max_tool_iterations)
        .temperature(self.temperature);

        let result = core
            .run_turn(&mut flat_history, None)
            .await
            .map_err(|e| e.into());
        if result.is_ok() {
            metrics_guard.mark_ok();
        }
        metrics_guard.finish();

        if let Ok(ref text) = result {
            self.history
                .push(ConversationMessage::Chat(ChatMessage::assistant(
                    text.clone(),
                )));
        }

        result
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
                let _ = event_tx
                    .send(TurnEvent::Error {
                        message: format!("Failed to reload provider: {}", e),
                    })
                    .await;
                return Err(e);
            }
        }

        if self.history.is_empty() {
            let system_prompt = self.build_system_prompt()?;
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(
                    system_prompt,
                )));
        }

        let context = self
            .memory_loader
            .load_context(
                self.memory.as_ref(),
                user_message,
                self.memory_session_id.as_deref(),
            )
            .await
            .unwrap_or_default();

        if self.auto_save {
            let autosave_key = format!("user_msg_{}", Uuid::new_v4());
            let _ = self
                .memory
                .store(
                    &autosave_key,
                    user_message,
                    MemoryCategory::Conversation,
                    self.memory_session_id.as_deref(),
                )
                .await;
        }

        let mut enriched = Self::build_user_envelope(user_message, &context);

        if let Some(svc) = crate::services::try_get_services() {
            let mode = *svc.coding_mode.read();
            if let Some(reminder) = crate::agent::mode_effects::pre_turn_reminder(mode) {
                enriched.push_str("\n\n");
                enriched.push_str(reminder);
            }
            let cfg = svc.config();
            if let Some(web_reminder) = crate::agent::mode_effects::web_research_disabled_reminder(
                mode,
                cfg.web_search.enabled,
                cfg.web_fetch.enabled,
            ) {
                enriched.push_str("\n\n");
                enriched.push_str(web_reminder);
            }
        }

        self.history
            .push(ConversationMessage::Chat(ChatMessage::user(enriched)));

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
        self.model_name = model.clone();

        let (new_api_key, new_api_url) = {
            let config = self.shared_config.load();
            (
                config.api_key.clone().unwrap_or_default(),
                config.api_url.clone().unwrap_or_default(),
            )
        };

        let need_reload = provider != self.cached_provider
            || !self.cached_api_key.constant_time_eq(&new_api_key)
            || new_api_url != self.cached_api_url;

        self.cached_provider = provider.clone();
        self.cached_api_key = crate::security::secret_string::SecretString::new(new_api_key);
        self.cached_api_url = new_api_url;

        if need_reload {
            tracing::info!("Model switch requires provider reload, reloading...");
            if let Err(e) = self.reload_provider().await {
                tracing::error!("Failed to reload provider during model switch: {}", e);
                let _ = event_tx
                    .send(TurnEvent::Error {
                        message: format!("Provider reload failed: {}", e),
                    })
                    .await;
            }
        }

        crate::agent::loop_::clear_model_switch_request();
    }

    fn build_response_cache_key(
        &self,
        messages: &[crate::providers::traits::ChatMessage],
        effective_model: &str,
    ) -> Option<String> {
        if self.temperature != 0.0 {
            return None;
        }
        self.response_cache.as_ref().map(|_| {
            let last_user = messages
                .iter()
                .rfind(|m| m.role == "user")
                .map(|m| m.content.as_str())
                .unwrap_or("");
            let system = messages
                .iter()
                .find(|m| m.role == "system")
                .map(|m| m.content.as_str());
            crate::memory::response_cache::ResponseCache::cache_key(
                effective_model,
                system,
                last_user,
            )
        })
    }

    async fn try_response_cache_hit(
        &mut self,
        cache_key: &Option<String>,
        user_message: &str,
    ) -> Option<String> {
        let (Some(cache), Some(key)) = (&self.response_cache, cache_key) else {
            return None;
        };

        let provider_label = self.cached_provider.clone();
        let model_label = self.model_name.clone();
        match cache.get(key) {
            Ok(Some(cached)) => {
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
                self.history
                    .push(ConversationMessage::Chat(ChatMessage::assistant(
                        cached.clone(),
                    )));
                self.trim_history();
                self.finish_turn_experience(user_message, &cached, &[], &[]);
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

    async fn prepare_iteration_context_budget(
        &mut self,
        iteration: usize,
        event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) {
        let est_tokens = self.estimate_history_tokens_internal();
        let max_ctx: usize = self.config.max_context_tokens;
        if est_tokens > max_ctx * 80 / 100 {
            let tokens_before = est_tokens;
            self.trim_history();
            let tokens_after = self.estimate_history_tokens_internal();

            let _ = event_tx
                .send(TurnEvent::ContextCompressed {
                    tokens_before,
                    tokens_after,
                })
                .await;
        }
        let sys_tokens = self.estimate_history_tokens_filtered(true);
        let hist_tokens = self.estimate_history_tokens_filtered(false);
        let total = sys_tokens + hist_tokens;
        let remaining = max_ctx.saturating_sub(total);
        let pct = if max_ctx > 0 {
            remaining * 100 / max_ctx
        } else {
            0
        };
        if iteration > 0 || pct < 50 {
            let warning = if pct < 20 {
                " WARNING: context nearly full, prioritize essential information."
            } else {
                ""
            };
            let budget_msg = format!(
                "[Context Budget] System: ~{}k tokens. History: ~{}k tokens. \
                 Remaining: ~{}k tokens ({}% free).{}",
                sys_tokens / 1000,
                hist_tokens / 1000,
                remaining / 1000,
                pct,
                warning
            );
            self.history.retain(|m| {
                !matches!(
                    m,
                    ConversationMessage::Chat(c)
                        if c.role == "system" && c.content.trim_start().starts_with("[Context Budget]")
                )
            });
            self.history
                .push(ConversationMessage::Chat(ChatMessage::system(budget_msg)));
        }
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

fn merge_tool_arguments(arguments: &mut serde_json::Value, extra: &serde_json::Value) {
    let extra_obj = match extra.as_object() {
        Some(obj) => obj,
        None => {
            tracing::debug!(
                target: "agent.gate",
                "permission_response.updated_input was not an object; ignoring"
            );
            return;
        }
    };
    if !arguments.is_object() {
        *arguments = serde_json::Value::Object(extra_obj.clone());
        return;
    }
    let target = arguments
        .as_object_mut()
        .expect("checked is_object above");
    for (k, v) in extra_obj.iter() {
        target.insert(k.clone(), v.clone());
    }
}

fn extract_file_edit_path(tool_name: &str, output: &str) -> String {

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(output) {
        for key in ["path", "file_path", "filename", "file"] {
            if let Some(s) = value.get(key).and_then(|v| v.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }

    for line in output.lines().take(4) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        for token in trimmed.split_whitespace() {
            if token.len() > 2 && (token.contains('/') || token.contains('\\')) {

                let cleaned = token.trim_matches(|c: char| c.is_ascii_punctuation());
                if !cleaned.is_empty() {
                    return cleaned.to_string();
                }
            }
        }
    }
    let _ = tool_name;
    String::new()
}

fn count_diff_lines(output: &str) -> (i32, i32) {
    let mut adds = 0i32;
    let mut dels = 0i32;
    for line in output.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            adds += 1;
        } else if line.starts_with('-') {
            dels += 1;
        }
    }
    (adds, dels)
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
