// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::config::{DelegateAgentConfig, SwarmConfig, SwarmStrategy};
use crate::providers::{self, Provider};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const SWARM_AGENT_TIMEOUT_SECS: u64 = 120;

pub struct SwarmTool {
    swarms: Arc<HashMap<String, SwarmConfig>>,
    agents: Arc<HashMap<String, DelegateAgentConfig>>,
    security: Arc<SecurityPolicy>,
    fallback_credential: Option<String>,
    provider_runtime_options: providers::ProviderRuntimeOptions,
}

impl SwarmTool {
    pub fn new(
        swarms: HashMap<String, SwarmConfig>,
        agents: HashMap<String, DelegateAgentConfig>,
        fallback_credential: Option<String>,
        security: Arc<SecurityPolicy>,
        provider_runtime_options: providers::ProviderRuntimeOptions,
    ) -> Self {
        Self {
            swarms: Arc::new(swarms),
            agents: Arc::new(agents),
            security,
            fallback_credential,
            provider_runtime_options,
        }
    }

    fn create_provider_for_agent(
        &self,
        agent_config: &DelegateAgentConfig,
        agent_name: &str,
    ) -> Result<Box<dyn Provider>, ToolResult> {
        let credential = agent_config
            .api_key
            .clone()
            .or_else(|| self.fallback_credential.clone());

        providers::create_provider_with_options(
            &agent_config.provider,
            credential.as_deref(),
            &self.provider_runtime_options,
        )
        .map_err(|e| ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!(
                "Failed to create provider '{}' for agent '{agent_name}': {e}",
                agent_config.provider
            )),
        })
    }

    async fn call_agent(
        &self,
        agent_name: &str,
        agent_config: &DelegateAgentConfig,
        prompt: &str,
        timeout_secs: u64,
    ) -> Result<String, String> {
        let provider = self
            .create_provider_for_agent(agent_config, agent_name)
            .map_err(|r| r.error.unwrap_or_default())?;

        let temperature = agent_config.temperature.unwrap_or(0.7);

        let result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            provider.chat_with_system(
                agent_config.system_prompt.as_deref(),
                prompt,
                &agent_config.model,
                temperature,
            ),
        )
        .await;

        match result {
            Ok(Ok(response)) => {
                if response.trim().is_empty() {
                    Ok("[Empty response]".to_string())
                } else {
                    Ok(response)
                }
            }
            Ok(Err(e)) => Err(format!("Agent '{agent_name}' failed: {e}")),
            Err(_) => Err(format!(
                "Agent '{agent_name}' timed out after {timeout_secs}s"
            )),
        }
    }

    async fn execute_sequential(
        &self,
        swarm_config: &SwarmConfig,
        prompt: &str,
        context: &str,
    ) -> anyhow::Result<ToolResult> {
        let mut current_input = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        let per_agent_timeout = swarm_config.timeout_secs / swarm_config.agents.len().max(1) as u64;
        let mut results = Vec::new();

        for (i, agent_name) in swarm_config.agents.iter().enumerate() {
            let agent_config = match self.agents.get(agent_name) {
                Some(cfg) => cfg,
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Swarm references unknown agent '{agent_name}'")),
                    });
                }
            };

            let agent_prompt = if i == 0 {
                current_input.clone()
            } else {
                format!("[Previous agent output]\n{current_input}\n\n[Original task]\n{prompt}")
            };

            match self
                .call_agent(agent_name, agent_config, &agent_prompt, per_agent_timeout)
                .await
            {
                Ok(output) => {
                    results.push(format!(
                        "[{agent_name} ({}/{})] {output}",
                        agent_config.provider, agent_config.model
                    ));
                    current_input = output;
                }
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: results.join("\n\n"),
                        error: Some(e),
                    });
                }
            }
        }

        Ok(ToolResult {
            success: true,
            output: format!(
                "[Swarm sequential — {} agents]\n\n{}",
                swarm_config.agents.len(),
                results.join("\n\n")
            ),
            error: None,
        })
    }

    async fn execute_parallel(
        &self,
        swarm_config: &SwarmConfig,
        prompt: &str,
        context: &str,
    ) -> anyhow::Result<ToolResult> {
        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        let mut join_set = tokio::task::JoinSet::new();

        for agent_name in &swarm_config.agents {
            let agent_config = match self.agents.get(agent_name) {
                Some(cfg) => cfg.clone(),
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Swarm references unknown agent '{agent_name}'")),
                    });
                }
            };

            let credential = agent_config
                .api_key
                .clone()
                .or_else(|| self.fallback_credential.clone());

            let provider = match providers::create_provider_with_options(
                &agent_config.provider,
                credential.as_deref(),
                &self.provider_runtime_options,
            ) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Failed to create provider for agent '{agent_name}': {e}"
                        )),
                    });
                }
            };

            let name = agent_name.clone();
            let prompt_clone = full_prompt.clone();
            let timeout = swarm_config.timeout_secs;
            let model = agent_config.model.clone();
            let temperature = agent_config.temperature.unwrap_or(0.7);
            let system_prompt = agent_config.system_prompt.clone();
            let provider_name = agent_config.provider.clone();

            join_set.spawn(async move {
                let result = tokio::time::timeout(
                    Duration::from_secs(timeout),
                    provider.chat_with_system(
                        system_prompt.as_deref(),
                        &prompt_clone,
                        &model,
                        temperature,
                    ),
                )
                .await;

                let output = match result {
                    Ok(Ok(text)) => {
                        if text.trim().is_empty() {
                            "[Empty response]".to_string()
                        } else {
                            text
                        }
                    }
                    Ok(Err(e)) => format!("[Error] {e}"),
                    Err(_) => format!("[Timed out after {timeout}s]"),
                };

                (name, provider_name, model, output)
            });
        }

        let mut results = Vec::new();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((name, provider_name, model, output)) => {
                    results.push(format!("[{name} ({provider_name}/{model})]\n{output}"));
                }
                Err(e) => {
                    results.push(format!("[join error] {e}"));
                }
            }
        }

        Ok(ToolResult {
            success: true,
            output: format!(
                "[Swarm parallel — {} agents]\n\n{}",
                swarm_config.agents.len(),
                results.join("\n\n---\n\n")
            ),
            error: None,
        })
    }

    async fn execute_router(
        &self,
        swarm_config: &SwarmConfig,
        prompt: &str,
        context: &str,
    ) -> anyhow::Result<ToolResult> {
        if swarm_config.agents.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Router swarm has no agents to choose from".into()),
            });
        }

        let agent_descriptions: Vec<String> = swarm_config
            .agents
            .iter()
            .filter_map(|name| {
                self.agents.get(name).map(|cfg| {
                    let desc = cfg
                        .system_prompt
                        .as_deref()
                        .unwrap_or("General purpose agent");
                    format!(
                        "- {name}: {desc} (provider: {}, model: {})",
                        cfg.provider, cfg.model
                    )
                })
            })
            .collect();

        let first_agent_name = &swarm_config.agents[0];
        let first_agent_config = match self.agents.get(first_agent_name) {
            Some(cfg) => cfg,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Swarm references unknown agent '{first_agent_name}'"
                    )),
                });
            }
        };

        let router_provider = self
            .create_provider_for_agent(first_agent_config, first_agent_name)
            .map_err(|r| anyhow::anyhow!(r.error.unwrap_or_default()))?;

        let base_router_prompt = swarm_config
            .router_prompt
            .as_deref()
            .unwrap_or("Pick the single best agent for this task.");

        let routing_prompt = format!(
            "{base_router_prompt}\n\nAvailable agents:\n{}\n\nUser task: {prompt}\n\n\
             Respond with ONLY the agent name, nothing else.",
            agent_descriptions.join("\n")
        );

        let chosen = tokio::time::timeout(
            Duration::from_secs(SWARM_AGENT_TIMEOUT_SECS),
            router_provider.chat_with_system(
                Some("You are a routing assistant. Respond with only the agent name."),
                &routing_prompt,
                &first_agent_config.model,
                0.0,
            ),
        )
        .await;

        let chosen_name = match chosen {
            Ok(Ok(name)) => name.trim().to_string(),
            Ok(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Router LLM call failed: {e}")),
                });
            }
            Err(_) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Router LLM call timed out".into()),
                });
            }
        };

        let matched_name = swarm_config
            .agents
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&chosen_name))
            .cloned()
            .unwrap_or_else(|| swarm_config.agents[0].clone());

        let agent_config = match self.agents.get(&matched_name) {
            Some(cfg) => cfg,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Router selected unknown agent '{matched_name}'")),
                });
            }
        };

        let full_prompt = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("[Context]\n{context}\n\n[Task]\n{prompt}")
        };

        match self
            .call_agent(
                &matched_name,
                agent_config,
                &full_prompt,
                swarm_config.timeout_secs,
            )
            .await
        {
            Ok(output) => Ok(ToolResult {
                success: true,
                output: format!(
                    "[Swarm router — selected '{matched_name}' ({}/{})]\n{output}",
                    agent_config.provider, agent_config.model
                ),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
            }),
        }
    }
}

