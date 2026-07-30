// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use regex::Regex;
use serde_json::json;

use crate::agent::flows::checkpoint::Checkpoint;
use crate::agent::flows::registry::global_checkpoint_store;
use crate::agent::flows::traits::Artifact;
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::code_intel::symbol_graph::SymbolGraph;
use crate::security::SecurityPolicy;

use super::super::traits::{Tool, ToolResult};

pub struct CodeXfileRefactorTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Arc<OpsApplier>,
}

impl CodeXfileRefactorTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        let ops_applier = Arc::new(
            OpsApplier::default_for_shared_workspace(security.workspace_root_handle())
                .with_allowed_roots(security.allowed_roots.clone()),
        );
        Self {
            security,
            ops_applier,
        }
    }

    #[must_use]
    pub fn with_ops_applier(mut self, ops_applier: Arc<OpsApplier>) -> Self {
        self.ops_applier = ops_applier;
        self
    }
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

struct FileRename {
    rel: PathBuf,
    abs: PathBuf,
    old: String,
    new: String,
    identifier_scoped: bool,
}

#[cfg(feature = "code-intel")]
fn rename_identifiers_ast(
    source: &str,
    path: &Path,
    symbol: &str,
    new_name: &str,
) -> Option<(String, usize)> {
    let lang = crate::apply_model::grammar_id_for_path(path)?;
    let language = crate::code_intel::grammars::grammar_for(lang)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();
    let mut walker = root.walk();
    let mut stack = vec![root];
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    while let Some(node) = stack.pop() {
        if node.child_count() == 0 {
            let kind = node.kind();
            if kind.contains("identifier")
                && let Ok(text) = node.utf8_text(bytes)
                && text == symbol
            {
                ranges.push((node.start_byte(), node.end_byte()));
            }
        } else {
            for child in node.children(&mut walker) {
                stack.push(child);
            }
        }
    }
    if ranges.is_empty() {
        return Some((source.to_string(), 0));
    }
    ranges.sort_by_key(|(s, _)| *s);
    let mut out = String::with_capacity(source.len());
    let mut last = 0usize;
    let mut count = 0usize;
    for (s, e) in ranges {
        if s < last {
            continue;
        }
        out.push_str(&source[last..s]);
        out.push_str(new_name);
        last = e;
        count += 1;
    }
    out.push_str(&source[last..]);
    Some((out, count))
}

#[async_trait]
impl Tool for CodeXfileRefactorTool {
    fn name(&self) -> &str {
        "code_xfile_refactor"
    }

