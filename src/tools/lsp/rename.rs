// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::text_edit::{apply_edits_to_content, secure_resolve_target, uri_to_local_path};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use regex::Regex;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Default)]
struct WorkspaceEditOutcome {
    applied: usize,
    errors: Vec<String>,
}

struct PendingFileEdit {
    resolved: PathBuf,
    old_text: String,
    new_text: String,
    applied: usize,
}

pub struct LspRenameTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Arc<OpsApplier>,
}

impl LspRenameTool {
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

    fn resolve_path(&self, file_path: &str) -> PathBuf {
        let p = PathBuf::from(file_path);
        if p.is_absolute() {
            p
        } else {
            self.security.workspace_dir().join(p)
        }
    }

    fn word_boundary_regex(symbol_name: &str) -> anyhow::Result<Regex> {
        let escaped = regex::escape(symbol_name);
        Regex::new(&format!(r"\b{escaped}\b"))
            .map_err(|e| anyhow::anyhow!("Failed to build rename regex: {e}"))
    }

    fn validate_write_path(&self, file_path: &str) -> anyhow::Result<()> {
        if !self.security.is_path_allowed(file_path) {
            anyhow::bail!("Path not allowed by security policy: {file_path}");
        }
        Ok(())
    }

    async fn secure_write(
        &self,
        file_path: &Path,
        old_content: &str,
        new_content: &str,
    ) -> Result<(), String> {
        let security = self.security.clone();
        let probe = file_path.to_path_buf();
        let resolved =
            tokio::task::spawn_blocking(move || secure_resolve_target(&security, &probe))
                .await
                .map_err(|e| format!("Path resolution task failed: {e}"))??;
        self.apply_full_file_replace(&resolved, old_content, new_content)
            .await
    }

    async fn apply_full_file_replace(
        &self,
        resolved: &Path,
        old_content: &str,
        new_content: &str,
    ) -> Result<(), String> {
        let op = EditOp::Replace {
            path: resolved.to_path_buf(),
            byte_range: 0..old_content.len(),
            old_text: old_content.to_string(),
            new_text: new_content.to_string(),
            anchor: None,
        };
        let batch = EditBatch::new(EditOrigin::XfileRefactorTool).with_op(op);
        self.ops_applier
            .apply_batch(batch)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", resolved.display()))?;
        crate::session::record_write_for_current_session(resolved);
        Ok(())
    }

