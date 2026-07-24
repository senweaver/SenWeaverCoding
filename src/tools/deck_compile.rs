// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::traits::{Tool, ToolResult};
use crate::agent::designer::deck::compile;

fn discover_deck_dir(workspace: &std::path::Path, session_rel: &str) -> Option<String> {
    let base = workspace.join(session_rel.replace('\\', "/").trim_start_matches('/'));
    let entries = std::fs::read_dir(&base).ok()?;
    let mut candidates: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join(compile::MANIFEST_FILE).is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            candidates.push((mtime, format!("{}/{}", session_rel.trim_end_matches('/'), name)));
        }
    }
    candidates.sort_by_key(|(t, _)| *t);
    candidates.pop().map(|(_, rel)| rel)
}

pub struct DeckCompileTool;

impl DeckCompileTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DeckCompileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for DeckCompileTool {
    fn name(&self) -> &str {
        "deck_compile"
    }

    fn description(&self) -> &str {
        "Validate a slide deck spec and compile it into the final PPTX (Designer mode, slide deck). \
         Reads `deck.json` + `slides/*.json` from the deck directory, runs schema/content/budget \
         validation, writes `render.json` (canvas preview model) and `deck.pptx`, and returns \
         P0/P1/P2 findings with exact file+field locations plus any slide files still missing. \
         Provide `dir` (workspace-relative path to the directory containing deck.json); when \
         omitted, the active design session's `deck/` directory is used. Run after every batch of \
         slide writes and fix every P0 before finishing."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Workspace-relative path to the deck directory (the one containing deck.json). Defaults to `<design output dir>/deck`."
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(session) = crate::session::current_session_context() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("No active session workspace.".to_string()),
            });
        };
        let workspace = std::path::PathBuf::from(&session.workspace_dir);
        let rel = match args
            .get("dir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(d) => d.to_string(),
            None => {
                let session_rel =
                    crate::agent::designer::pipeline::designer_session_dir(&session.session_id);
                discover_deck_dir(&workspace, &session_rel)
                    .unwrap_or_else(|| format!("{session_rel}/deck"))
            }
        };
        if rel.contains("..") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Path traversal is not allowed.".to_string()),
            });
        }
        let rel_norm = rel.replace('\\', "/");
        let mut deck_dir = workspace.join(rel_norm.trim_start_matches('/'));
        if !deck_dir.join(compile::MANIFEST_FILE).is_file() && deck_dir.join("deck").join(compile::MANIFEST_FILE).is_file() {
            deck_dir = deck_dir.join("deck");
        }
        if !deck_dir.join(compile::MANIFEST_FILE).is_file() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "No `{}` found under `{rel_norm}` — write the deck manifest first (see the deck sub-mode skill).",
                    compile::MANIFEST_FILE
                )),
            });
        }
        let outcome = tokio::task::spawn_blocking(move || compile::compile_deck(&deck_dir, &workspace))
            .await
            .map_err(|e| anyhow::anyhow!("deck compile task failed: {e}"))?;
        Ok(ToolResult {
            success: true,
            output: outcome.format_report(&rel_norm),
            error: None,
        })
    }
}
