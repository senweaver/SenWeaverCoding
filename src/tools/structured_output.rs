// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;

/// Structured output tool for SDK/non-interactive workflows.
///
/// Accepts a JSON object conforming to a dynamic output schema and validates it.
/// Designed to be called exactly once per session as the final tool call.
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
        // Enforce single-call semantics
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

        // Basic schema validation if a schema is provided
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn name_matches() {
        assert_eq!(StructuredOutputTool::new(None).name(), "structured_output");
    }

    #[tokio::test]
    async fn accepts_valid_data() {
        let tool = StructuredOutputTool::new(None);
        let result = tool
            .execute(json!({"data": {"key": "value"}}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("key"));
    }

    #[tokio::test]
    async fn rejects_second_call() {
        let tool = StructuredOutputTool::new(None);
        let r1 = tool.execute(json!({"data": {"a": 1}})).await.unwrap();
        assert!(r1.success);
        let r2 = tool.execute(json!({"data": {"b": 2}})).await.unwrap();
        assert!(!r2.success);
        assert!(r2.error.unwrap().contains("once"));
    }

    #[tokio::test]
    async fn validates_required_fields() {
        let schema = json!({"required": ["name", "value"]});
        let tool = StructuredOutputTool::new(Some(schema));
        let result = tool
            .execute(json!({"data": {"name": "test"}}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("value"));
    }

    #[tokio::test]
    async fn missing_data_returns_error() {
        let tool = StructuredOutputTool::new(None);
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }
}
