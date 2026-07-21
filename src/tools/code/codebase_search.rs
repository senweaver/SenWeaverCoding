// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::json;

use super::super::traits::{Tool, ToolResult};

pub struct CodebaseSearchTool;

impl CodebaseSearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodebaseSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_workspace() -> PathBuf {
    crate::session::current_session_context()
        .map(|c| PathBuf::from(c.workspace_dir))
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[async_trait]
impl Tool for CodebaseSearchTool {
    fn name(&self) -> &str {
        "codebase_search"
    }

    fn description(&self) -> &str {
        "Semantic + lexical search over the indexed codebase. Give a natural-language question \
         (e.g. \"where is the websocket auth handled?\") and get back the most relevant code \
         locations as path:line with snippets. Fuses the dense vector index (when enabled) with \
         the lexical index via reciprocal-rank fusion. Prefer this over raw content_search when \
         you don't know the exact symbols/strings to grep for."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language description of what you are looking for."
                },
                "top_k": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 8, max 25)."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let top_k = args
            .get("top_k")
            .and_then(|v| v.as_u64())
            .map(|n| n.clamp(1, 25) as usize)
            .unwrap_or(8);

        let workspace = resolve_workspace();
        let Some(source) = crate::agent::loop_::services::rag_source(&workspace) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Code search index is unavailable (code_rag disabled or not initialized)."
                        .to_string(),
                ),
            });
        };

        let hits = source.retrieve(query, top_k).await;
        if hits.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No codebase matches for query: {query}"),
                error: None,
            });
        }

        let mut out = String::with_capacity(256);
        out.push_str(&format!("Codebase search results for: {query}\n\n"));
        for (i, hit) in hits.iter().enumerate() {
            let rel = crate::util::path_relative_to(&hit.path, &workspace)
                .unwrap_or_else(|| hit.path.clone());
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            out.push_str(&format!("{}. {}:{}\n", i + 1, rel_str, hit.line));
            let snippet = hit.snippet.trim_end();
            if !snippet.is_empty() {
                for line in snippet.lines().take(6) {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out.push('\n');
        }

        Ok(ToolResult {
            success: true,
            output: out,
            error: None,
        })
    }
}
