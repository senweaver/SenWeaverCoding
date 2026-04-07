// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

pub struct SleepTool;

impl SleepTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SleepTool {
    fn default() -> Self {
        Self::new()
    }
}

const MAX_SLEEP_SECS: f64 = 300.0;

fn clamp_sleep_seconds(raw: f64) -> f64 {
    raw.clamp(0.0, MAX_SLEEP_SECS)
}

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "sleep"
    }

    fn description(&self) -> &str {
        "Wait for a specified duration without holding a shell process. Useful for polling or waiting between operations."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "seconds": {
                    "type": "number",
                    "description": "Duration to sleep in seconds (max 300)",
                },
            },
            "required": ["seconds"],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let Some(raw) = args.get("seconds").and_then(|v| v.as_f64()) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing or invalid required parameter: seconds".to_string()),
            });
        };

        let seconds = clamp_sleep_seconds(raw);
        tokio::time::sleep(Duration::from_secs_f64(seconds)).await;

        Ok(ToolResult {
            success: true,
            output: format!("Slept for {seconds} seconds"),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn sleep_zero_succeeds() {
        let tool = SleepTool::new();
        let result = tool.execute(json!({ "seconds": 0 })).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "Slept for 0 seconds");
    }

    #[test]
    fn clamp_limits_range() {
        assert_eq!(super::clamp_sleep_seconds(9999.0), 300.0);
        assert_eq!(super::clamp_sleep_seconds(-1.0), 0.0);
        assert_eq!(super::clamp_sleep_seconds(1.5), 1.5);
    }

    #[tokio::test]
    async fn sleep_rejects_missing_seconds() {
        let tool = SleepTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn spec_requires_seconds() {
        let tool = SleepTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, "sleep");
        assert_eq!(spec.parameters["required"], json!(["seconds"]));
    }
}
