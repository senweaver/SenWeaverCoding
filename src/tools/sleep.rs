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
