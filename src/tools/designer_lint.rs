// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::traits::{Tool, ToolResult};
use crate::agent::designer::{design_md, lint};

pub struct DesignerLintTool;

impl DesignerLintTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesignerLintTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_workspace_file(rel_path: &str) -> Result<(std::path::PathBuf, String), String> {
    let rel = rel_path.trim();
    if rel.is_empty() {
        return Err("Missing required field `path`.".to_string());
    }
    if rel.contains("..") {
        return Err("Path traversal is not allowed.".to_string());
    }
    let session = crate::session::current_session_context()
        .ok_or_else(|| "No active session workspace.".to_string())?;
    let workspace = std::path::PathBuf::from(&session.workspace_dir);
    let candidate = workspace.join(rel);
    Ok((candidate, rel.replace('\\', "/")))
}

#[async_trait]
impl Tool for DesignerLintTool {
    fn name(&self) -> &str {
        "designer_lint"
    }

    fn description(&self) -> &str {
        "Audit a design artifact (Designer mode). For `.html` paths: anti-AI-slop lint with P0/P1/P2 findings — indigo accents, trust gradients, emoji icons, invented metrics, filler copy, placeholder CDNs, untokenized hex, missing data-od-id. For deck manifests (`deck.json`): full deck spec validation + compile (same as `deck_compile`). For diagram sources: `.mmd` (Mermaid type/fence checks), `.echarts.json` (strict JSON + option structure), `.mindmap.md` (single-root nested list). For `DESIGN.md` baton paths: structural validation of the token frontmatter and usage sections. Provide `path` (workspace-relative). Run during the critique stage and fix every P0/error before shipping."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace-relative path to the HTML artifact to lint."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let rel_path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let (abs, rel) = match resolve_workspace_file(&rel_path) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e),
                });
            }
        };
        let content = match tokio::fs::read_to_string(&abs).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Could not read `{rel}`: {e}")),
                });
            }
        };
        let lower = rel.to_ascii_lowercase();
        if lower.ends_with("deck.json") {
            let Some(deck_dir) = abs.parent().map(std::path::Path::to_path_buf) else {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Cannot resolve the deck directory for `{rel}`.")),
                });
            };
            let workspace = crate::session::current_session_context()
                .map(|s| std::path::PathBuf::from(s.workspace_dir))
                .unwrap_or_else(|| deck_dir.clone());
            let outcome = tokio::task::spawn_blocking(move || {
                crate::agent::designer::deck::compile::compile_deck(&deck_dir, &workspace)
            })
            .await
            .map_err(|e| anyhow::anyhow!("deck compile task failed: {e}"))?;
            return Ok(ToolResult {
                success: true,
                output: outcome.format_report(&rel),
                error: None,
            });
        }
        if lower.ends_with(".mmd") {
            let report = lint::lint_mermaid(&content);
            return Ok(ToolResult {
                success: true,
                output: lint::format_report(&rel, &report),
                error: None,
            });
        }
        if lower.ends_with(".echarts.json") {
            let report = lint::lint_echarts_json(&content);
            return Ok(ToolResult {
                success: true,
                output: lint::format_report(&rel, &report),
                error: None,
            });
        }
        if lower.ends_with(".mindmap.md") {
            let report = lint::lint_mindmap_md(&content);
            return Ok(ToolResult {
                success: true,
                output: lint::format_report(&rel, &report),
                error: None,
            });
        }
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            let report = design_md::validate(&content);
            return Ok(ToolResult {
                success: true,
                output: design_md::format_validation(&rel, &report),
                error: None,
            });
        }
        let report = lint::lint_html(&content);
        Ok(ToolResult {
            success: true,
            output: lint::format_report(&rel, &report),
            error: None,
        })
    }
}