#[async_trait]
impl Tool for SwarmTool {
    fn name(&self) -> &str {
        "swarm"
    }

    fn description(&self) -> &str {
        "Orchestrate a swarm of agents to collaboratively handle a task. Supports sequential \
         (pipeline), parallel (fan-out/fan-in), and router (LLM-selected) strategies."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let swarm_names: Vec<&str> = self.swarms.keys().map(String::as_str).collect();
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "swarm": {
                    "type": "string",
                    "minLength": 1,
                    "description": format!(
                        "Name of the swarm to invoke. Available: {}",
                        if swarm_names.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            swarm_names.join(", ")
                        }
                    )
                },
                "prompt": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The task/prompt to send to the swarm"
                },
                "context": {
                    "type": "string",
                    "description": "Optional context to include (e.g. relevant code, prior findings)"
                }
            },
            "required": ["swarm", "prompt"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let swarm_name = args
            .get("swarm")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .ok_or_else(|| anyhow::anyhow!("Missing 'swarm' parameter"))?;

        if swarm_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("'swarm' parameter must not be empty".into()),
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

        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");

        let swarm_config = match self.swarms.get(swarm_name) {
            Some(cfg) => cfg,
            None => {
                let available: Vec<&str> = self.swarms.keys().map(String::as_str).collect();
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Unknown swarm '{swarm_name}'. Available swarms: {}",
                        if available.is_empty() {
                            "(none configured)".to_string()
                        } else {
                            available.join(", ")
                        }
                    )),
                });
            }
        };

        if swarm_config.agents.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Swarm '{swarm_name}' has no agents configured")),
            });
        }

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "swarm")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        match swarm_config.strategy {
            SwarmStrategy::Sequential => {
                self.execute_sequential(swarm_config, prompt, context).await
            }
            SwarmStrategy::Parallel => self.execute_parallel(swarm_config, prompt, context).await,
            SwarmStrategy::Router => self.execute_router(swarm_config, prompt, context).await,
        }
    }
}
