// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::agent::prompt::{PromptContext, SystemPromptBuilder};
use crate::config::{DelegateAgentConfig, DelegateToolConfig};
use crate::observability::traits::{Observer, ObserverEvent, ObserverMetric};
use crate::providers::{self, ChatMessage, Provider};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackgroundDelegateResult {
    pub task_id: String,
    pub agent: String,
    pub status: BackgroundTaskStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

pub struct DelegateTool {

    agents: Arc<RwLock<HashMap<String, DelegateAgentConfig>>>,
    security: Arc<SecurityPolicy>,

    fallback_credential: Option<String>,

    provider_runtime_options: providers::ProviderRuntimeOptions,

    depth: u32,

    parent_tools: Arc<RwLock<Vec<Arc<dyn Tool>>>>,

    multimodal_config: crate::config::MultimodalConfig,

    delegate_config: DelegateToolConfig,

    workspace_root: Arc<RwLock<PathBuf>>,

    cancellation_token: CancellationToken,
}

impl DelegateTool {
    pub fn new(
        agents: HashMap<String, DelegateAgentConfig>,
        fallback_credential: Option<String>,
        security: Arc<SecurityPolicy>,
    ) -> Self {
        Self::new_with_options(
            agents,
            fallback_credential,
            security,
            providers::ProviderRuntimeOptions::default(),
        )
    }

    pub fn new_with_options(
        agents: HashMap<String, DelegateAgentConfig>,
        fallback_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        provider_runtime_options: providers::ProviderRuntimeOptions,
    ) -> Self {
        Self {
            agents: Arc::new(RwLock::new(agents)),
            security,
            fallback_credential,
            provider_runtime_options,
            depth: 0,
            parent_tools: Arc::new(RwLock::new(Vec::new())),
            multimodal_config: crate::config::MultimodalConfig::default(),
            delegate_config: DelegateToolConfig::default(),
            workspace_root: Arc::new(RwLock::new(PathBuf::new())),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn with_depth(
        agents: HashMap<String, DelegateAgentConfig>,
        fallback_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        depth: u32,
    ) -> Self {
        Self::with_depth_and_options(
            agents,
            fallback_credential,
            security,
            depth,
            providers::ProviderRuntimeOptions::default(),
        )
    }

    pub fn with_depth_and_options(
        agents: HashMap<String, DelegateAgentConfig>,
        fallback_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        depth: u32,
        provider_runtime_options: providers::ProviderRuntimeOptions,
    ) -> Self {
        Self {
            agents: Arc::new(RwLock::new(agents)),
            security,
            fallback_credential,
            provider_runtime_options,
            depth,
            parent_tools: Arc::new(RwLock::new(Vec::new())),
            multimodal_config: crate::config::MultimodalConfig::default(),
            delegate_config: DelegateToolConfig::default(),
            workspace_root: Arc::new(RwLock::new(PathBuf::new())),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn refresh_agents(&self, next: HashMap<String, DelegateAgentConfig>) {
        *self.agents.write() = next;
    }

    pub fn agents_handle(&self) -> Arc<RwLock<HashMap<String, DelegateAgentConfig>>> {
        Arc::clone(&self.agents)
    }

    pub fn with_parent_tools(mut self, parent_tools: Arc<RwLock<Vec<Arc<dyn Tool>>>>) -> Self {
        self.parent_tools = parent_tools;
        self
    }

    pub fn with_multimodal_config(mut self, config: crate::config::MultimodalConfig) -> Self {
        self.multimodal_config = config;
        self
    }

    pub fn with_delegate_config(mut self, config: DelegateToolConfig) -> Self {
        self.delegate_config = config;
        self
    }

    pub fn parent_tools_handle(&self) -> Arc<RwLock<Vec<Arc<dyn Tool>>>> {
        Arc::clone(&self.parent_tools)
    }

    pub fn workspace_snapshot(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    pub fn with_workspace_root(mut self, root: Arc<RwLock<PathBuf>>) -> Self {
        self.workspace_root = root;
        self
    }

    pub fn with_cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = token;
        self
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    fn results_dir(&self) -> PathBuf {
        self.workspace_snapshot().join("delegate_results")
    }

    fn validate_task_id(task_id: &str) -> Result<(), String> {
        if uuid::Uuid::parse_str(task_id).is_err() {
            return Err(format!("Invalid task_id '{task_id}': must be a valid UUID"));
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Delegate a subtask to a specialized agent. Use when: a task benefits from a different model \
         (e.g. fast summarization, deep reasoning, code generation). The sub-agent runs a single \
         prompt by default; with agentic=true it can iterate with a filtered tool-call loop. \
         Supports background execution (returns a task_id immediately) and parallel execution \
         (runs multiple agents concurrently). Use action='check_result' with a task_id to \
         retrieve background results."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let agent_names: Vec<String> = {
            let guard = self.agents.read();
            guard.keys().cloned().collect()
        };
        let agent_names_view: Vec<&str> = agent_names.iter().map(|s| s.as_str()).collect();
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["delegate", "check_result", "list_results", "cancel_task"],
                    "description": "Action to perform. Default: 'delegate'. Use 'check_result' to \
                                    retrieve a background task result, 'list_results' to list all \
                                    background tasks, 'cancel_task' to cancel a running background task.",
                    "default": "delegate"
                },
                "agent": {
                    "type": "string",
                    "minLength": 1,
                    "description": format!(
                        "Name of the agent to delegate to. Available: {}",
                        if agent_names_view.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            agent_names_view.join(", ")
                        }
                    )
                },
                "prompt": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The task/prompt to send to the sub-agent"
                },
                "context": {
                    "type": "string",
                    "description": "Optional context to prepend (e.g. relevant code, prior findings)"
                },
                "background": {
                    "type": "boolean",
                    "description": "When true, the sub-agent runs in a background tokio task and \
                                    returns a task_id immediately. Results are stored to \
                                    workspace/delegate_results/{task_id}.json.",
                    "default": false
                },
                "parallel": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Array of agent names to run concurrently with the same prompt. \
                                    Returns all results when all agents complete. Cannot be combined \
                                    with 'background'."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID for check_result/cancel_task actions (returned by \
                                    background delegation)."
                }
            },
            "required": []
        })
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("delegate");

        match action {
            "check_result" => return self.handle_check_result(&args).await,
            "list_results" => return self.handle_list_results().await,
            "cancel_task" => return self.handle_cancel_task(&args).await,
            "delegate" => {}
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Unknown action '{other}'. Use delegate/check_result/list_results/cancel_task."
                    )),
                });
            }
        }

        if let Some(parallel_agents) = args.get("parallel").and_then(|v| v.as_array()) {
            return self.execute_parallel(parallel_agents, &args).await;
        }

        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| anyhow::anyhow!("Missing 'agent' parameter"))?;

        if agent_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("'agent' parameter must not be empty".into()),
            });
        }

        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| anyhow::anyhow!("Missing 'prompt' parameter"))?;

        if prompt.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("'prompt' parameter must not be empty".into()),
            });
        }

        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if background {
            return self.execute_background(agent_name, prompt, &args).await;
        }

        self.execute_sync(agent_name, prompt, &args).await
    }
}

