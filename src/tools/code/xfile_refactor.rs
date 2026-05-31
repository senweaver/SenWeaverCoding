// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;
use serde_json::json;

use crate::agent::flows::checkpoint::Checkpoint;
use crate::agent::flows::registry::global_checkpoint_store;
use crate::agent::flows::traits::Artifact;
use crate::code_intel::symbol_graph::SymbolGraph;

use super::super::traits::{Tool, ToolResult};

pub struct CodeXfileRefactorTool;

impl CodeXfileRefactorTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeXfileRefactorTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_workspace(arg: Option<&str>) -> PathBuf {
    arg.map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn word_boundary_re(name: &str) -> anyhow::Result<Regex> {
    let escaped = regex::escape(name);
    Regex::new(&format!(r"\b{escaped}\b")).map_err(|e| anyhow::anyhow!("bad regex: {e}"))
}

fn unified_diff(old: &str, new: &str, path: &Path) -> String {

    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = String::new();
    out.push_str(&format!("--- {}\n", path.display()));
    out.push_str(&format!("+++ {}\n", path.display()));
    let max = old_lines.len().max(new_lines.len());
    for i in 0..max {
        match (old_lines.get(i), new_lines.get(i)) {
            (Some(o), Some(n)) if o == n => {}
            (Some(o), Some(n)) => {
                out.push_str(&format!("-{o}\n"));
                out.push_str(&format!("+{n}\n"));
            }
            (Some(o), None) => out.push_str(&format!("-{o}\n")),
            (None, Some(n)) => out.push_str(&format!("+{n}\n")),
            (None, None) => {}
        }
    }
    out
}

#[async_trait]
impl Tool for CodeXfileRefactorTool {
    fn name(&self) -> &str {
        "code_xfile_refactor"
    }

    fn description(&self) -> &str {
        "Cross-file symbol refactor guided by the workspace \
         SymbolGraph.  `mode=preview` returns per-file diffs without \
         writing; `mode=apply` performs the rename and pushes a \
         checkpoint so `flow_rollback` can undo it."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "symbol": { "type": "string" },
                "new_name": { "type": "string" },
                "mode": {
                    "type": "string",
                    "enum": ["preview", "apply"],
                    "description": "Default: preview.",
                },
                "workspace": { "type": "string" },
            },
            "required": ["symbol", "new_name"],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let symbol = args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        let new_name = args.get("new_name").and_then(|v| v.as_str()).unwrap_or("");
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("preview")
            .to_string();
        let workspace = resolve_workspace(args.get("workspace").and_then(|v| v.as_str()));

        if symbol.is_empty() || new_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("`symbol` and `new_name` are required".into()),
            });
        }
        if symbol == new_name {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("`new_name` equals `symbol`  - nothing to do".into()),
            });
        }

        let graph = match SymbolGraph::load(&workspace) {
            Ok(Some(g)) => g,
            _ => match SymbolGraph::build(&workspace) {
                Ok(g) => {
                    let _ = g.persist(&workspace);
                    g
                }
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("could not build SymbolGraph: {e}")),
                    });
                }
            },
        };

        let mut affected_files: Vec<PathBuf> = Vec::new();
        for entry in &graph.symbols {
            if entry.id.name == symbol && !affected_files.contains(&entry.id.file) {
                affected_files.push(entry.id.file.clone());
            }
        }
        for edge in &graph.edges {
            if (edge.to.name == symbol || edge.from.name == symbol)
                && !affected_files.contains(&edge.from.file)
            {
                affected_files.push(edge.from.file.clone());
            }
        }

        let re = word_boundary_re(symbol)?;
        let mut per_file: Vec<(PathBuf, String, String)> = Vec::new();
        for rel in &affected_files {
            let abs = workspace.join(rel);
            let Ok(old) = fs::read_to_string(&abs) else {
                continue;
            };
            let new = re.replace_all(&old, new_name).to_string();
            if new != old {
                per_file.push((rel.clone(), old, new));
            }
        }

        if per_file.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: json!({
                    "mode": mode,
                    "symbol": symbol,
                    "new_name": new_name,
                    "files": [],
                    "note": "no files required modification",
                })
                .to_string(),
                error: None,
            });
        }

        match mode.as_str() {
            "preview" => {
                let payload = json!({
                    "mode": "preview",
                    "symbol": symbol,
                    "new_name": new_name,
                    "files": per_file.iter().map(|(rel, old, new)| json!({
                        "file": rel,
                        "diff": unified_diff(old, new, rel),
                    })).collect::<Vec<_>>(),
                });
                Ok(ToolResult {
                    success: true,
                    output: payload.to_string(),
                    error: None,
                })
            }
            "apply" => {
                let mut artifacts_pre: Vec<Artifact> = Vec::new();
                for (rel, old, new) in &per_file {
                    let abs = workspace.join(rel);
                    artifacts_pre.push(Artifact::new(rel.to_string_lossy(), old.clone()));
                    if let Err(e) = fs::write(&abs, new) {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("write failed for {}: {e}", abs.display())),
                        });
                    }
                }

                let cp = Checkpoint::new(
                    format!("xfile_refactor::{symbol}->{new_name}"),
                    format!("rename {symbol} -> {new_name}"),
                    artifacts_pre,
                    vec![],
                );
                global_checkpoint_store().push(cp);

                Ok(ToolResult {
                    success: true,
                    output: json!({
                        "mode": "apply",
                        "symbol": symbol,
                        "new_name": new_name,
                        "files_changed": per_file.len(),
                        "files": per_file.iter().map(|(rel, _, _)| rel).collect::<Vec<_>>(),
                        "checkpoint_pushed": true,
                    })
                    .to_string(),
                    error: None,
                })
            }
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("unknown mode: {other}")),
            }),
        }
    }
}
