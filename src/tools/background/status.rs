// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::json;

use super::super::traits::{Tool, ToolResult};

pub struct BackgroundStatusTool;

impl BackgroundStatusTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BackgroundStatusTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BackgroundStatusTool {
    fn name(&self) -> &str {
        "background_status"
    }

    fn description(&self) -> &str {
        "List this session's background shells (started with shell background:true) \
         with their running state, exit code, elapsed time and buffered output size. \
         Use background_logs to read a shell's output and background_kill to stop one."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Optional bg-<id> to show a single background shell"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let filter_id = args.get("id").and_then(|v| v.as_str()).map(str::trim);
        let snapshots = super::registry::status_snapshot();
        let rows: Vec<serde_json::Value> = snapshots
            .iter()
            .filter(|s| filter_id.map_or(true, |id| s.id == id))
            .map(|s| {
                json!({
                    "id": s.id,
                    "command": s.command,
                    "running": s.running,
                    "exit_code": s.exit_code,
                    "elapsed_secs": s.elapsed_secs,
                    "buffered_lines": s.buffered_lines,
                    "dropped_lines": s.dropped_lines,
                })
            })
            .collect();

        if rows.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: match filter_id {
                    Some(id) => format!(
                        "No background shell '{id}' in this session (it may have exited long ago and been evicted)."
                    ),
                    None => "No background shells in this session.".to_string(),
                },
                error: None,
            });
        }

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&json!({ "shells": rows }))
                .unwrap_or_default(),
            error: None,
        })
    }
}
