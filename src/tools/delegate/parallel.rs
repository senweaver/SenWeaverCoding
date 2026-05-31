// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::super::traits::{Tool, ToolResult};
use crate::agent::multi_agent_runtime::{MultiAgentRuntime, global_runtime};
use crate::agent::scheduler::SchedulableTask;
use crate::agent::scheduler::runtime::TaskExecutor;
use crate::agent::subagent_limiter::{PermitResult, SubagentLimiter};
use crate::coordinator::delegation::{
    MergeStrategy, MergedOutput, SubTaskResult, merge_results_structured,
    merge_results_with_judge_structured,
};
use crate::observability::coordination_metrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubTaskInput {
    pub id: String,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_capability")]
    pub capability: String,
}

fn default_capability() -> String {
    "general".to_string()
}

fn default_merge_strategy() -> MergeStrategy {
    MergeStrategy::All
}

fn default_max_parallel() -> usize {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DelegateParallelArgs {
    pub tasks: Vec<SubTaskInput>,
    #[serde(default = "default_merge_strategy")]
    pub merge_strategy: MergeStrategy,
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,

    #[serde(default)]
    pub allow_single_agent_fallback: bool,
}

pub struct DelegateParallelTool {

    parent_tools: Option<Arc<RwLock<Vec<Arc<dyn Tool>>>>>,
    multimodal_config: crate::config::MultimodalConfig,

    workspace_root: Arc<RwLock<PathBuf>>,
    delegate_config: crate::config::DelegateToolConfig,
}

impl Default for DelegateParallelTool {
    fn default() -> Self {
        Self {
            parent_tools: None,
            multimodal_config: crate::config::MultimodalConfig::default(),
            workspace_root: Arc::new(RwLock::new(PathBuf::new())),
            delegate_config: crate::config::DelegateToolConfig::default(),
        }
    }
}

impl DelegateParallelTool {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_parent_tools(
        mut self,
        parent_tools: Arc<RwLock<Vec<Arc<dyn Tool>>>>,
    ) -> Self {
        self.parent_tools = Some(parent_tools);
        self
    }

    #[must_use]
    pub fn with_multimodal_config(mut self, cfg: crate::config::MultimodalConfig) -> Self {
        self.multimodal_config = cfg;
        self
    }

    #[must_use]
    pub fn with_workspace_root(mut self, root: Arc<RwLock<PathBuf>>) -> Self {
        self.workspace_root = root;
        self
    }

    #[must_use]
    pub fn with_delegate_config(mut self, cfg: crate::config::DelegateToolConfig) -> Self {
        self.delegate_config = cfg;
        self
    }
}

fn err_result(msg: impl Into<String>) -> ToolResult {
    let msg = msg.into();
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(msg),
    }
}

type DegradedNotesMap = std::collections::HashMap<String, (bool, Option<String>)>;

fn lock_degraded_notes(
    notes: &std::sync::Mutex<DegradedNotesMap>,
) -> std::sync::MutexGuard<'_, DegradedNotesMap> {
    notes.lock().unwrap_or_else(|poisoned| {
        tracing::warn!(
            target = "delegate_parallel",
            "degraded_notes mutex poisoned by prior worker panic; recovering inner state"
        );
        poisoned.into_inner()
    })
}

fn ok_result(output: impl Into<String>) -> ToolResult {
    ToolResult {
        success: true,
        output: output.into(),
        error: None,
    }
}

#[async_trait]
impl Tool for DelegateParallelTool {
    fn name(&self) -> &str {
        "delegate_parallel"
    }