impl DelegateTool {

    async fn execute_sync(
        &self,
        agent_name: &str,
        prompt: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");

        let agent_config = {
            let guard = self.agents.read();
            match guard.get(agent_name) {
                Some(cfg) => cfg.clone(),
                None => {
                    let available: Vec<String> = guard.keys().cloned().collect();
                    drop(guard);
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Unknown agent '{agent_name}'. Available agents: {}",
                            if available.is_empty() {
                                "(none configured)".to_string()
                            } else {
                                available.join(", ")
                            }
                        )),
                    });
                }
            }
        };

        if self.depth >= agent_config.max_depth {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Delegation depth limit reached ({depth}/{max}). \
                     Cannot delegate further to prevent infinite loops.",
                    depth = self.depth,
                    max = agent_config.max_depth
                )),
            });
        }

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "delegate")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        crate::agent::profile::runtime_hooks::track_delegate_spawn(
            agent_name,
            &agent_config.provider,
            &agent_config.model,
        );

        let provider_credential_owned = agent_config
            .api_key
            .clone()
            .or_else(|| self.fallback_credential.clone());
        #[allow(clippy::option_as_ref_deref)]
        let provider_credential = provider_credential_owned.as_ref().map(String::as_str);

        let runtime_provider_name = match crate::services::try_get_services() {
            Some(services) => {
                let cfg = services.config();
                providers::resolve_runtime_provider_name(&agent_config.provider, &cfg)
            }
            None => agent_config.provider.clone(),
        };

        let provider: Box<dyn Provider> = match providers::create_provider_with_options_async(
            runtime_provider_name,
            provider_credential.map(str::to_string),
            self.provider_runtime_options.clone(),
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Failed to create provider '{}' for agent '{agent_name}': {e}",
                        agent_config.provider
                    )),
                });
            }
        };

        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        let temperature = agent_config.temperature.unwrap_or(0.7);

        if agent_config.agentic {
            return self
                .execute_agentic(
                    agent_name,
                    &agent_config,
                    &*provider,
                    &full_prompt,
                    temperature,
                )
                .await;
        }

        let ws_root = self.workspace_snapshot();
        let enriched_system_prompt =
            self.build_enriched_system_prompt(&agent_config, &[], ws_root.as_path());
        let system_prompt_ref = enriched_system_prompt.as_deref();

        let timeout_secs = agent_config
            .timeout_secs
            .unwrap_or(self.delegate_config.timeout_secs);
        let result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            provider.chat_with_system(
                system_prompt_ref,
                &full_prompt,
                &agent_config.model,
                temperature,
            ),
        )
        .await;

        let result = match result {
            Ok(inner) => inner,
            Err(_elapsed) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Agent '{agent_name}' timed out after {timeout_secs}s"
                    )),
                });
            }
        };

        match result {
            Ok(response) => {
                let mut rendered = response;
                if rendered.trim().is_empty() {
                    rendered = "[Empty response]".to_string();
                }

                crate::agent::profile::runtime_hooks::track_delegate_complete(agent_name, true);

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "[Agent '{agent_name}' ({provider}/{model})]\n{rendered}",
                        provider = agent_config.provider,
                        model = agent_config.model
                    ),
                    error: None,
                })
            }
            Err(e) => {

                crate::agent::profile::runtime_hooks::track_delegate_complete(agent_name, false);
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Agent '{agent_name}' failed: {e}",)),
                })
            }
        }
    }
}

