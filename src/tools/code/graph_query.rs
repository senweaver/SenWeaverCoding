// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::json;

use crate::code_intel::git_timeline::build_timeline;
use crate::code_intel::symbol_graph::{EdgeKind, SymbolGraph, SymbolId};

use super::super::traits::{Tool, ToolResult};

pub struct CodeGraphQueryTool;

impl CodeGraphQueryTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeGraphQueryTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_workspace(arg: Option<&str>) -> PathBuf {
    let base = crate::session::current_session_context()
        .map(|c| PathBuf::from(c.workspace_dir))
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    match arg {
        Some(a) if !a.trim().is_empty() => {
            let candidate = PathBuf::from(a);
            let abs = if candidate.is_absolute() {
                candidate
            } else {
                base.join(&candidate)
            };
            let base_c = base.canonicalize().unwrap_or_else(|_| base.clone());
            let abs_c = abs.canonicalize().unwrap_or_else(|_| abs.clone());
            if abs_c.starts_with(&base_c) {
                abs_c
            } else {
                tracing::warn!(
                    target: "code_intel",
                    requested = %abs.display(),
                    "code_graph_query workspace argument escapes session workspace; confining to workspace root"
                );
                base_c
            }
        }
        _ => base,
    }
}

fn load_or_build_graph(root: &std::path::Path) -> std::io::Result<SymbolGraph> {
    if let Some(g) = SymbolGraph::load(root)? {
        return Ok(g);
    }
    let g = SymbolGraph::build(root)?;
    let _ = g.persist(root);
    Ok(g)
}

fn symbol_to_json(sym: &SymbolId) -> serde_json::Value {
    json!({
        "file": sym.file,
        "name": sym.name,
        "line": sym.line,
    })
}

#[async_trait]
impl Tool for CodeGraphQueryTool {
    fn name(&self) -> &str {
        "code_graph_query"
    }

    fn description(&self) -> &str {
        "Query the workspace SymbolGraph.  Supports `callers_of`, \
         `implementors_of`, and `recent_changes` (latest commit per \
         symbol via git blame)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "enum": ["callers_of", "implementors_of", "recent_changes"],
                },
                "symbol": {
                    "type": "string",
                    "description": "Symbol name to look up.  Required for `callers_of` and `implementors_of`.",
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum number of results to return (default 50).",
                },
                "workspace": {
                    "type": "string",
                    "description": "Workspace root.  Defaults to the current working directory.",
                },
            },
            "required": ["query"],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tokio::task::spawn_blocking(move || run_graph_query(args))
            .await
            .map_err(|e| anyhow::anyhow!("code_graph_query task panicked: {e}"))?
    }
}

fn run_graph_query(args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let symbol = args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(200) as usize)
            .unwrap_or(50);
        let workspace = resolve_workspace(args.get("workspace").and_then(|v| v.as_str()));

        let graph = match load_or_build_graph(&workspace) {
            Ok(g) => g,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("failed to load/build symbol graph: {e}")),
                });
            }
        };

        let payload = match query {
            "callers_of" => {
                if symbol.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("`symbol` is required for callers_of".into()),
                    });
                }
                let mut hits: Vec<&SymbolId> = graph.callers_of(symbol);
                hits.truncate(limit);
                json!({
                    "query": "callers_of",
                    "symbol": symbol,
                    "results": hits.iter().map(|s| symbol_to_json(s)).collect::<Vec<_>>(),
                })
            }
            "implementors_of" => {
                if symbol.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("`symbol` is required for implementors_of".into()),
                    });
                }
                let mut hits: Vec<&SymbolId> = graph.implementors_of(symbol);
                hits.truncate(limit);
                json!({
                    "query": "implementors_of",
                    "symbol": symbol,
                    "results": hits.iter().map(|s| symbol_to_json(s)).collect::<Vec<_>>(),
                })
            }
            "recent_changes" => {
                let tl = build_timeline(&workspace, &graph);
                let mut rows: Vec<(SymbolId, crate::code_intel::git_timeline::TimelineEntry)> = tl
                    .into_iter()
                    .filter(|(s, _)| symbol.is_empty() || s.name == symbol)
                    .collect();
                rows.sort_by(|a, b| {
                    b.1.author_time_unix
                        .unwrap_or(i64::MIN)
                        .cmp(&a.1.author_time_unix.unwrap_or(i64::MIN))
                });
                rows.truncate(limit);
                json!({
                    "query": "recent_changes",
                    "results": rows.iter().map(|(s, t)| json!({
                        "symbol": symbol_to_json(s),
                        "commit": t.commit,
                        "author": t.author,
                        "author_time_unix": t.author_time_unix,
                        "summary": t.summary,
                    })).collect::<Vec<_>>(),
                })
            }
            "" => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("`query` is required".into()),
                });
            }
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("unknown query kind: {other}")),
                });
            }
        };

        let _ = EdgeKind::Calls;

        Ok(ToolResult {
            success: true,
            output: payload.to_string(),
            error: None,
        })
}
