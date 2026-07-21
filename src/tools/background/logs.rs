// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::json;

use super::super::traits::{Tool, ToolResult};

const DEFAULT_TAIL_LINES: usize = 120;
const MAX_LOG_OUTPUT_BYTES: usize = 32_768;

pub struct BackgroundLogsTool;

impl BackgroundLogsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BackgroundLogsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BackgroundLogsTool {
    fn name(&self) -> &str {
        "background_logs"
    }

    fn description(&self) -> &str {
        "Read the buffered output of a background shell started with shell \
         background:true. Returns the most recent lines (stderr lines are \
         prefixed with [stderr]). Use background_status to list shells."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The bg-<id> returned when the background shell was started"
                },
                "tail_lines": {
                    "type": "integer",
                    "description": "How many trailing lines to return. Default 120, max 2000",
                    "default": 120
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

        let tail_lines = args
            .get("tail_lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_TAIL_LINES);

        let Some((snapshot, text)) = super::registry::logs_for(id, tail_lines) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "No background shell '{id}' in this session. Use background_status to list live shells; exited shells are evicted after a while."
                )),
            });
        };

        let body = crate::util::truncate_head_tail(&text, MAX_LOG_OUTPUT_BYTES, 25)
            .unwrap_or(text);
        let status_line = if snapshot.running {
            format!("[{} running for {}s]", snapshot.id, snapshot.elapsed_secs)
        } else {
            format!(
                "[{} exited with code {} after {}s]",
                snapshot.id,
                snapshot
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                snapshot.elapsed_secs
            )
        };
        let dropped_note = if snapshot.dropped_lines > 0 {
            format!(
                "\n[{} oldest lines dropped from the ring buffer]",
                snapshot.dropped_lines
            )
        } else {
            String::new()
        };

        Ok(ToolResult {
            success: true,
            output: format!("{status_line}{dropped_note}\n{body}"),
            error: None,
        })
    }
}