    fn description(&self) -> &str {
        "Cross-file symbol refactor guided by the workspace \
         SymbolGraph.  `mode=preview` returns per-file diffs without \
         writing; `mode=apply` performs the rename through the shared \
         edit pipeline (journal + validation + rollback) and pushes a \
         checkpoint so `flow_rollback` can undo it.  With code-intel the \
         rename only touches identifier tokens, never matches inside \
         comments or string literals."
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
                "workspace": {
                    "type": "string",
                    "description": "Optional workspace root; must resolve inside the session workspace.",
                },
            },
            "required": ["symbol", "new_name"],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }
        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        let symbol = args
            .get("symbol")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new_name = args
            .get("new_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("preview")
            .to_string();

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

        let workspace = match args.get("workspace").and_then(|v| v.as_str()) {
            Some(ws) => {
                if !self.security.is_path_allowed(ws) {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "workspace '{ws}' is outside the session workspace"
                        )),
                    });
                }
                let resolved = self.security.resolve_tool_path(ws);
                let canonical = tokio::fs::canonicalize(&resolved)
                    .await
                    .unwrap_or(resolved);
                if !self.security.is_resolved_path_allowed(&canonical) {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(
                            self.security.resolved_path_violation_message(&canonical),
                        ),
                    });
                }
                canonical
            }
            None => self.security.workspace_dir(),
        };

        let symbol_for_blocking = symbol.clone();
        let new_name_for_blocking = new_name.clone();
        let workspace_for_blocking = workspace.clone();
        let computed = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<FileRename>> {
            let graph = match SymbolGraph::load(&workspace_for_blocking) {
                Ok(Some(g)) => g,
                _ => {
                    let g = SymbolGraph::build(&workspace_for_blocking)?;
                    let _ = g.persist(&workspace_for_blocking);
                    g
                }
            };

            let mut affected_files: Vec<PathBuf> = Vec::new();
            for entry in &graph.symbols {
                if entry.id.name == symbol_for_blocking
                    && !affected_files.contains(&entry.id.file)
                {
                    affected_files.push(entry.id.file.clone());
                }
            }
            for edge in &graph.edges {
                if (edge.to.name == symbol_for_blocking || edge.from.name == symbol_for_blocking)
                    && !affected_files.contains(&edge.from.file)
                {
                    affected_files.push(edge.from.file.clone());
                }
            }

            let re = word_boundary_re(&symbol_for_blocking)?;
            let mut renames: Vec<FileRename> = Vec::new();
            for rel in &affected_files {
                let abs = workspace_for_blocking.join(rel);
                let Ok(old) = std::fs::read_to_string(&abs) else {
                    continue;
                };

                #[cfg(feature = "code-intel")]
                let (new, identifier_scoped) = match rename_identifiers_ast(
                    &old,
                    &abs,
                    &symbol_for_blocking,
                    &new_name_for_blocking,
                ) {
                    Some((rewritten, _)) => (rewritten, true),
                    None => (
                        re.replace_all(&old, new_name_for_blocking.as_str())
                            .to_string(),
                        false,
                    ),
                };
                #[cfg(not(feature = "code-intel"))]
                let (new, identifier_scoped) = (
                    re.replace_all(&old, new_name_for_blocking.as_str())
                        .to_string(),
                    false,
                );

                if new != old {
                    renames.push(FileRename {
                        rel: rel.clone(),
                        abs,
                        old,
                        new,
                        identifier_scoped,
                    });
                }
            }
            Ok(renames)
        })
        .await
        .map_err(|e| anyhow::anyhow!("code_xfile_refactor task panicked: {e}"))??;

        if computed.is_empty() {
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

        let regex_only = computed.iter().any(|r| !r.identifier_scoped);
        let regex_note = if regex_only {
            Some(
                "regex word-boundary match used for at least one file (no code-intel grammar); \
                 matches inside comments or string literals may have been renamed - review the diff"
                    .to_string(),
            )
        } else {
            None
        };

        match mode.as_str() {
            "preview" => {
                let payload = json!({
                    "mode": "preview",
                    "symbol": symbol,
                    "new_name": new_name,
                    "identifier_scoped": !regex_only,
                    "note": regex_note,
                    "files": computed.iter().map(|r| json!({
                        "file": r.rel,
                        "diff": unified_diff(&r.old, &r.new, &r.rel),
                    })).collect::<Vec<_>>(),
                });
                Ok(ToolResult {
                    success: true,
                    output: payload.to_string(),
                    error: None,
                })
            }
            "apply" => {
                let planned_paths: Vec<PathBuf> =
                    computed.iter().map(|r| r.abs.clone()).collect();
                let _resource_guards =
                    match crate::session::acquire_many_file_write_guards(
                        planned_paths.clone(),
                    )
                    .await
                    {
                        Ok(g) => g,
                        Err(e) => {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!("{e}")),
                            });
                        }
                    };

                for p in &planned_paths {
                    if crate::session::is_stale_for_current_session(p) {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(crate::session::stale_file_error_message(p)),
                        });
                    }
                }

                let mut batch = EditBatch::new(EditOrigin::XfileRefactorTool).with_atomic(true);
                for r in &computed {
                    batch.push(EditOp::Replace {
                        path: r.abs.clone(),
                        byte_range: 0..r.old.len(),
                        old_text: r.old.clone(),
                        new_text: r.new.clone(),
                        anchor: None,
                    });
                }
                let batch_id = batch.batch_id.clone();

                let artifacts_pre: Vec<Artifact> = computed
                    .iter()
                    .map(|r| Artifact::new(r.rel.to_string_lossy(), r.old.clone()))
                    .collect();

                match self.ops_applier.apply_batch(batch).await {
                    Ok(_) => {
                        for r in &computed {
                            crate::session::record_write_for_current_session(&r.abs);
                            crate::agent::file_edit_emitter::emit_file_edit(
                                &r.abs,
                                Some(r.old.as_bytes()),
                                Some(r.new.as_bytes()),
                                Some(batch_id.clone()),
                            )
                            .await;
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
                                "identifier_scoped": !regex_only,
                                "note": regex_note,
                                "files_changed": computed.len(),
                                "files": computed.iter().map(|r| &r.rel).collect::<Vec<_>>(),
                            })
                            .to_string(),
                            error: None,
                        })
                    }
                    Err(e) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cross-file refactor failed (rolled back {} file(s)): {e}",
                            computed.len()
                        )),
                    }),
                }
            }
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("unknown mode: {other}")),
            }),
        }
    }
}
