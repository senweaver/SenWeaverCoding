// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::code_intel::search::{IncrementalIndex, heuristic};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct CodeSearchTool {
    workspace_dir: PathBuf,

    shared_index: Option<Arc<dyn IncrementalIndex>>,
}

impl CodeSearchTool {
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self {
            workspace_dir,
            shared_index: None,
        }
    }

    pub fn with_index(mut self, idx: Arc<dyn IncrementalIndex>) -> Self {
        self.shared_index = Some(idx);
        self
    }
}

impl Default for CodeSearchTool {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_default())
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn name(&self) -> &str {
        "code_search"
    }

    fn description(&self) -> &str {
        "Full-text search across the workspace.  Uses an incremental index when one \
         is available (built with the `code-search` feature); otherwise falls back \
         to an on-demand line scan."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query / substring." },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of hits to return (default 20, max 200)."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) => q.to_string(),
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing 'query' parameter".into()),
                });
            }
        };
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| (n as usize).min(200))
            .unwrap_or(20);

        let hits = {
            let shared_index = self.shared_index.clone();
            let workspace_dir = self.workspace_dir.clone();
            let query_owned = query.clone();
            tokio::task::spawn_blocking(move || {
                if let Some(idx) = shared_index {
                    idx.search(&query_owned, limit)
                } else {
                    let s = heuristic::Search::new(workspace_dir);
                    s.search(&query_owned, limit)
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("code_search join error: {e}"))?
        };

        match hits {
            Ok(hits) if hits.is_empty() => Ok(ToolResult {
                success: true,
                output: format!("No matches for '{query}'."),
                error: None,
            }),
            Ok(hits) => {

                let rendered = if crate::token_saver::is_enabled() {
                    let ctx = crate::token_saver::global();
                    if matches!(
                        ctx.level,
                        crate::token_saver::CompactLevel::Conservative
                    ) {
                        None
                    } else {
                        let grep_hits: Vec<crate::token_saver::GrepHit> = hits
                            .iter()
                            .map(|h| crate::token_saver::GrepHit {
                                file: h.path.to_string_lossy().to_string(),
                                line_no: u64::from(h.line),
                                line: h.snippet.clone(),
                            })
                            .collect();
                        let opts = crate::token_saver::GrepOpts {
                            level: ctx.level,
                            per_file_cap: 5,
                            total_cap: 0,
                        };
                        Some(crate::token_saver::compact_grep_results(
                            &grep_hits, &opts,
                        ))
                    }
                } else {
                    None
                };
                let mut buf = format!("{} matches for '{}':\n", hits.len(), query);
                if let Some(compact) = rendered {
                    buf.push_str(&compact);
                } else {
                    for h in hits {
                        buf.push_str(&format!(
                            "  {}:{}  {}\n",
                            h.path.display(),
                            h.line,
                            h.snippet
                        ));
                    }
                }
                Ok(ToolResult {
                    success: true,
                    output: buf,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("code_search failed: {e}")),
            }),
        }
    }
}