    async fn text_rename(
        &self,
        file_path: &PathBuf,
        symbol_name: &str,
        new_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<ToolResult> {
        if !file_path.is_file() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("File not found: {}", file_path.display())),
            });
        }

        let re = Self::word_boundary_regex(symbol_name)?;
        let content = tokio::fs::read_to_string(file_path).await?;

        if !re.is_match(&content) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Symbol '{}' not found in file", symbol_name)),
            });
        }

        let occurrences: Vec<(usize, String)> = content
            .lines()
            .enumerate()
            .filter(|(_, line)| re.is_match(line))
            .map(|(i, line)| (i + 1, line.trim().to_string()))
            .collect();

        let match_count: usize = content.lines().map(|line| re.find_iter(line).count()).sum();

        if dry_run {
            Ok(ToolResult {
                success: true,
                output: format!(
                    "[DRY RUN] Would rename '{}' to '{}' ({} match(es) on {} line(s)):\n{}",
                    symbol_name,
                    new_name,
                    match_count,
                    occurrences.len(),
                    occurrences
                        .iter()
                        .map(|(line, content)| format!("  Line {}: {}", line, content))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                error: None,
            })
        } else {
            if !self.security.record_action() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Rate limit exceeded: action budget exhausted".into()),
                });
            }

            let new_content = re.replace_all(&content, new_name).to_string();
            if let Err(e) = self.secure_write(file_path, &content, &new_content).await {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e),
                });
            }

            Ok(ToolResult {
                success: true,
                output: format!(
                    "Renamed '{}' to '{}' ({} match(es)) in file: {}",
                    symbol_name,
                    new_name,
                    match_count,
                    file_path.display()
                ),
                error: None,
            })
        }
    }

    async fn glob_rename(
        &self,
        pattern: &str,
        symbol_name: &str,
        new_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<ToolResult> {
        use glob::glob as glob_pattern;

        let re = Self::word_boundary_regex(symbol_name)?;

        let full_pattern = format!("{}/{}", self.security.workspace_dir().display(), pattern);
        let matches: Vec<PathBuf> = tokio::task::spawn_blocking(
            move || -> anyhow::Result<Vec<PathBuf>> {
                let paths = glob_pattern(&full_pattern)?;
                Ok(paths
                    .filter_map(|entry| entry.ok())
                    .filter(|path| path.is_file())
                    .collect())
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("glob task panicked: {e}"))??;

        if matches.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No files found matching pattern: {}", pattern),
                error: None,
            });
        }

        let mut results: Vec<String> = Vec::new();
        results.push(format!(
            "Found {} file(s) matching pattern '{}'",
            matches.len(),
            pattern
        ));

        let mut total_renamed = 0;
        let mut files_changed = 0;

        for path in &matches {
            if !path.is_file() {
                continue;
            }

            let path_str = path.to_string_lossy();
            if !self.security.is_path_allowed(&path_str) {
                results.push(format!("  Skipped (policy): {}", path.display()));
                continue;
            }

            let content = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(e) => {
                    results.push(format!("  Error reading {}: {}", path.display(), e));
                    continue;
                }
            };

            let count = re.find_iter(&content).count();
            if count == 0 {
                continue;
            }

            if dry_run {
                total_renamed += count;
                results.push(format!("  [DRY] {} ({} match(es))", path.display(), count));
            } else {
                if !self.security.record_action() {
                    results.push(format!("  Rate limited, skipping: {}", path.display()));
                    continue;
                }

                let new_content = re.replace_all(&content, new_name).to_string();
                if let Err(e) = self.secure_write(path, &content, &new_content).await {
                    results.push(format!("  Error writing {}: {}", path.display(), e));
                    continue;
                }
                total_renamed += count;
                files_changed += 1;
                results.push(format!(
                    "  Renamed {} match(es) in: {}",
                    count,
                    path.display()
                ));
            }
        }

        if dry_run {
            results.push(format!(
                "\n[DRY RUN] Would rename {} total match(es)",
                total_renamed
            ));
        } else {
            results.push(format!(
                "\nRenamed {} total match(es) across {} file(s)",
                total_renamed, files_changed
            ));
        }

        Ok(ToolResult {
            success: true,
            output: results.join("\n"),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for LspRenameTool {
    fn name(&self) -> &str {
        "lsp_rename"
    }

    fn description(&self) -> &str {
        "Rename a symbol across files. Supports single file rename and glob pattern rename. \
         Use file_path for single file, or glob_pattern for batch rename across multiple files. \
         Provides dry-run mode to preview changes before applying."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file containing the symbol to rename (use either file_path OR glob_pattern)"
                },
                "glob_pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts'). Use either glob_pattern OR file_path."
                },
                "symbol_name": {
                    "type": "string",
                    "description": "The current name of the symbol to rename"
                },
                "new_name": {
                    "type": "string",
                    "description": "The new name for the symbol"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would be changed without making edits (default: false)"
                }
            },
            "required": ["symbol_name", "new_name"]
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

        let symbol_name = args
            .get("symbol_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'symbol_name' parameter"))?;

        let new_name = args
            .get("new_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_name' parameter"))?;

        let cli_dry_run = crate::util::get_runtime_var("SEN_DRY_RUN").as_deref() == Some("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        if symbol_name.is_empty() || new_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("symbol_name and new_name cannot be empty".into()),
            });
        }

        if symbol_name == new_name {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("symbol_name and new_name must be different".into()),
            });
        }

        if let Some(pattern) = args.get("glob_pattern").and_then(|v| v.as_str()) {
            return self
                .glob_rename(pattern, symbol_name, new_name, dry_run)
                .await;
        }

        let file_path_str = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Either 'file_path' or 'glob_pattern' is required"))?;

        self.validate_write_path(file_path_str)?;

        let file_path = self.resolve_path(file_path_str);

        if !dry_run {
            if let Ok(lsp_result) = self.try_lsp_rename(&file_path, symbol_name, new_name).await {
                return Ok(lsp_result);
            }
        }

        self.text_rename(&file_path, symbol_name, new_name, dry_run)
            .await
    }
}

impl LspRenameTool {