    fn description(&self) -> &str {
        "Delegate multiple sub-tasks to run in parallel via the multi-agent runtime. \
         Tasks can declare dependencies; the scheduler runs independent tasks \
         concurrently and merges results using the configured MergeStrategy."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "List of sub-tasks to execute",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "description": {"type": "string"},
                            "prompt": {"type": "string"},
                            "depends_on": {
                                "type": "array",
                                "items": {"type": "string"},
                                "default": []
                            },
                            "capability": {
                                "type": "string",
                                "default": "general"
                            }
                        },
                        "required": ["id", "description", "prompt"]
                    }
                },
                "merge_strategy": {
                    "type": "string",
                    "enum": ["first", "all", "voting", "llm_judge"],
                    "default": "all"
                },
                "max_parallel": {
                    "type": "integer",
                    "default": 4,
                    "minimum": 1,
                    "maximum": 32
                },
                "allow_single_agent_fallback": {
                    "type": "boolean",
                    "description": "When true, missing multi-agent runtime / capability degrade to a single-agent fallback. Default false (strict).",
                    "default": false
                }
            },
            "required": ["tasks"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let args: DelegateParallelArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(err_result(format!("Invalid arguments: {e}"))),
        };

        if args.tasks.is_empty() {
            return Ok(err_result("At least one task is required"));
        }

        let allow_fallback = args.allow_single_agent_fallback;

        let schedulable: Vec<SchedulableTask> = args
            .tasks
            .iter()
            .map(|t| {
                let mut s =
                    SchedulableTask::new(t.id.clone(), t.description.clone(), t.prompt.clone());
                s.required_capability = t.capability.clone();
                for dep in &t.depends_on {
                    s = s.with_dependency(dep.clone());
                }
                s
            })
            .collect();

        let degraded_notes: Arc<std::sync::Mutex<DegradedNotesMap>> =
            Arc::new(std::sync::Mutex::new(DegradedNotesMap::new()));
        let degraded_notes_exec = degraded_notes.clone();

        let (limiter, call_timeout) = match crate::services::try_get_services() {
            Some(svc) => {
                let cfg = svc.config();
                let limiter = match crate::agent::multi_agent_runtime::global_runtime() {
                    Some(rt) => rt.subagent_limiter.clone(),
                    None => Arc::new(SubagentLimiter::new(
                        &cfg.agent_runtime.subagent_limit,
                    )),
                };
                let secs = cfg.agent_runtime.subagent_call_timeout_secs;
                let timeout = if secs == 0 {
                    None
                } else {
                    Some(Duration::from_secs(secs))
                };
                (limiter, timeout)
            }
            None => (
                Arc::new(SubagentLimiter::new(
                    &crate::agent::subagent_limiter::SubagentLimitConfig::default(),
                )),
                Some(Duration::from_secs(120)),
            ),
        };
        let limiter_exec = limiter.clone();

        let delegation_root_id = format!(
            "delegate_parallel:{}",
            uuid::Uuid::new_v4()
        );
        let _delegation_root_handle = limiter.register(
            delegation_root_id.clone(),
            None,
            tokio_util::sync::CancellationToken::new(),
        );
        let delegation_root_for_exec = delegation_root_id.clone();

        let parent_tools_exec = self.parent_tools.clone();
        let multimodal_exec = self.multimodal_config.clone();
        let workspace_root_exec = Arc::clone(&self.workspace_root);
        let delegate_cfg_exec = self.delegate_config.clone();

        let exec: TaskExecutor = Arc::new(move |task, ct| {
            let id = task.id.clone();
            let prompt = task.prompt.clone();
            let capability = task.required_capability.clone();
            let notes = degraded_notes_exec.clone();
            let limiter = limiter_exec.clone();
            let call_timeout = call_timeout;
            let cancel = ct.clone();
            let parent_tools = parent_tools_exec.clone();
            let multimodal = multimodal_exec.clone();
            let workspace_root = Arc::clone(&workspace_root_exec);
            let delegate_cfg = delegate_cfg_exec.clone();
            let delegation_root = delegation_root_for_exec.clone();
            Box::pin(async move {

                let _lineage = limiter.register(
                    id.clone(),
                    Some(delegation_root.clone()),
                    cancel.clone(),
                );

                let _permit = match limiter.try_acquire() {
                    PermitResult::Granted(p) => p,
                    PermitResult::Queued => {

                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(60);
                        let mut wait = std::time::Duration::from_millis(25);
                        loop {
                            if cancel.is_cancelled() {
                                return Err(format!(
                                    "subagent '{id}' cancelled while waiting for permit"
                                ));
                            }
                            tokio::time::sleep(wait).await;
                            wait = (wait * 2).min(std::time::Duration::from_millis(500));
                            if let PermitResult::Granted(p) = limiter.try_acquire() {
                                break p;
                            }
                            if std::time::Instant::now() > deadline {
                                return Err(format!(
                                    "subagent '{id}' timed out waiting for limiter permit (active={}/{})",
                                    limiter.active_count(),
                                    limiter.max_concurrent()
                                ));
                            }
                        }
                    }
                    PermitResult::Rejected { active, max } => {
                        return Err(format!(
                            "subagent '{id}' rejected: limiter at capacity ({active}/{max})"
                        ));
                    }
                };
                let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
                    coordination_metrics::incr_delegate_parallel_no_runtime();
                    if allow_fallback {
                        coordination_metrics::incr_delegate_parallel_fallback();
                        lock_degraded_notes(&notes).insert(
                            id.clone(),
                            (
                                true,
                                Some(
                                    "no multi_agent_runtime; single-agent fallback".to_string(),
                                ),
                            ),
                        );
                        return single_agent_fallback(&id, &prompt, cancel.clone(), call_timeout)
                            .await;
                    }
                    return Err(format!(
                        "delegate_parallel: multi_agent_runtime not initialized (task '{id}')"
                    ));
                };

                let Some(agent_info) = rt.registry.find_best_available(&capability) else {
                    coordination_metrics::incr_delegate_parallel_no_capability();
                    tracing::debug!(
                        capability = %capability,
                        task_id = %id,
                        "delegate_parallel: no agent matches capability"
                    );
                    if allow_fallback {
                        coordination_metrics::incr_delegate_parallel_fallback();
                        lock_degraded_notes(&notes).insert(
                            id.clone(),
                            (
                                true,
                                Some(format!(
                                    "no agent matches capability '{capability}'; single-agent fallback"
                                )),
                            ),
                        );
                        return single_agent_fallback(&id, &prompt, cancel.clone(), call_timeout)
                            .await;
                    }
                    return Err(format!(
                        "delegate_parallel: no agent matches capability '{capability}' for task '{id}'"
                    ));
                };

                let (
                    provider_name,
                    model,
                    system_prompt,
                    api_key,
                    api_url,
                    temperature,
                    agentic,
                    allowed_tools,
                    max_iterations,
                    agentic_timeout_secs,
                ) = match crate::services::try_get_services() {
                    Some(svc) => {
                        let cfg = svc.shared_config.load();

                        let agent_key = agent_info
                            .id
                            .rsplit_once('/')
                            .map(|(_, n)| n)
                            .unwrap_or(&agent_info.name);
                        if let Some(dcfg) = cfg.agents.get(agent_key) {
                            (
                                dcfg.provider.clone(),
                                dcfg.model.clone(),
                                dcfg.system_prompt.clone(),
                                dcfg.api_key.clone(),
                                None,
                                dcfg.temperature.unwrap_or(0.7),
                                dcfg.agentic,
                                dcfg.allowed_tools.clone(),
                                dcfg.max_iterations,
                                dcfg.agentic_timeout_secs,
                            )
                        } else {
                            let resolved_model = match crate::providers::resolve_default_model(&cfg) {
                                Ok(m) => m,
                                Err(e) => {
                                    lock_degraded_notes(&notes).insert(
                                        id.clone(),
                                        (
                                            true,
                                            Some(format!(
                                                "no_model_configured: {e}"
                                            )),
                                        ),
                                    );
                                    return Err(format!(
                                        "delegate_parallel: no_model_configured for task '{id}': {e}"
                                    ));
                                }
                            };
                            (
                                cfg.default_provider
                                    .clone()
                                    .unwrap_or_else(|| "openrouter".into()),
                                resolved_model,
                                None,
                                cfg.api_key.clone(),
                                cfg.api_url.clone(),
                                cfg.default_temperature,
                                false,
                                Vec::new(),
                                10,
                                None,
                            )
                        }
                    }
                    None => {
                        if allow_fallback {
                            coordination_metrics::incr_delegate_parallel_fallback();
                            lock_degraded_notes(&notes).insert(
                                id.clone(),
                                (
                                    true,
                                    Some(
                                        "services container missing; echo fallback".into(),
                                    ),
                                ),
                            );
                            return Ok(format!("[{}] {}", id, prompt));
                        }
                        return Err(format!(
                            "delegate_parallel: services container not initialized (task '{id}')"
                        ));
                    }
                };

                let _ = rt.registry.assign_task(&agent_info.id, &id);
                rt.blackboard.inner().write(
                    &id,
                    serde_json::json!({
                        "delegated_to": &agent_info.id,
                        "agent_name": &agent_info.name,
                        "provider": &provider_name,
                        "model": &model,
                        "started_at": chrono::Utc::now().to_rfc3339(),
                    }),
                    "delegate_parallel",
                    "delegations",
                );

                let resolved_provider_name = match crate::services::try_get_services() {
                    Some(svc) => {
                        let cfg = svc.shared_config.load();
                        crate::providers::resolve_runtime_provider_name(&provider_name, &cfg)
                    }
                    None => provider_name.clone(),
                };
                let provider = match crate::providers::create_provider_with_url_async(
                    resolved_provider_name,
                    api_key,
                    api_url,
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        rt.registry.complete_task(&agent_info.id, false);
                        return Err(format!("provider build failed for '{provider_name}': {e}"));
                    }
                };

                let result = if agentic
                    && !allowed_tools.is_empty()
                    && let Some(parent_tools_ref) = parent_tools.as_ref()
                {
                    let workspace_dir_buf = workspace_root.read().clone();
                    let chosen_timeout = Duration::from_secs(
                        agentic_timeout_secs
                            .unwrap_or(delegate_cfg.agentic_timeout_secs.max(1)),
                    );
                    run_role_agentic(
                        RoleAgenticCtx {
                            agent_id: agent_info.id.clone(),
                            id: id.clone(),
                            prompt: prompt.clone(),
                            provider: provider.as_ref(),
                            provider_name: &provider_name,
                            model: &model,
                            system_prompt: system_prompt.as_deref(),
                            temperature,
                            allowed_tools: &allowed_tools,
                            max_iterations,
                            timeout: chosen_timeout,
                            multimodal: &multimodal,
                            workspace_dir: workspace_dir_buf.as_path(),
                            parent_tools: parent_tools_ref,
                            cancel: cancel.clone(),
                        },
                    )
                    .await
                } else {

                    let chat_fut = provider.chat_with_system(
                        system_prompt.as_deref(),
                        &prompt,
                        &model,
                        temperature,
                    );
                    let raw = if let Some(timeout) = call_timeout {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {
                                rt.registry.complete_task(&agent_info.id, false);
                                return Err(format!(
                                    "sub-agent '{}' cancelled mid-call", agent_info.id
                                ));
                            }
                            _ = tokio::time::sleep(timeout) => {
                                rt.registry.complete_task(&agent_info.id, false);

                                limiter.on_overrun(&id);
                                return Err(format!(
                                    "sub-agent '{}' timed out after {:?}",
                                    agent_info.id, timeout
                                ));
                            }
                            r = chat_fut => r,
                        }
                    } else {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {
                                rt.registry.complete_task(&agent_info.id, false);
                                return Err(format!(
                                    "sub-agent '{}' cancelled mid-call", agent_info.id
                                ));
                            }
                            r = chat_fut => r,
                        }
                    };
                    raw.map_err(|e| format!("sub-agent chat failed: {e}"))
                };

                match result {
                    Ok(output) => {
                        rt.registry.complete_task(&agent_info.id, true);

                        rt.blackboard.inner().write(
                            &id,
                            serde_json::json!({
                                "delegated_to": &agent_info.id,
                                "output_preview": output.chars().take(200).collect::<String>(),
                                "completed_at": chrono::Utc::now().to_rfc3339(),
                                "agentic": agentic,
                            }),
                            "delegate_parallel",
                            "results",
                        );
                        Ok(output)
                    }
                    Err(e) => {
                        rt.registry.complete_task(&agent_info.id, false);
                        Err(format!("sub-agent '{}' failed: {e}", agent_info.id))
                    }
                }
            })
        });

        let outcomes = match global_runtime() {
            Some(rt) => {
                match rt
                    .submit_task_graph(schedulable, args.max_parallel, exec)
                    .await
                {
                    Ok(o) => o,
                    Err(e) => return Ok(err_result(format!("Task graph rejected: {e}"))),
                }
            }
            None => {

                let rt = MultiAgentRuntime::new();
                match rt
                    .submit_task_graph(schedulable, args.max_parallel, exec)
                    .await
                {
                    Ok(o) => o,
                    Err(e) => return Ok(err_result(format!("Task graph rejected: {e}"))),
                }
            }
        };

        let notes_snapshot = lock_degraded_notes(&degraded_notes).clone();
        let results: Vec<SubTaskResult> = outcomes
            .into_iter()
            .map(|o| {
                let (degraded, reason) = notes_snapshot
                    .get(&o.task_id)
                    .cloned()
                    .unwrap_or((false, None));
                SubTaskResult {
                    task_id: o.task_id,
                    agent_id: "delegate_parallel".into(),
                    output: o.result,
                    success: o.success,
                    confidence: None,
                    degraded,
                    reason,
                }
            })
            .collect();

        let merged: MergedOutput = if matches!(args.merge_strategy, MergeStrategy::LlmJudge) {
            let (provider_opt, model, temperature) = match crate::services::try_get_services() {
                Some(svc) => {
                    let cfg = svc.shared_config.load();
                    let provider_name = cfg
                        .default_provider
                        .clone()
                        .unwrap_or_else(|| "openrouter".into());
                    let resolved_provider_name =
                        crate::providers::resolve_runtime_provider_name(&provider_name, &cfg);
                    match crate::providers::resolve_default_model(&cfg) {
                        Ok(model) => {
                            let temperature = cfg.default_temperature;
                            let provider = crate::providers::create_provider_with_url_async(
                                resolved_provider_name,
                                cfg.api_key.clone(),
                                cfg.api_url.clone(),
                            )
                            .await
                            .ok()
                            .map(|p| Arc::from(p));
                            (provider, model, temperature)
                        }
                        Err(e) => {
                            tracing::warn!(
                                target = "delegate_parallel",
                                "no_model_configured for llm-judge merge: {e}"
                            );
                            (None, String::new(), 0.0)
                        }
                    }
                }
                None => (None, String::new(), 0.0),
            };
            merge_results_with_judge_structured(
                &results,
                args.merge_strategy,
                provider_opt,
                &model,
                temperature,
            )
            .await
        } else {
            merge_results_structured(&results, args.merge_strategy)
        };

        let payload = serde_json::json!({
            "merged": merged.merged,
            "metadata": {
                "degraded": merged.degraded,
                "reasons": merged.reasons,
                "failures": merged.failures,
                "tasks": results.iter().map(|r| serde_json::json!({
                    "task_id": r.task_id,
                    "success": r.success,
                    "degraded": r.degraded,
                    "reason": r.reason,
                })).collect::<Vec<_>>(),
            }
        });
        match serde_json::to_string(&payload) {
            Ok(s) => Ok(ok_result(s)),
            Err(e) => Ok(err_result(format!("delegate_parallel: serialize merged output failed: {e}"))),
        }
    }
}

