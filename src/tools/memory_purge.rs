// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::memory::Memory;
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct MemoryPurgeTool {
    memory: Arc<dyn Memory>,
    security: Arc<SecurityPolicy>,
}

impl MemoryPurgeTool {
    pub fn new(memory: Arc<dyn Memory>, security: Arc<SecurityPolicy>) -> Self {
        Self { memory, security }
    }
}

#[async_trait]
impl Tool for MemoryPurgeTool {
    fn name(&self) -> &str {
        "memory_purge"
    }

    fn description(&self) -> &str {
        "Remove all memories in a namespace (category) or session. Use to bulk-delete conversation context or category-scoped data. Returns the number of deleted entries. WARNING: This operation cannot be undone."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "namespace": {
                    "type": "string",
                    "description": "The namespace (category) to purge. Deletes all memories in this category."
                },
                "session_id": {
                    "type": "string",
                    "description": "The session ID to purge. Deletes all memories in this session."
                }
            },
            "minProperties": 1
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let namespace = args.get("namespace").and_then(|v| v.as_str());
        let session_id = args.get("session_id").and_then(|v| v.as_str());

        if namespace.is_none() && session_id.is_none() {
            return Err(anyhow::anyhow!(
                "Must provide either 'namespace' or 'session_id' parameter"
            ));
        }

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "memory_purge")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let mut total_purged = 0;
        let mut output_parts = Vec::new();

        if let Some(ns) = namespace {
            match self.memory.purge_namespace(ns).await {
                Ok(count) => {
                    total_purged += count;
                    output_parts.push(format!("Purged {count} memories from namespace '{ns}'"));
                }
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to purge namespace: {e}")),
                    });
                }
            }
        }

        if let Some(sid) = session_id {
            match self.memory.purge_session(sid).await {
                Ok(count) => {
                    total_purged += count;
                    output_parts.push(format!("Purged {count} memories from session '{sid}'"));
                }
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to purge session: {e}")),
                    });
                }
            }
        }

        Ok(ToolResult {
            success: true,
            output: if output_parts.is_empty() {
                format!("Purged {total_purged} memories")
            } else {
                output_parts.join("; ")
            },
            error: None,
        })
    }
}