    async fn try_lsp_rename(
        &self,
        file_path: &std::path::PathBuf,
        symbol_name: &str,
        new_name: &str,
    ) -> anyhow::Result<ToolResult> {
        let svc = crate::services::try_get_services()
            .ok_or_else(|| anyhow::anyhow!("Services not initialized"))?;

        let lang = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let file_uri = crate::services::lsp::core::path_to_uri(file_path);

        let content = tokio::fs::read_to_string(file_path).await?;
        let (line, character) = content
            .lines()
            .enumerate()
            .find_map(|(i, l)| {
                find_symbol_column_utf16(l, symbol_name).map(|col| (i as u32, col as u32))
            })
            .ok_or_else(|| anyhow::anyhow!("Symbol not found in file for LSP rename"))?;

        let params = serde_json::json!({
            "textDocument": { "uri": file_uri },
            "position": { "line": line, "character": character },
            "newName": new_name
        });

        match svc
            .lsp
            .request(
                lang,
                &self.security.workspace_dir(),
                Some(file_path),
                "textDocument/rename",
                params,
            )
            .await
        {
            Ok(resp) => {
                let security = self.security.clone();
                let (pending, mut compute_errors) =
                    tokio::task::spawn_blocking(move || compute_workspace_edits(&resp, &security))
                        .await
                        .unwrap_or_else(|_| (Vec::new(), Vec::new()));
                let mut outcome = WorkspaceEditOutcome {
                    applied: 0,
                    errors: std::mem::take(&mut compute_errors),
                };
                for edit in pending {
                    match self
                        .apply_full_file_replace(&edit.resolved, &edit.old_text, &edit.new_text)
                        .await
                    {
                        Ok(()) => outcome.applied += edit.applied,
                        Err(e) => outcome.errors.push(e),
                    }
                }
                if outcome.applied > 0 {
                    let mut output = format!(
                        "LSP rename: '{}' -> '{}' ({} edits applied via language server)",
                        symbol_name, new_name, outcome.applied
                    );
                    if !outcome.errors.is_empty() {
                        output.push_str(&format!(
                            "\nSkipped {} file(s):\n{}",
                            outcome.errors.len(),
                            outcome.errors.join("\n")
                        ));
                    }
                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    })
                } else if !outcome.errors.is_empty() {
                    Err(anyhow::anyhow!(
                        "LSP rename blocked by security policy: {}",
                        outcome.errors.join("; ")
                    ))
                } else {
                    Err(anyhow::anyhow!("LSP rename returned no edits"))
                }
            }
            Err(e) => Err(anyhow::anyhow!("LSP rename failed: {e}")),
        }
    }
}

fn find_symbol_column_utf16(line: &str, symbol: &str) -> Option<usize> {
    if symbol.is_empty() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut search_from = 0usize;
    while let Some(rel) = line[search_from..].find(symbol) {
        let byte_pos = search_from + rel;
        let before_ok = line[..byte_pos]
            .chars()
            .next_back()
            .map(|c| !is_word(c))
            .unwrap_or(true);
        let after_ok = line[byte_pos + symbol.len()..]
            .chars()
            .next()
            .map(|c| !is_word(c))
            .unwrap_or(true);
        if before_ok && after_ok {
            let utf16_col: usize = line[..byte_pos].chars().map(|c| c.len_utf16()).sum();
            return Some(utf16_col);
        }
        search_from = byte_pos + symbol.len();
        if search_from >= line.len() {
            break;
        }
    }
    None
}

fn collect_edit_groups(resp: &serde_json::Value) -> Vec<(String, Vec<serde_json::Value>)> {
    let mut groups: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
    if let Some(doc_changes) = resp.get("documentChanges").and_then(|v| v.as_array()) {
        for change in doc_changes {
            let Some(uri) = change.pointer("/textDocument/uri").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(edits) = change.get("edits").and_then(|v| v.as_array()) {
                groups.push((uri.to_string(), edits.clone()));
            }
        }
        if !groups.is_empty() {
            return groups;
        }
    }
    if let Some(changes) = resp.get("changes").and_then(|v| v.as_object()) {
        for (uri, edits_val) in changes {
            if let Some(edits) = edits_val.as_array() {
                groups.push((uri.clone(), edits.clone()));
            }
        }
    }
    groups
}

fn compute_workspace_edits(
    resp: &serde_json::Value,
    security: &SecurityPolicy,
) -> (Vec<PendingFileEdit>, Vec<String>) {
    let mut pending: Vec<PendingFileEdit> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (uri, edits) in collect_edit_groups(resp) {
        let file_path = uri_to_local_path(&uri);
        let resolved = match secure_resolve_target(security, &file_path) {
            Ok(p) => p,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("Failed to read {}: {e}", resolved.display()));
                continue;
            }
        };
        let (new_content, applied, edit_errors) = apply_edits_to_content(&content, &edits);
        errors.extend(edit_errors);
        if new_content == content {
            continue;
        }
        pending.push(PendingFileEdit {
            resolved,
            old_text: content,
            new_text: new_content,
            applied,
        });
    }
    (pending, errors)
}
