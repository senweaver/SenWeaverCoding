// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Lightweight LLM task tool for structured JSON-only sub-calls.
//!
//! Runs a single prompt through an LLM provider with no tool access and
//! optionally validates the response against a caller-supplied JSON Schema.
//! Ideal for structured data extraction in workflows.

use super::traits::{Tool, ToolResult};
use crate::providers::{self, Provider};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct LlmTaskTool {
    security: Arc<SecurityPolicy>,

    default_provider: String,

    default_model: String,

    default_temperature: f64,

    api_key: Option<String>,

    provider_runtime_options: providers::ProviderRuntimeOptions,
}

impl LlmTaskTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        default_provider: String,
        default_model: String,
        default_temperature: f64,
        api_key: Option<String>,
        provider_runtime_options: providers::ProviderRuntimeOptions,
    ) -> Self {
        Self {
            security,
            default_provider,
            default_model,
            default_temperature,
            api_key,
            provider_runtime_options,
        }
    }
}

#[async_trait]
impl Tool for LlmTaskTool {
    fn name(&self) -> &str {
        "llm_task"
    }

    fn description(&self) -> &str {
        "Run a prompt through an LLM with no tool access and return the response. \
         Optionally validates the output against a JSON Schema. Ideal for structured \
         data extraction, classification, summarization, and transformation tasks."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The prompt to send to the LLM."
                },
                "schema": {
                    "type": "object",
                    "description": "Optional JSON Schema to validate the LLM response against. \
                                    When provided, the LLM is instructed to return valid JSON \
                                    matching this schema."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override (must be a model already added in Provider settings). \
                                    Defaults to the configured default model."
                },
                "temperature": {
                    "type": "number",
                    "description": "Optional temperature override (0.0-2.0). \
                                    Defaults to the configured default temperature."
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "llm_task")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let prompt = match args.get("prompt").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => p,
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing or empty required parameter: prompt".to_string()),
                });
            }
        };

        let schema = args.get("schema").and_then(|v| v.as_object());
        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model);
        let temperature = args
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.default_temperature);

        let effective_prompt = if let Some(schema_obj) = schema {
            let schema_json =
                serde_json::to_string_pretty(&serde_json::Value::Object(schema_obj.clone()))
                    .unwrap_or_else(|_| "{}".to_string());
            format!(
                "{prompt}\n\n\
                 IMPORTANT: You MUST respond with valid JSON that conforms to this schema:\n\
                 ```json\n{schema_json}\n```\n\
                 Respond ONLY with the JSON object, no explanation or markdown."
            )
        } else {
            prompt.to_string()
        };

        let api_key_ref = self.api_key.as_deref();
        let provider: Box<dyn Provider> = match providers::create_provider_with_options(
            &self.default_provider,
            api_key_ref,
            &self.provider_runtime_options,
        ) {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to create provider: {e}")),
                });
            }
        };

        let response = match provider
            .simple_chat(&effective_prompt, model, temperature)
            .await
        {
            Ok(text) => text,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("LLM call failed: {e}")),
                });
            }
        };

        if let Some(schema_obj) = schema {
            let schema_value = serde_json::Value::Object(schema_obj.clone());
            match validate_json_response(&response, &schema_value) {
                Ok(validated_json) => Ok(ToolResult {
                    success: true,
                    output: validated_json,
                    error: None,
                }),
                Err(validation_error) => Ok(ToolResult {
                    success: false,
                    output: response,
                    error: Some(format!("Schema validation failed: {validation_error}")),
                }),
            }
        } else {
            Ok(ToolResult {
                success: true,
                output: response,
                error: None,
            })
        }
    }
}

fn validate_json_response(response: &str, schema: &serde_json::Value) -> Result<String, String> {

    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        let inner = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        inner
    } else {
        trimmed
    };

    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON: {e}"))?;

    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        for req in required {
            if let Some(field_name) = req.as_str() {
                if parsed.get(field_name).is_none() {
                    return Err(format!("Missing required field: {field_name}"));
                }
            }
        }
    }

    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        for (prop_name, prop_schema) in properties {
            if let Some(value) = parsed.get(prop_name) {
                if let Some(expected_type) = prop_schema.get("type").and_then(|t| t.as_str()) {
                    if !type_matches(value, expected_type) {
                        return Err(format!(
                            "Field '{prop_name}' has wrong type: expected {expected_type}, \
                             got {}",
                            json_type_name(value)
                        ));
                    }
                }
            }
        }
    }

    serde_json::to_string(&parsed).map_err(|e| format!("JSON serialization error: {e}"))
}

fn type_matches(value: &serde_json::Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
