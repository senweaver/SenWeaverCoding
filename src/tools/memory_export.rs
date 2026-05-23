// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::memory::traits::ExportFilter;
use crate::memory::{Memory, MemoryCategory};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct MemoryExportTool {
    memory: Arc<dyn Memory>,
}

impl MemoryExportTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryExportTool {
    fn name(&self) -> &str {
        "memory_export"
    }

    fn description(&self) -> &str {
        "Export memories as a JSON array for GDPR Art. 20 data portability. \
         Supports filtering by namespace, session, category, and time range. \
         Returns a structured, machine-readable JSON array of memory entries."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "namespace": {
                    "type": "string",
                    "description": "Filter by namespace (agent/context isolation boundary)."
                },
                "session_id": {
                    "type": "string",
                    "description": "Filter by session ID."
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category: core, daily, conversation, or a custom name."
                },
                "since": {
                    "type": "string",
                    "description": "RFC 3339 lower bound (inclusive) on created_at. Example: 2025-01-01T00:00:00Z"
                },
                "until": {
                    "type": "string",
                    "description": "RFC 3339 upper bound (inclusive) on created_at. Example: 2025-12-31T23:59:59Z"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let namespace = args
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(String::from);
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let category = args
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "core" => MemoryCategory::Core,
                "daily" => MemoryCategory::Daily,
                "conversation" => MemoryCategory::Conversation,
                other => MemoryCategory::Custom(other.to_string()),
            });
        let since = args.get("since").and_then(|v| v.as_str()).map(String::from);
        let until = args.get("until").and_then(|v| v.as_str()).map(String::from);

        let filter = ExportFilter {
            namespace,
            session_id,
            category,
            since,
            until,
        };

        match self.memory.export(&filter).await {
            Ok(entries) => {
                let json_output = serde_json::to_string(&entries)
                    .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {e}\"}}"));
                Ok(ToolResult {
                    success: true,
                    output: json_output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Export failed: {e}")),
            }),
        }
    }
}
