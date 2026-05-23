// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::providers::traits::{ChatMessage, Provider, StructuredResponse};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;

pub struct StructuredOutputTool {
    schema: Option<serde_json::Value>,
    called: Arc<RwLock<bool>>,
}

impl StructuredOutputTool {
    pub fn new(schema: Option<serde_json::Value>) -> Self {
        Self {
            schema,
            called: Arc::new(RwLock::new(false)),
        }
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = Some(schema);
        self
    }

    pub fn schema(&self) -> Option<&serde_json::Value> {
        self.schema.as_ref()
    }
}

pub async fn request_structured_output(
    provider: &dyn Provider,
    messages: &[ChatMessage],
    schema: &serde_json::Value,
    model: &str,
    temperature: f64,
) -> anyhow::Result<StructuredResponse> {
    provider
        .chat_structured(messages, schema, model, temperature)
        .await
}

#[async_trait]
impl Tool for StructuredOutputTool {
    fn name(&self) -> &str {
        "structured_output"
    }

    fn description(&self) -> &str {
        "Provide structured JSON output for SDK or non-interactive workflows. This should be called exactly once as the final step to return validated structured data."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "object",
                    "description": "The structured JSON data to output. Must conform to the session's output schema if one is defined."
                }
            },
            "required": ["data"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        {
            let mut called = self.called.write();
            if *called {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("structured_output can only be called once per session".into()),
                });
            }
            *called = true;
        }

        let data = args
            .get("data")
            .ok_or_else(|| anyhow::anyhow!("Missing 'data' parameter"))?;

        if let Some(schema) = &self.schema {
            if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                for field in required {
                    if let Some(field_name) = field.as_str() {
                        if data.get(field_name).is_none() {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "Missing required field '{}' in output data",
                                    field_name
                                )),
                            });
                        }
                    }
                }
            }
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()),
            error: None,
        })
    }
}
