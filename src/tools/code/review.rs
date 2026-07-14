// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::json;

use crate::code_intel::review::context;
use crate::code_intel::symbol_graph::SymbolGraph;

const EMPTY_GRAPH_HINT: &str =
    "\n\n(Note: the SymbolGraph has no symbols yet for this workspace — supported source files \
     may be absent, or run a build first. Impact analysis needs a populated graph.)";

use super::super::traits::{Tool, ToolResult};

pub struct CodeReviewTool;

impl CodeReviewTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeReviewTool {
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
                    "code review workspace argument escapes session workspace; confining to workspace root"
                );
                base_c
            }
        }
        _ => base,
    }
}

fn load_or_build_graph(root: &std::path::Path) -> std::io::Result<SymbolGraph> {
    if let Some(writer) =
        crate::code_intel::symbol_graph::incremental::get_or_build_writer(root)
    {
        return Ok(writer.graph().read().clone());
    }
    let g = SymbolGraph::build(root)?;
    let _ = g.persist(root);
    Ok(g)
}

fn parse_changed_files(args: &serde_json::Value) -> Option<Vec<PathBuf>> {
    let arr = args.get("changed_files")?.as_array()?;
    let files: Vec<PathBuf> = arr
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    if files.is_empty() { None } else { Some(files) }
}

#[async_trait]
impl Tool for CodeReviewTool {
    fn name(&self) -> &str {
        "code_review"
    }

    fn description(&self) -> &str {
        "Token-efficient code review over the workspace SymbolGraph.  \
         Maps git changes to affected symbols and computes the minimal review set so you \
         read only what matters.  Actions: `impact_radius` (blast radius of changed files), \
         `detect_changes` (risk-scored review priorities + test gaps), `review_context` \
         (focused subgraph + relevant source line ranges), `minimal_context` (~compact bootstrap). \
         Every action attaches an estimated `context_savings` (tokens saved vs reading whole files)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["impact_radius", "detect_changes", "review_context", "minimal_context"],
                    "description": "Which review operation to run.",
                },
                "changed_files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit changed file paths (relative to workspace). Auto-detected from git diff when omitted.",
                },
                "base": {
                    "type": "string",
                    "description": "Git ref to diff against (default `HEAD`, i.e. uncommitted changes; use `HEAD~1` to review the last commit).",
                },
                "include_source": {
                    "type": "boolean",
                    "description": "review_context only: include relevant source line ranges (default true).",
                },
                "max_lines_per_file": {
                    "type": "integer",
                    "minimum": 20,
                    "maximum": 2000,
                    "description": "review_context only: above this size only changed-symbol line ranges are returned (default 200).",
                },
                "task": {
                    "type": "string",
                    "description": "minimal_context only: short task description used to suggest next tools.",
                },
                "workspace": {
                    "type": "string",
                    "description": "Workspace root.  Defaults to the current working directory.",
                },
            },
            "required": ["action"],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tokio::task::spawn_blocking(move || run_review(args))
            .await
            .map_err(|e| anyhow::anyhow!("code_review task panicked: {e}"))?
    }
}

fn run_review(args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let workspace = resolve_workspace(args.get("workspace").and_then(|v| v.as_str()));
        let base = args
            .get("base")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("HEAD")
            .to_string();
        let changed = parse_changed_files(&args);

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

        let graph_empty = graph.symbols.iter().all(|s| s.id.is_file_anchor());

        let mut report = match action {
            "impact_radius" => {
                context::impact_radius_report(&graph, &workspace, changed, &base)
            }
            "detect_changes" => {
                context::detect_changes_report(&graph, &workspace, changed, &base)
            }
            "review_context" => {
                let include_source = args
                    .get("include_source")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let max_lines = args
                    .get("max_lines_per_file")
                    .and_then(|v| v.as_u64())
                    .map(|v| v.clamp(20, 2000) as usize)
                    .unwrap_or(200);
                context::review_context_report(
                    &graph,
                    &workspace,
                    changed,
                    &base,
                    include_source,
                    max_lines,
                )
            }
            "minimal_context" => {
                let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
                context::minimal_context_report(&graph, &workspace, task, &base)
            }
            "" => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("`action` is required".into()),
                });
            }
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("unknown action: {other}")),
                });
            }
        };

        if graph_empty {
            report.push_str(EMPTY_GRAPH_HINT);
        }

        Ok(ToolResult {
            success: true,
            output: report,
            error: None,
        })
}