impl DelegateTool {

    async fn execute_background(
        &self,
        agent_name: &str,
        prompt: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        let agent_config = {
            let guard = self.agents.read();
            match guard.get(agent_name) {
                Some(cfg) => cfg.clone(),
                None => {
                    let available: Vec<String> = guard.keys().cloned().collect();
                    drop(guard);
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Unknown agent '{agent_name}'. Available agents: {}",
                            if available.is_empty() {
                                "(none configured)".to_string()
                            } else {
                                available.join(", ")
                            }
                        )),
                    });
                }
            }
        };

        if self.depth >= agent_config.max_depth {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Delegation depth limit reached ({depth}/{max}).",
                    depth = self.depth,
                    max = agent_config.max_depth
                )),
            });
        }

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "delegate")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let task_id = uuid::Uuid::new_v4().to_string();
        let results_dir = self.results_dir();
        tokio::fs::create_dir_all(&results_dir).await?;

        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        let started_at = chrono::Utc::now().to_rfc3339();
        let agent_name_owned = agent_name.to_string();

        let initial_result = BackgroundDelegateResult {
            task_id: task_id.clone(),
            agent: agent_name_owned.clone(),
            status: BackgroundTaskStatus::Running,
            output: None,
            error: None,
            started_at: started_at.clone(),
            finished_at: None,
        };
        let result_path = results_dir.join(format!("{task_id}.json"));
        let json_bytes = serde_json::to_vec_pretty(&initial_result)?;
        tokio::fs::write(&result_path, &json_bytes).await?;

        let agents = Arc::clone(&self.agents);
        let security = Arc::clone(&self.security);
        let fallback_credential = self.fallback_credential.clone();
        let provider_runtime_options = self.provider_runtime_options.clone();
        let depth = self.depth;
        let parent_tools = Arc::clone(&self.parent_tools);
        let multimodal_config = self.multimodal_config.clone();
        let delegate_config = self.delegate_config.clone();
        let workspace_root = Arc::clone(&self.workspace_root);
        let child_token = self.cancellation_token.child_token();
        let task_id_clone = task_id.clone();

        let _bg_task = crate::runtime::spawn_supervised(
            format!("tools.delegate.bg_task.{}", task_id_clone),
            async move {

                let inner = DelegateTool {
                    agents,
                    security,
                    fallback_credential,
                    provider_runtime_options,
                    depth,
                    parent_tools,
                    multimodal_config,
                    delegate_config,
                    workspace_root: Arc::clone(&workspace_root),
                    cancellation_token: child_token.clone(),
                };

                let args_inner = json!({
                    "agent": agent_name_owned,
                    "prompt": full_prompt,
                });

                let outcome = tokio::select! {
                    () = child_token.cancelled() => {
                        Err("Cancelled by parent session".to_string())
                    }
                    result = Box::pin(inner.execute_sync(&agent_name_owned, &full_prompt, &args_inner)) => {
                        match result {
                            Ok(tool_result) => {
                                if tool_result.success {
                                    Ok(tool_result.output)
                                } else {
                                    Err(tool_result.error.unwrap_or_else(|| "Unknown error".into()))
                                }
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
                };

                let finished_at = chrono::Utc::now().to_rfc3339();
                let final_result = match outcome {
                    Ok(output) => BackgroundDelegateResult {
                        task_id: task_id_clone.clone(),
                        agent: agent_name_owned,
                        status: BackgroundTaskStatus::Completed,
                        output: Some(output),
                        error: None,
                        started_at,
                        finished_at: Some(finished_at),
                    },
                    Err(err) => {
                        let status = if err.contains("Cancelled") {
                            BackgroundTaskStatus::Cancelled
                        } else {
                            BackgroundTaskStatus::Failed
                        };
                        BackgroundDelegateResult {
                            task_id: task_id_clone.clone(),
                            agent: agent_name_owned,
                            status,
                            output: None,
                            error: Some(err),
                            started_at,
                            finished_at: Some(finished_at),
                        }
                    }
                };

                let result_path = results_dir.join(format!("{}.json", task_id_clone));
                if let Ok(bytes) = serde_json::to_vec_pretty(&final_result) {
                    let _ = tokio::fs::write(&result_path, &bytes).await;
                }
            },
        );

        Ok(ToolResult {
            success: true,
            output: format!(
                "Background task started for agent '{agent_name}'.\n\
                 task_id: {task_id}\n\
                 Use action='check_result' with task_id='{task_id}' to retrieve the result."
            ),
            error: None,
        })
    }

    async fn execute_parallel(
        &self,
        parallel_agents: &[serde_json::Value],
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| anyhow::anyhow!("Missing 'prompt' parameter for parallel execution"))?;

        if prompt.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("'prompt' parameter must not be empty".into()),
            });
        }

        let agent_names: Vec<String> = parallel_agents
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();

        if agent_names.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("'parallel' array must contain at least one agent name".into()),
            });
        }

        for name in &agent_names {
            let guard = self.agents.read();
            if !guard.contains_key(name) {
                let available: Vec<String> = guard.keys().cloned().collect();
                drop(guard);
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Unknown agent '{name}' in parallel list. Available: {}",
                        if available.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )),
                });
            }
        }

        let mut handles = Vec::with_capacity(agent_names.len());
        for agent_name in &agent_names {
            let agents = Arc::clone(&self.agents);
            let security = Arc::clone(&self.security);
            let fallback_credential = self.fallback_credential.clone();
            let provider_runtime_options = self.provider_runtime_options.clone();
            let depth = self.depth;
            let parent_tools = Arc::clone(&self.parent_tools);
            let multimodal_config = self.multimodal_config.clone();
            let delegate_config = self.delegate_config.clone();
            let workspace_root = Arc::clone(&self.workspace_root);
            let cancellation_token = self.cancellation_token.child_token();
            let agent_name = agent_name.clone();
            let prompt = prompt.to_string();
            let args_clone = args.clone();

            let (tx, rx) = tokio::sync::oneshot::channel();
            let inner_agent_name = agent_name.clone();
            crate::runtime::spawn_supervised("tools.delegate_subagent", async move {
                let inner = DelegateTool {
                    agents,
                    security,
                    fallback_credential,
                    provider_runtime_options,
                    depth,
                    parent_tools,
                    multimodal_config,
                    delegate_config,
                    workspace_root,
                    cancellation_token,
                };
                let result =
                    Box::pin(inner.execute_sync(&inner_agent_name, &prompt, &args_clone)).await;
                let _ = tx.send((inner_agent_name, result));
            });
            handles.push(rx);
        }

        let mut outputs = Vec::with_capacity(handles.len());
        let mut all_success = true;

        for handle in handles {
            match handle.await {
                Ok((agent_name, Ok(tool_result))) => {
                    if !tool_result.success {
                        all_success = false;
                    }
                    outputs.push(format!(
                        "--- {agent_name} (success={}) ---\n{}{}",
                        tool_result.success,
                        tool_result.output,
                        tool_result
                            .error
                            .map(|e| format!("\nError: {e}"))
                            .unwrap_or_default()
                    ));
                }
                Ok((agent_name, Err(e))) => {
                    all_success = false;
                    outputs.push(format!("--- {agent_name} (success=false) ---\nError: {e}"));
                }
                Err(_) => {
                    all_success = false;
                    outputs.push("--- [sub-agent panicked] ---".to_string());
                }
            }
        }

        Ok(ToolResult {
            success: all_success,
            output: format!(
                "[Parallel delegation: {} agents]\n\n{}",
                agent_names.len(),
                outputs.join("\n\n")
            ),
            error: if all_success {
                None
            } else {
                Some("One or more parallel agents failed".into())
            },
        })
    }

    async fn handle_check_result(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_id' parameter for check_result"))?;

        if let Err(e) = Self::validate_task_id(task_id) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
            });
        }

        let result_path = self.results_dir().join(format!("{task_id}.json"));
        if !result_path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("No result found for task_id '{task_id}'")),
            });
        }

        let content = tokio::fs::read_to_string(&result_path).await?;
        let result: BackgroundDelegateResult = serde_json::from_str(&content)?;

        Ok(ToolResult {
            success: result.status == BackgroundTaskStatus::Completed,
            output: serde_json::to_string_pretty(&result)?,
            error: if result.status == BackgroundTaskStatus::Completed {
                None
            } else {
                result.error
            },
        })
    }

    async fn handle_list_results(&self) -> anyhow::Result<ToolResult> {
        let results_dir = self.results_dir();
        if !results_dir.exists() {
            return Ok(ToolResult {
                success: true,
                output: "No background delegate results found.".into(),
                error: None,
            });
        }

        let mut entries = tokio::fs::read_dir(&results_dir).await?;
        let mut results = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(result) = serde_json::from_str::<BackgroundDelegateResult>(&content) {
                        results.push(json!({
                            "task_id": result.task_id,
                            "agent": result.agent,
                            "status": result.status,
                            "started_at": result.started_at,
                            "finished_at": result.finished_at,
                        }));
                    }
                }
            }
        }

        if results.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No background delegate results found.".into(),
                error: None,
            });
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&results)?,
            error: None,
        })
    }

    async fn handle_cancel_task(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_id' parameter for cancel_task"))?;

        if let Err(e) = Self::validate_task_id(task_id) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
            });
        }

        let result_path = self.results_dir().join(format!("{task_id}.json"));
        if !result_path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("No task found for task_id '{task_id}'")),
            });
        }

        let content = tokio::fs::read_to_string(&result_path).await?;
        let mut result: BackgroundDelegateResult = serde_json::from_str(&content)?;

        if result.status != BackgroundTaskStatus::Running {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Task '{task_id}' is not running (status: {:?})",
                    result.status
                )),
            });
        }

        result.status = BackgroundTaskStatus::Cancelled;
        result.error = Some("Cancelled by user request".into());
        result.finished_at = Some(chrono::Utc::now().to_rfc3339());
        let bytes = serde_json::to_vec_pretty(&result)?;
        tokio::fs::write(&result_path, &bytes).await?;

        Ok(ToolResult {
            success: true,
            output: format!("Task '{task_id}' cancellation requested."),
            error: None,
        })
    }

    pub fn cancel_all_background_tasks(&self) {
        self.cancellation_token.cancel();
    }

    fn build_enriched_system_prompt(
        &self,
        agent_config: &DelegateAgentConfig,
        sub_tools: &[Box<dyn Tool>],
        workspace_dir: &Path,
    ) -> Option<String> {

        let skills_dir = agent_config
            .skills_directory
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .map(|dir| workspace_dir.join(dir))
            .unwrap_or_else(|| crate::skills::skills_dir(workspace_dir));
        let skills = crate::skills::load_skills_from_directory(&skills_dir, false);

        let has_shell = sub_tools.iter().any(|t| t.name() == "shell");
        let shell_policy = if has_shell {
            "## Shell Policy\n\n\
             - Prefer non-destructive commands. Use `trash` over `rm` where possible.\n\
             - Do not run commands that exfiltrate data or modify system-critical paths.\n\
             - Avoid interactive commands that block on stdin.\n\
             - Quote paths that may contain spaces."
                .to_string()
        } else {
            String::new()
        };

        let ctx = PromptContext {
            workspace_dir,
            model_name: &agent_config.model,
            tools: sub_tools,
            allowed_tool_names: None,
            skills: &skills,
            skills_prompt_mode: crate::config::SkillsPromptInjectionMode::Full,
            identity_config: None,
            dispatcher_instructions: "",
            tool_descriptions: None,
            security_summary: None,
            autonomy_level: crate::security::AutonomyLevel::default(),
            global_directives: &[],
            coding_mode_label: None,
        };

        let builder = SystemPromptBuilder::default()
            .add_section(Box::new(crate::agent::prompt::ToolsSection))
            .add_section(Box::new(crate::agent::prompt::SafetySection))
            .add_section(Box::new(crate::agent::prompt::SkillsSection))
            .add_section(Box::new(crate::agent::prompt::WorkspaceSection))
            .add_section(Box::new(crate::agent::prompt::DateTimeSection));

        let mut enriched = builder.build(&ctx).unwrap_or_default();

        if !shell_policy.is_empty() {
            enriched.push_str(&shell_policy);
            enriched.push_str("\n\n");
        }

        if let Some(operator_prompt) = agent_config.system_prompt.as_ref() {
            enriched.push_str(operator_prompt);
            enriched.push('\n');
        }

        let trimmed = enriched.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    async fn execute_agentic(
        &self,
        agent_name: &str,
        agent_config: &DelegateAgentConfig,
        provider: &dyn Provider,
        full_prompt: &str,
        temperature: f64,
    ) -> anyhow::Result<ToolResult> {
        if agent_config.allowed_tools.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Agent '{agent_name}' has agentic=true but allowed_tools is empty"
                )),
            });
        }

        let allowed = agent_config
            .allowed_tools
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect::<std::collections::HashSet<_>>();

        let sub_tools: Vec<Box<dyn Tool>> = {
            let parent_tools = self.parent_tools.read();
            parent_tools
                .iter()
                .filter(|tool| allowed.contains(tool.name()))
                .filter(|tool| tool.name() != "delegate")
                .map(|tool| Box::new(ToolArcRef::new(tool.clone())) as Box<dyn Tool>)
                .collect()
        };

        if sub_tools.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Agent '{agent_name}' has no executable tools after filtering allowlist ({})",
                    agent_config.allowed_tools.join(", ")
                )),
            });
        }

        let ws_root = self.workspace_snapshot();
        let enriched_system_prompt =
            self.build_enriched_system_prompt(agent_config, &sub_tools, ws_root.as_path());

        let mut history = Vec::new();
        if let Some(system_prompt) = enriched_system_prompt.as_ref() {
            history.push(ChatMessage::system(system_prompt.clone()));
        }
        history.push(ChatMessage::user(full_prompt.to_string()));

        let noop_observer = NoopObserver;

        let agentic_timeout_secs = agent_config
            .agentic_timeout_secs
            .unwrap_or(self.delegate_config.agentic_timeout_secs);

        use crate::agent::SubagentChunkKind;
        use crate::agent::loop_::{DraftEvent, take_parent_draft_channel};
        let (sub_tx, mut sub_rx) =
            tokio::sync::mpsc::channel::<DraftEvent>(64);
        let parent_for_bridge = take_parent_draft_channel();
        let agent_id_for_bridge = agent_name.to_string();
        let task_id_for_bridge = agent_name.to_string();
        let bridge_handle = if let Some(parent) = parent_for_bridge.clone() {
            Some(
                crate::runtime::spawn_supervised(
                    "delegate.subagent_bridge",
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
                                    let preview =
                                        output.chars().take(160).collect::<String>();
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
                                    break;
                                }
                            }
                        }
                    },
                )
                .into_inner(),
            )
        } else {
            drop(sub_rx);
            None
        };
        let on_delta_for_loop = if bridge_handle.is_some() {
            Some(sub_tx)
        } else {
            None
        };

        let pacing_default = crate::config::PacingConfig::default();
        let delegated_policy = crate::agent::loop_::policy::PolicyBundle::delegated(
            provider,
            &sub_tools,
            &noop_observer,
            &agent_config.provider,
            &agent_config.model,
            &self.multimodal_config,
            &pacing_default,
            &[],
            &[],
        )
        .with_temperature(temperature)
        .with_max_iterations(agent_config.max_iterations)
        .with_on_delta(on_delta_for_loop);

        let result = tokio::time::timeout(
            Duration::from_secs(agentic_timeout_secs),
            crate::agent::loop_::unified::UnifiedLoop::new(delegated_policy).run(&mut history),
        )
        .await;
        if let Some(h) = bridge_handle {
            h.abort();
        }

        match result {
            Ok(Ok(response)) => {
                let rendered = if response.trim().is_empty() {
                    "[Empty response]".to_string()
                } else {
                    response
                };

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "[Agent '{agent_name}' ({provider}/{model}, agentic)]\n{rendered}",
                        provider = agent_config.provider,
                        model = agent_config.model
                    ),
                    error: None,
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Agent '{agent_name}' failed: {e}")),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Agent '{agent_name}' timed out after {agentic_timeout_secs}s"
                )),
            }),
        }
    }
}

struct ToolArcRef {
    inner: Arc<dyn Tool>,
}

impl ToolArcRef {
    fn new(inner: Arc<dyn Tool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool for ToolArcRef {
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

struct NoopObserver;

impl Observer for NoopObserver {
    fn record_event(&self, _event: &ObserverEvent) {}

    fn record_metric(&self, _metric: &ObserverMetric) {}

    fn name(&self) -> &str {
        "noop"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