async fn single_agent_fallback(
    id: &str,
    prompt: &str,
    cancel: tokio_util::sync::CancellationToken,
    call_timeout: Option<Duration>,
) -> Result<String, String> {
    let (provider_name, model, api_key, api_url, temperature) =
        match crate::services::try_get_services() {
            Some(svc) => {
                let cfg = svc.shared_config.load();
                let raw_provider_name = cfg
                    .default_provider
                    .clone()
                    .unwrap_or_else(|| "openrouter".into());
                let resolved_provider_name =
                    crate::providers::resolve_runtime_provider_name(&raw_provider_name, &cfg);
                let resolved_model = match crate::providers::resolve_default_model(&cfg) {
                    Ok(m) => m,
                    Err(e) => {
                        return Err(format!(
                            "single-agent fallback: no_model_configured: {e}"
                        ));
                    }
                };
                (
                    resolved_provider_name,
                    resolved_model,
                    cfg.api_key.clone(),
                    cfg.api_url.clone(),
                    cfg.default_temperature,
                )
            }
            None => return Ok(format!("[{}] {}", id, prompt)),
        };
    let provider = match crate::providers::create_provider_with_url_async(
        provider_name.clone(),
        api_key,
        api_url,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => return Err(format!("single-agent fallback: provider build failed: {e}")),
    };
    let chat_fut = provider.chat_with_system(None, prompt, &model, temperature);
    let result = if let Some(timeout) = call_timeout {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(format!("single-agent fallback for '{id}' cancelled"));
            }
            _ = tokio::time::sleep(timeout) => {
                return Err(format!(
                    "single-agent fallback for '{id}' timed out after {:?}",
                    timeout
                ));
            }
            r = chat_fut => r,
        }
    } else {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(format!("single-agent fallback for '{id}' cancelled"));
            }
            r = chat_fut => r,
        }
    };
    result.map_err(|e| format!("single-agent fallback: chat failed: {e}"))
}

