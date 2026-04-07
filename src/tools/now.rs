// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use chrono::{Local, Utc};
use serde_json::json;

/// Returns the current date and time, giving the LLM temporal awareness.
pub struct NowTool;

impl NowTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for NowTool {
    fn name(&self) -> &str {
        "now"
    }

    fn description(&self) -> &str {
        "Returns the current date and time in RFC 3339 format. \
         Use timezone 'utc' for UTC or 'local' for the system's local time."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "timezone": {
                    "type": "string",
                    "description": "Timezone to use: 'utc' or 'local' (default: 'local')",
                    "enum": ["utc", "local"],
                    "default": "local"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let tz = args
            .get("timezone")
            .and_then(|v| v.as_str())
            .unwrap_or("local");

        let (formatted, tz_label) = match tz {
            "utc" => (Utc::now().to_rfc3339(), "UTC"),
            _ => (Local::now().to_rfc3339(), "Local"),
        };

        Ok(ToolResult {
            success: true,
            output: format!("{formatted} ({tz_label})"),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn now_utc() {
        let tool = NowTool::new();
        let result = tool.execute(json!({"timezone": "utc"})).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("UTC"));
        assert!(result.output.contains("T"));
    }

    #[tokio::test]
    async fn now_local() {
        let tool = NowTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("Local"));
    }

    #[test]
    fn schema_is_valid() {
        let tool = NowTool::new();
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }
}
