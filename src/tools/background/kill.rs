// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::json;

use super::super::traits::{Tool, ToolResult};

pub struct BackgroundKillTool;

impl BackgroundKillTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BackgroundKillTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BackgroundKillTool {
    fn name(&self) -> &str {
        "background_kill"
    }

    fn description(&self) -> &str {
        "Terminate a background shell started with shell background:true. \
         The process tree is killed. Use background_status to list shells \
         and background_logs to inspect output before/after killing."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The bg-<id> of the background shell to kill"
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'id' parameter"))?;

        let visible = super::registry::status_snapshot()
            .into_iter()
            .any(|s| s.id == id && s.running);
        if !visible {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "No running background shell '{id}' in this session; use background_status to list shells."
                )),
            });
        }

        if super::registry::kill(id) {
            Ok(ToolResult {
                success: true,
                output: format!("Kill signal sent to background shell '{id}'. Poll background_status to confirm exit."),
                error: None,
            })
        } else {
            Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Background shell '{id}' exists but its kill channel was already consumed (it may be exiting)."
                )),
            })
        }
    }
}