struct RoleAgenticCtx<'a> {
    agent_id: String,
    id: String,
    prompt: String,
    provider: &'a dyn crate::providers::Provider,
    provider_name: &'a str,
    model: &'a str,
    system_prompt: Option<&'a str>,
    temperature: f64,
    allowed_tools: &'a [String],
    max_iterations: usize,
    timeout: Duration,
    multimodal: &'a crate::config::MultimodalConfig,
    workspace_dir: &'a std::path::Path,
    parent_tools: &'a Arc<RwLock<Vec<Arc<dyn Tool>>>>,
    cancel: tokio_util::sync::CancellationToken,
}

async fn run_role_agentic(ctx: RoleAgenticCtx<'_>) -> Result<String, String> {
    use crate::agent::SubagentChunkKind;
    use crate::agent::loop_::{DraftEvent, take_parent_draft_channel};
    use crate::providers::ChatMessage;

    let allowed: std::collections::HashSet<&str> = ctx
        .allowed_tools
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let sub_tools: Vec<Box<dyn Tool>> = {
        let parent_tools = ctx.parent_tools.read();
        parent_tools
            .iter()
            .filter(|tool| allowed.contains(tool.name()))

            .filter(|tool| {
                tool.name() != "delegate" && tool.name() != "delegate_parallel"
            })
            .map(|tool| {
                Box::new(crate::tools::ArcToolRef(tool.clone())) as Box<dyn Tool>
            })
            .collect()
    };

    if sub_tools.is_empty() {
        return Err(format!(
            "role '{}' has no executable tools after filtering allowlist ({:?})",
            ctx.agent_id, ctx.allowed_tools
        ));
    }

    let mut history: Vec<ChatMessage> = Vec::new();
    if let Some(sys) = ctx.system_prompt {
        if !sys.trim().is_empty() {
            history.push(ChatMessage::system(sys.to_string()));
        }
    } else {

        history.push(ChatMessage::system(format!(
            "You are a sub-agent running in workspace {}.\n\
             Use only the provided tools. Return a concise final answer.",
            ctx.workspace_dir.display()
        )));
    }
    history.push(ChatMessage::user(ctx.prompt.clone()));

    let observer = crate::observability::noop::NoopObserver;
    let pacing = crate::config::PacingConfig::default();

    let (sub_tx, mut sub_rx) =
        tokio::sync::mpsc::channel::<DraftEvent>(64);
    let parent_for_bridge = take_parent_draft_channel();
    let task_id_for_bridge = ctx.id.clone();
    let agent_id_for_bridge = ctx.agent_id.clone();
    let bridge_handle = if let Some(parent) = parent_for_bridge.clone() {
        Some(crate::runtime::spawn_supervised(
            "delegate_parallel.subagent_bridge",
            async move {
                while let Some(event) = sub_rx.recv().await {
                    let translated = match event {
                        DraftEvent::Content(text) => Some(DraftEvent::Subagent {
                            task_id: task_id_for_bridge.clone(),
                            agent_id: agent_id_for_bridge.clone(),
                            kind: SubagentChunkKind::Chunk,
                            delta: text,
                        }),
                        DraftEvent::Thinking(text) => Some(DraftEvent::Subagent {
                            task_id: task_id_for_bridge.clone(),
                            agent_id: agent_id_for_bridge.clone(),
                            kind: SubagentChunkKind::Thinking,
                            delta: text,
                        }),
                        DraftEvent::ToolCall { name, .. } => {
                            Some(DraftEvent::Subagent {
                                task_id: task_id_for_bridge.clone(),
                                agent_id: agent_id_for_bridge.clone(),
                                kind: SubagentChunkKind::ToolCall,
                                delta: name,
                            })
                        }
                        DraftEvent::ToolResult {
                            name,
                            output,
                            success: _,
                            tool_call_id: _,
                        } => {
                            let preview = output.chars().take(160).collect::<String>();
                            Some(DraftEvent::Subagent {
                                task_id: task_id_for_bridge.clone(),
                                agent_id: agent_id_for_bridge.clone(),
                                kind: SubagentChunkKind::ToolResult,
                                delta: format!("{name}: {preview}"),
                            })
                        }
                        DraftEvent::Progress(text) => Some(DraftEvent::Subagent {
                            task_id: task_id_for_bridge.clone(),
                            agent_id: agent_id_for_bridge.clone(),
                            kind: SubagentChunkKind::Status,
                            delta: text,
                        }),

                        _ => None,
                    };
                    if let Some(t) = translated {
                        if parent.send(t).await.is_err() {
                            tracing::debug!(
                                target: "delegate_parallel.subagent_bridge",
                                "parent draft channel dropped; stopping bridge"
                            );
                            break;
                        }
                    }
                }
            },
        )
        .into_inner())
    } else {

        drop(sub_rx);
        None
    };

    let on_delta_for_loop = if bridge_handle.is_some() {
        Some(sub_tx)
    } else {
        None
    };

    let delegated_policy = crate::agent::loop_::policy::PolicyBundle::delegated(
        ctx.provider,
        &sub_tools,
        &observer,
        ctx.provider_name,
        ctx.model,
        ctx.multimodal,
        &pacing,
        &[],
        &[],
    )
    .with_channel_name("delegate_parallel")
    .with_temperature(ctx.temperature)
    .with_max_iterations(ctx.max_iterations)
    .with_cancellation(Some(ctx.cancel.clone()))
    .with_on_delta(on_delta_for_loop);
    let loop_fut =
        crate::agent::loop_::unified::UnifiedLoop::new(delegated_policy).run(&mut history);

    let result = tokio::select! {
        biased;
        _ = ctx.cancel.cancelled() => {
            if let Some(h) = bridge_handle.as_ref() { h.abort(); }
            return Err(format!(
                "sub-agent '{}' (task '{}') cancelled mid-loop",
                ctx.agent_id, ctx.id
            ));
        }
        _ = tokio::time::sleep(ctx.timeout) => {
            if let Some(h) = bridge_handle.as_ref() { h.abort(); }
            return Err(format!(
                "sub-agent '{}' (task '{}') timed out after {:?}",
                ctx.agent_id, ctx.id, ctx.timeout
            ));
        }
        r = loop_fut => r,
    };

    if let Some(h) = bridge_handle { h.abort(); }

    match result {
        Ok(out) if out.trim().is_empty() => Ok("[Empty response]".to_string()),
        Ok(out) => Ok(out),
        Err(e) => Err(format!("agentic loop failed: {e}")),
    }
}
