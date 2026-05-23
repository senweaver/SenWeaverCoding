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
use uuid::Uuid;

pub(crate) const TURN_EVENT_DRAIN_BUFFER: usize = 1024;

#[derive(Debug, Clone)]
pub enum TurnEvent {

    Chunk { delta: String },

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

        let mut cache = self.merged_specs_cache.lock();
        if let Some(entry) = cache.as_ref() {
            if entry.activation_revision == revision && entry.base_ptr == base_ptr {
                return std::sync::Arc::clone(&entry.merged);
            }
        }

        let extra = activated_arc.lock().tool_specs();
        if extra.is_empty() {
            *cache = None;
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

        let mut _turn_metrics = crate::agent::executor_core::TurnMetricsGuard::start();

        let _ = event_tx
            .send(TurnEvent::ProgressTick {
                iteration: 0,
                max_iterations: self.config.max_tool_iterations,
                tokens_used: 0,
            })
            .await;

        self.apply_turn_preamble(user_message, &event_tx).await?;
        self.apply_gui_model_switch(&event_tx).await;
        let effective_model = self.classify_model(user_message);

        let mut history_chat = self.tool_dispatcher.to_provider_messages(&self.history);

        let cancel = self.cancel_signal.load_full().as_ref().clone();
        let live_cfg = self.shared_config.load_ref();
        let multimodal = live_cfg.multimodal.clone();
        let pacing = live_cfg.pacing.clone();
        let dedup_exempt = live_cfg.agent.tool_call_dedup_exempt.clone();
        drop(live_cfg);
        let excluded_tools: Vec<String> = Vec::new();
        let provider_name = self.cached_provider.clone();

        let hook_runner_arc = self.hook_runner.as_ref().and_then(|h| h.current());
        let hook_runner_ref = hook_runner_arc.as_deref();

        let gui_hooks: Arc<GuiHooksFromAgent> = Arc::new(GuiHooksFromAgent::from_agent(self));

        let final_text = {
            let policy = crate::agent::loop_policy::PolicyBundle::gui(
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
            .with_cancellation(Some(cancel))
            .with_event_sink(crate::agent::event_sink::EventSink::turn(event_tx.clone()))
            .with_activated_tools(self.activated_tools.as_ref())
            .with_hooks(hook_runner_ref)
            .with_rbac(self.rbac_engine.as_ref(), self.rbac_identity.as_ref())
            .with_model_switch_callback(Some(crate::agent::loop_::get_model_switch_state()))
            .with_response_cache_hook(Some(gui_hooks.clone()))
            .with_memory_session_hook(Some(gui_hooks.clone()))
            .with_model_classifier_hook(Some(gui_hooks.clone()))
            .with_turn_preamble_hook(Some(gui_hooks.clone()))
            .with_gui_model_switch_hook(Some(gui_hooks.clone()))
            .with_iteration_context_budget_hook(Some(gui_hooks.clone()))
            .with_experience_recorder_hook(Some(gui_hooks.clone()))
            .with_plan_mode_nudge_hook(Some(gui_hooks.clone()));

            match crate::agent::loop_unified::UnifiedLoop::new(policy)
                .run(&mut history_chat)
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    let msg = err.to_string();
                    let _ = event_tx
                        .send(TurnEvent::Error {
                            message: msg.clone(),
                        })
                        .await;
                    return Err(AgentError::ToolDispatchFailed(msg));
                }
            }
        };

        Self::replace_history_from_flat(&mut self.history, history_chat);
        self.trim_history();

        crate::evolution::record_provider_model(
            Some(self.cached_provider.as_str()),
            Some(effective_model.as_str()),
        );
        crate::evolution::set_response_text(&final_text);

