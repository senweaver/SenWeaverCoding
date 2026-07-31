// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

const DEFAULT_MAX_CHARS: usize = 30_000;

pub struct ToolResultExpandTool;

impl Default for ToolResultExpandTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolResultExpandTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ToolResultExpandTool {
    fn name(&self) -> &str {
        "tool_result_expand"
    }

    fn description(&self) -> &str {
        "Retrieve the full text of an earlier tool result that was truncated or evicted \
         during context compaction. Truncation markers in the transcript include a blob id \
         (e.g. 'archived as blob a1b2c3d4e5f60718'); pass that id here to read the archived \
         output, optionally paged with offset_chars/max_chars."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Blob id from a truncation/eviction marker (16 hex chars)"
                },
                "offset_chars": {
                    "type": "integer",
                    "description": "Character offset to start reading from (default 0)"
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return (default 30000)"
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
            .unwrap_or("");
        if id.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'id' parameter".into()),
            });
        }
        let offset = args
            .get("offset_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let max_chars = args
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .map(|v| (v as usize).clamp(1, 200_000))
            .unwrap_or(DEFAULT_MAX_CHARS);

        let id_owned = id.to_string();
        let content = tokio::task::spawn_blocking(move || {
            crate::agent::history::blob_store::get(&id_owned)
        })
        .await?;
        let Some(content) = content else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "No archived tool output found for blob id '{id}'. It may have been \
                     evicted from the archive; re-run the original tool instead."
                )),
            });
        };

        let total_chars = content.chars().count();
        let slice: String = content.chars().skip(offset).take(max_chars).collect();
        let end = offset.saturating_add(slice.chars().count()).min(total_chars);
        let mut output = format!(
            "[archived tool output {id}: chars {offset}..{end} of {total_chars}]\n{slice}"
        );
        if end < total_chars {
            output.push_str(&format!(
                "\n[... {} more chars; call again with offset_chars={end}]",
                total_chars - end
            ));
        }
        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
