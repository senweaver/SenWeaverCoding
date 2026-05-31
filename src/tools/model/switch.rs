// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::agent::loop_::get_model_switch_state;
use crate::providers;
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct ModelSwitchTool {
    security: Arc<SecurityPolicy>,
}

impl ModelSwitchTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for ModelSwitchTool {
    fn name(&self) -> &str {
        "model_switch"
    }

    fn description(&self) -> &str {
        "Switch the AI model at runtime. Use 'get' to see current model, 'list_providers' to see available providers, 'list_models' to see models for a provider, or 'set' to switch to a different model. The switch takes effect immediately for the current conversation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get", "set", "list_providers", "list_models"],
                    "description": "Action to perform: get current model, set a new model, list available providers, or list models for a provider"
                },
                "provider": {
                    "type": "string",
                    "description": "Provider name (e.g., 'openai', 'anthropic', 'groq', 'ollama'). Required for 'set' and 'list_models' actions."
                },
                "model": {
                    "type": "string",
                    "description": "Model ID (must be a model already added in Provider settings). Required for 'set' action."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("get");

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "model_switch")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        match action {
            "get" => self.handle_get(),
            "set" => self.handle_set(&args),
            "list_providers" => self.handle_list_providers(),
            "list_models" => self.handle_list_models(&args),
            _ => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action: {}. Valid actions: get, set, list_providers, list_models",
                    action
                )),
            }),
        }
    }
}

impl ModelSwitchTool {
    fn handle_get(&self) -> anyhow::Result<ToolResult> {
        let switch_state = get_model_switch_state();
        let pending = switch_state.lock().clone();

        let (current_provider, current_model) = match crate::services::try_get_services() {
            Some(svc) => {
                let cfg = svc.config();
                let provider = cfg
                    .default_provider
                    .clone()
                    .unwrap_or_else(|| "openrouter".to_string());
                let model = cfg.default_model.clone().unwrap_or_default();
                (provider, model)
            }
            None => (String::new(), String::new()),
        };

        let resolved_current_model = if current_model.is_empty() {
            crate::services::try_get_services()
                .and_then(|svc| {
                    let cfg = svc.config();
                    providers::resolve_default_model(&cfg).ok()
                })
                .unwrap_or_default()
        } else {
            current_model
        };

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&json!({
                "current_provider": current_provider,
                "current_model": resolved_current_model,
                "pending_switch": pending,
                "note": "current_provider/current_model reflect the active runtime model. Use 'set' to switch."
            }))?,
            error: None,
        })
    }

    fn handle_set(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let provider = args.get("provider").and_then(|v| v.as_str());

        let provider = match provider {
            Some(p) => p,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing 'provider' parameter for 'set' action".to_string()),
                });
            }
        };

        let model = args.get("model").and_then(|v| v.as_str());

        let model = match model {
            Some(m) => m,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing 'model' parameter for 'set' action".to_string()),
                });
            }
        };

        let known_providers = providers::list_providers();
        let provider_valid = known_providers.iter().any(|p| {
            p.name.eq_ignore_ascii_case(provider)
                || p.aliases.iter().any(|a| a.eq_ignore_ascii_case(provider))
        });

        if !provider_valid {
            return Ok(ToolResult {
                success: false,
                output: serde_json::to_string_pretty(&json!({
                    "available_providers": known_providers.iter().map(|p| p.name).collect::<Vec<_>>()
                }))?,
                error: Some(format!(
                    "Unknown provider: {}. Use 'list_providers' to see available options.",
                    provider
                )),
            });
        }

        if let Some(svc) = crate::services::try_get_services() {
            let cfg = svc.config();
            let registered = configured_models_for_provider(&cfg, provider);
            if registered.is_empty() {
                return Ok(ToolResult {
                    success: false,
                    output: serde_json::to_string_pretty(&json!({
                        "provider": provider,
                        "model": model,
                        "registered_models": Vec::<String>::new(),
                    }))?,
                    error: Some(format!(
                        "鏈坊鍔犳ā鍨?/ no_model_configured: provider '{provider}' has no models in Provider settings."
                    )),
                });
            }
            let model_registered = registered
                .iter()
                .any(|m| m.eq_ignore_ascii_case(model));
            if !model_registered {
                return Ok(ToolResult {
                    success: false,
                    output: serde_json::to_string_pretty(&json!({
                        "provider": provider,
                        "model": model,
                        "registered_models": registered,
                    }))?,
                    error: Some(format!(
                        "鏈坊鍔犳ā鍨?/ model_not_registered: model '{model}' is not in Provider settings for '{provider}'. Add it first or pick from registered_models."
                    )),
                });
            }
        } else {
            tracing::warn!(
                target = "model_switch",
                "services container not initialized; skipping registered-model validation"
            );
        }

        let switch_state = get_model_switch_state();
        *switch_state.lock() = Some((provider.to_string(), model.to_string()));

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&json!({
                "message": "Model switch requested",
                "provider": provider,
                "model": model,
                "note": "The agent will switch to this model on the next turn. Use 'get' to check pending switch."
            }))?,
            error: None,
        })
    }

    fn handle_list_providers(&self) -> anyhow::Result<ToolResult> {
        let providers_list = providers::list_providers();

        let providers: Vec<serde_json::Value> = providers_list
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "display_name": p.display_name,
                    "aliases": p.aliases,
                    "local": p.local
                })
            })
            .collect();

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&json!({
                "providers": providers,
                "count": providers.len(),
                "example": "Use action 'set' with provider and model to switch"
            }))?,
            error: None,
        })
    }

    fn handle_list_models(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let provider = args.get("provider").and_then(|v| v.as_str());

        let provider = match provider {
            Some(p) => p,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "Missing 'provider' parameter for 'list_models' action".to_string(),
                    ),
                });
            }
        };

        let models = match crate::services::try_get_services() {
            Some(svc) => {
                let cfg = svc.config();
                configured_models_for_provider(&cfg, provider)
            }
            None => Vec::new(),
        };

        if models.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: serde_json::to_string_pretty(&json!({
                    "provider": provider,
                    "models": Vec::<String>::new(),
                    "note": "鏈坊鍔犳ā鍨?/ no_model_configured: please add models in Provider settings"
                }))?,
                error: None,
            });
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&json!({
                "provider": provider,
                "models": models,
                "example": "Use action 'set' with this provider and a model ID to switch"
            }))?,
            error: None,
        })
    }
}

fn configured_models_for_provider(
    config: &crate::config::Config,
    provider: &str,
) -> Vec<String> {
    let trimmed = provider.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Some(profile) = config.model_providers.get(trimmed) {
        return providers::profile_model_names(profile);
    }
    for (pid, profile) in config.model_providers.iter() {
        if pid.eq_ignore_ascii_case(trimmed)
            || profile
                .preset_id
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case(trimmed))
                .unwrap_or(false)
        {
            return providers::profile_model_names(profile);
        }
    }
    Vec::new()
}