        _turn_metrics.mark_ok();
        Ok(final_text)
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
        let mirrored =
            crate::providers::sanitize::mirror_tool_ids_in_chat_messages(messages.to_vec());
        let cleaned =
            crate::providers::sanitize::clean_empty_assistant_tool_calls_in_chat_messages(mirrored);
        let mut expanded =
            super::sqlite_gateway_hydrate::hydrate_gateway_sqlite_messages(&cleaned);
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
            _ => 200,
        }
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
                        let scrubbed = crate::agent::pii_sanitize::scrub_credentials(&r.output);
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
                            format!("Error: {}", crate::agent::pii_sanitize::scrub_credentials(&reason)),
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
            if let Some(web_active) = crate::agent::mode_effects::web_research_active_reminder(
                mode,
                cfg.web_search.enabled,
                cfg.web_fetch.enabled,
            ) {
                enriched.push_str("\n\n");
                enriched.push_str(web_active);
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
    available_hints: Vec<String>,
    route_model_by_hint: HashMap<String, String>,
    auto_classify: Option<crate::agent::eval::AutoClassifyConfig>,
    default_model: String,
    temperature: f64,
    experience_replay: Option<crate::agent::experience::ExperienceReplay>,
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
            available_hints: agent.available_hints.clone(),
            route_model_by_hint: agent.route_model_by_hint.clone(),
            auto_classify: agent.config.auto_classify.clone(),
            default_model: agent.model_name.clone(),
            temperature: agent.temperature,
            experience_replay: agent.experience_replay.clone(),
            observer: agent.observer.clone(),
            cached_provider: agent.cached_provider.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_hooks::ResponseCacheHook for GuiHooksFromAgent {
    fn build_key(&self, messages: &[ChatMessage], model: &str) -> Option<String> {
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
            crate::memory::response_cache::ResponseCache::cache_key(model, system, last_user)
        })
    }

    async fn try_hit(&self, key: &str, _user_message: &str) -> Option<String> {
        let cache = self.response_cache.as_ref()?;
        let provider_label = self.cached_provider.clone();
        let model_label = self.default_model.clone();
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
            let _ = cache.put(key, model, response, output_tokens);
        }
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_hooks::MemorySessionHook for GuiHooksFromAgent {
    async fn on_turn_start(&self, user_message: &str) {
        if !self.auto_save {
            return;
        }
        let key = format!("user_msg_{}", Uuid::new_v4());
        let _ = self
            .memory
            .store(
                &key,
                user_message,
                MemoryCategory::Conversation,
                self.memory_session_id.as_deref(),
            )
            .await;
    }

    async fn on_turn_end(&self, assistant_message: &str, _tools_used: &[String]) {
        if !self.auto_save {
            return;
        }
        let key = format!("assistant_msg_{}", Uuid::new_v4());
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

impl crate::agent::loop_hooks::ModelClassifierHook for GuiHooksFromAgent {
    fn classify(&self, user_message: &str) -> Option<String> {
        if let Some(decision) = crate::agent::classifier::classify_with_decision(
            &self.classification_config,
            user_message,
        ) {
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
                    "Classified message route via hook"
                );
                return Some(format!("route:{}", decision.hint));
            }
        }
        if let Some(ref ac) = self.auto_classify {
            let tier = crate::agent::eval::estimate_complexity(user_message);
            if let Some(hint) = ac.hint_for(tier) {
                if self.available_hints.contains(&hint.to_string()) {
                    tracing::info!(
                        target: "query_classification",
                        hint = hint,
                        complexity = ?tier,
                        message_length = user_message.len(),
                        "Auto-classified by complexity via hook"
                    );
                    return Some(format!("route:{hint}"));
                }
            }
        }
        None
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_hooks::TurnPreambleHook for GuiHooksFromAgent {
    async fn apply(
        &self,
        _user_message: &str,
        _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> Result<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_hooks::GuiModelSwitchHook for GuiHooksFromAgent {
    async fn poll(&self, _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>) -> Option<String> {
        None
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_hooks::IterationContextBudgetHook for GuiHooksFromAgent {
    async fn prepare(
        &self,
        _iteration: usize,
        _event_tx: &tokio::sync::mpsc::Sender<TurnEvent>,
    ) {
    }
}

#[async_trait::async_trait]
impl crate::agent::loop_hooks::ExperienceRecorderHook for GuiHooksFromAgent {
    async fn record(&self, summary: &crate::agent::loop_hooks::TurnExperienceSummary) {
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
        let dims = crate::agent::self_eval::heuristic_eval(
            &summary.user_query,
            &summary.assistant_response,
            &refs,
        );
        let reward = (dims.aggregate() * 2.0) - 1.0;
        let query_category =
            crate::agent::classifier::classify(&self.classification_config, &summary.user_query)
                .unwrap_or_else(|| "general".to_string());
        let experience = crate::agent::experience::Experience {
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
impl crate::agent::loop_hooks::PlanModeNudgeHook for GuiHooksFromAgent {
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
