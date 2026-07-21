// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::memory::Memory;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

pub struct MemoryRecallTool {
    memory: Arc<dyn Memory>,
}

impl MemoryRecallTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }

    fn description(&self) -> &str {
        "Search long-term memory for relevant facts, preferences, or context. Returns scored results ranked by relevance. Supports keyword search, time-only query (since/until), or both."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords or phrase to search for in memory (optional if since/until provided)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return (default: 5)"
                },
                "since": {
                    "type": "string",
                    "description": "Filter memories created at or after this time (RFC 3339, e.g. 2025-03-01T00:00:00Z)"
                },
                "until": {
                    "type": "string",
                    "description": "Filter memories created at or before this time (RFC 3339)"
                },
                "search_mode": {
                    "type": "string",
                    "enum": ["bm25", "embedding", "hybrid"],
                    "description": "Preferred search strategy hint (bm25/embedding/hybrid). The effective strategy is determined by the configured memory backend and embedding availability; this hint is validated but the backend config decides the actual mode."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let since = args.get("since").and_then(|v| v.as_str());
        let until = args.get("until").and_then(|v| v.as_str());

        if query.trim().is_empty() && since.is_none() && until.is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Provide at least 'query' (keywords) or time range ('since'/'until')".into(),
                ),
            });
        }

        if let Some(s) = since {
            if chrono::DateTime::parse_from_rfc3339(s).is_err() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Invalid 'since' date: {s}. Expected RFC 3339 format, e.g. 2025-03-01T00:00:00Z"
                    )),
                });
            }
        }
        if let Some(u) = until {
            if chrono::DateTime::parse_from_rfc3339(u).is_err() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Invalid 'until' date: {u}. Expected RFC 3339 format, e.g. 2025-03-01T00:00:00Z"
                    )),
                });
            }
        }
        if let (Some(s), Some(u)) = (since, until) {
            if let (Ok(s_dt), Ok(u_dt)) = (
                chrono::DateTime::parse_from_rfc3339(s),
                chrono::DateTime::parse_from_rfc3339(u),
            ) {
                if s_dt >= u_dt {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'since' must be before 'until'".into()),
                    });
                }
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(5, |v| v as usize)
            .min(1000);

        let session_id = crate::session::current_session_context().map(|c| c.session_id);

        if let Some(mode) = args.get("search_mode").and_then(|v| v.as_str()) {
            let provider = std::env::var("SEN_MEMORY_EMBEDDING_PROVIDER")
                .ok()
                .or_else(|| {
                    crate::services::try_get_services().and_then(|svc| {
                        let cfg = svc.shared_config.load();
                        let p = cfg.memory.embedding_provider.trim().to_string();
                        if p.is_empty() { None } else { Some(p) }
                    })
                })
                .unwrap_or_else(|| "none".into())
                .to_ascii_lowercase();
            let embedding_ready = !(provider.is_empty() || provider == "none");
            if matches!(mode, "embedding" | "hybrid") && !embedding_ready {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "search_mode={mode} requires a configured memory embedding provider \
                         (current embedding_provider={provider})"
                    )),
                });
            }
        }

        match self
            .memory
            .recall(query, limit, session_id.as_deref(), since, until)
            .await
        {
            Ok(entries) if entries.is_empty() => Ok(ToolResult {
                success: true,
                output: "No memories found.".into(),
                error: None,
            }),
            Ok(entries) => {
                crate::agent::profile::runtime_hooks::publish_memory_event("recall", None);
                let mut output = format!("Found {} memories:\n", entries.len());
                for entry in &entries {
                    let score = entry
                        .score
                        .map_or_else(String::new, |s| format!(" [{:.0}%]", s * 100.0));
                    let _ = writeln!(
                        output,
                        "- [{}] {}: {}{score}",
                        entry.category, entry.key, entry.content
                    );
                }
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Memory recall failed: {e}")),
            }),
        }
    }
}
