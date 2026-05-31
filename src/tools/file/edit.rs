// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::edit_history::EditHistory;
use super::super::traits::{Tool, ToolResult};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use memchr::memmem::Finder;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {

    Replace,

    Append,

    InsertAfter,

    InsertBefore,
}

impl Default for EditMode {
    fn default() -> Self {
        EditMode::Replace
    }
}

pub struct FileEditTool {
    security: Arc<SecurityPolicy>,
    edit_history: Option<Arc<EditHistory>>,
    ops_applier: Arc<OpsApplier>,
}

impl FileEditTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        let ops_applier = Arc::new(OpsApplier::default_for_shared_workspace(
            security.workspace_root_handle(),
        ));
        Self {
            security,
            edit_history: None,
            ops_applier,
        }
    }

    pub fn with_edit_history(mut self, history: Arc<EditHistory>) -> Self {
        self.edit_history = Some(history);
        self
    }

    #[must_use]
    pub fn with_ops_applier(mut self, ops_applier: Arc<OpsApplier>) -> Self {
        self.ops_applier = ops_applier;
        self
    }

    async fn snapshot_before_write(&self, path: &std::path::Path) {
        if let Some(ref history) = self.edit_history {
            let _ = history
                .snapshot_before_write_async(
                    path.to_path_buf(),
                    "file_edit".to_string(),
                    "edit file".to_string(),
                )
                .await;
        }
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file using various modes: replace (default), append, insert_after, insert_before"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Relative paths resolve from workspace; outside paths require policy allowlist."
                },
                "mode": {
                    "type": "string",
                    "description": "Edit mode: replace (default), append, insert_after, insert_before",
                    "enum": ["replace", "append", "insert_after", "insert_before"],
                    "default": "replace"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find (used for replace, insert_after, insert_before modes; ignored for append)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement/insertion text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences instead of requiring exactly one match (default false)"
                },
                "scope": {
                    "type": "string",
                    "description": "Optional function or method name (e.g. \"calculate\" or \"fn calculate\") \
                                    that restricts the replacement to the named scope's byte range. \
                                    When provided, only matches within that function/method body are \
                                    replaced; matches outside are left untouched. Falls back to \
                                    whole-file replace when the scope cannot be located."
                },
                "expected_mtime_ms": {
                    "type": "integer",
                    "description": "Expected file modification time in milliseconds since epoch. \
                                    If the file has been modified since this timestamp, the edit is \
                                    rejected to prevent overwriting manual edits."
                }
            },
            "required": ["path", "new_string"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .map(EditMode::from_str)
            .unwrap_or(Ok(EditMode::default()))
            .map_err(|e: String| anyhow::anyhow!(e))?;

        let old_string = args.get("old_string").and_then(|v| v.as_str());

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string' parameter"))?;

        if mode != EditMode::Append && old_string.is_none() {
            let mode_label = match mode {
                EditMode::Replace => "replace",
                EditMode::InsertAfter => "insert_after",
                EditMode::InsertBefore => "insert_before",
                EditMode::Append => "append",
            };
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("'old_string' is required for mode '{mode_label}'")),
            });
        }

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

        if !self.security.is_path_allowed(path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        let full_path = self.security.resolve_tool_path(path);

        let Some(parent) = full_path.parent() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid path: missing parent directory".into()),
            });
        };

        let resolved_parent = match tokio::fs::canonicalize(parent).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to resolve file path: {e}")),
                });
            }
        };

        if !self.security.is_resolved_path_allowed(&resolved_parent) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .resolved_path_violation_message(&resolved_parent),
                ),
            });
        }

        if !crate::security::sandbox_allows_path(&resolved_parent) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Sandbox policy denies write to {}",
                    resolved_parent.display()
                )),
            });
        }

        let Some(file_name) = full_path.file_name() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid path: missing file name".into()),
            });
        };

        let resolved_target = resolved_parent.join(file_name);

        if self.security.is_runtime_config_path(&resolved_target) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .runtime_config_violation_message(&resolved_target),
                ),
            });
        }

        if let Ok(meta) = tokio::fs::symlink_metadata(&resolved_target).await {
            if meta.file_type().is_symlink() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to edit through symlink: {}",
                        resolved_target.display()
                    )),
                });
            }
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let _resource_guard = match crate::session::acquire_file_write_for_current_session(
            &resolved_target,
        )
        .await
        {
            Some(Ok(g)) => Some(g),
            Some(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
            None => None,
        };

        if crate::session::is_stale_for_current_session(&resolved_target) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(crate::session::stale_file_error_message(&resolved_target)),
            });
        }

        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
        if let Ok(meta) = tokio::fs::metadata(&resolved_target).await {
            if meta.len() > MAX_FILE_SIZE {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "File too large ({:.1} MB). Maximum supported size is 10 MB.",
                        meta.len() as f64 / (1024.0 * 1024.0)
                    )),
                });
            }
        }

        if let Some(expected_ms) = args.get("expected_mtime_ms").and_then(|v| v.as_u64()) {
            if let Ok(meta) = tokio::fs::metadata(&resolved_target).await {
                if let Ok(mtime) = meta.modified() {
                    let actual_ms = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if actual_ms != expected_ms {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "File has been modified since last read \
                                 (expected mtime {expected_ms}ms, actual {actual_ms}ms). \
                                 Please re-read the file before editing to avoid \
                                 overwriting manual changes."
                            )),
                        });
                    }
                }
            }
        }

        match mode {
            EditMode::Replace => {
                let old_string = old_string
                    .ok_or_else(|| anyhow::anyhow!("'old_string' is required for mode 'replace'"))?;
                self.execute_replace(&args, old_string, new_string, &resolved_target, path)
                    .await
            }
            EditMode::Append => {
                self.execute_append(new_string, &resolved_target, path)
                    .await
            }
            EditMode::InsertAfter => {
                let old_string = old_string.ok_or_else(|| {
                    anyhow::anyhow!("'old_string' is required for mode 'insert_after'")
                })?;
                self.execute_insert_after(old_string, new_string, &resolved_target, path)
                    .await
            }
            EditMode::InsertBefore => {
                let old_string = old_string.ok_or_else(|| {
                    anyhow::anyhow!("'old_string' is required for mode 'insert_before'")
                })?;
                self.execute_insert_before(old_string, new_string, &resolved_target, path)
                    .await
            }
        }
    }
}

impl FileEditTool {

    async fn dispatch_full_file_rewrite(
        &self,
        path: &std::path::Path,
        original: &str,
        new_content: &str,
    ) -> Result<(), String> {
        self.snapshot_before_write(path).await;
        let batch = EditBatch::new(EditOrigin::FileEditTool).with_op(EditOp::Replace {
            path: path.to_path_buf(),
            byte_range: 0..original.len(),
            old_text: original.to_string(),
            new_text: new_content.to_string(),
            anchor: None,
        });
        let batch_id = batch.batch_id.clone();
        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
                crate::session::record_write_for_current_session(path);
                crate::agent::file_edit_emitter::emit_file_edit(
                    path,
                    Some(original.as_bytes()),
                    Some(new_content.as_bytes()),
                    Some(batch_id),
                )
                .await;
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    async fn execute_replace(
        &self,
        args: &serde_json::Value,
        old_string: &str,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let scope_name = args
            .get("scope")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if old_string.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("old_string must not be empty".into()),
            });
        }

        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let scope_range: Option<std::ops::Range<usize>> = if let Some(ref name) = scope_name {
            let range =
                crate::code_intel::outline::locate_named_scope(resolved_target, name);
            if range.is_none() {
                tracing::warn!(
                    scope = %name,
                    path = %display_path,
                    "scope not found  - falling back to whole-file replace"
                );
            }
            range
        } else {
            None
        };

        let finder = Finder::new(old_string.as_bytes());
        let bytes = content.as_bytes();

        let mut hits: Vec<usize> = Vec::with_capacity(4);
        let search_range = scope_range.clone().unwrap_or(0..bytes.len());
        for pos in finder.find_iter(&bytes[search_range.clone()]) {

            let abs_pos = search_range.start + pos;
            hits.push(abs_pos);
            if hits.len() >= 4 {
                break;
            }
        }

        if hits.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(if scope_name.is_some() {
                    format!(
                        "old_string not found in scope '{}'",
                        scope_name.as_deref().unwrap_or("")
                    )
                } else {
                    "old_string not found in file".into()
                }),
            });
        }

        if !replace_all && hits.len() > 1 {

            let total = if hits.len() == 4 {
                let extra = finder
                    .find_iter(&bytes[search_range.clone()])
                    .count();
                extra
            } else {
                hits.len()
            };
            let mut msg = format!(
                "old_string matches {total} times. Showing first 3 hit locations:\n",
            );
            for hit in hits.iter().take(3) {
                let (line_no, line) = locate_line(&content, *hit);
                msg.push_str(&format!(
                    "  - line {line_no} : {line}\n",
                ));
            }
            msg.push_str(
                "Use exact, longer old_string (include surrounding lines) to disambiguate.",
            );
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(msg),
            });
        }

        let new_content = if replace_all {

            let mut out = String::with_capacity(bytes.len() + new_string.len());
            let mut cursor = 0usize;
            for pos in finder.find_iter(&bytes[search_range.clone()]) {
                let abs_pos = search_range.start + pos;
                out.push_str(&content[cursor..abs_pos]);
                out.push_str(new_string);
                cursor = abs_pos + old_string.len();
            }
            out.push_str(&content[cursor..]);
            out
        } else {
            let pos = hits[0];
            let mut out = String::with_capacity(bytes.len() + new_string.len());
            out.push_str(&content[..pos]);
            out.push_str(new_string);
            out.push_str(&content[pos + old_string.len()..]);
            out
        };

        let replaced_count = if replace_all { hits.len() } else { 1 };

        match self
            .dispatch_full_file_rewrite(resolved_target, &content, &new_content)
            .await
        {
            Ok(()) => {
                let diff = self.generate_diff(display_path, old_string, new_string);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Edited {display_path}: replaced {replaced_count} occurrence(s)\n{diff}"
                    ),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
        }
    }

    async fn execute_append(
        &self,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let needs_newline = !content.is_empty() && !content.ends_with('\n');
        let to_append = if needs_newline {
            format!("\n{new_string}")
        } else {
            new_string.to_string()
        };
        let new_content = format!("{content}{to_append}");

        match self
            .dispatch_full_file_rewrite(resolved_target, &content, &new_content)
            .await
        {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!("Appended to {display_path}:\n```\n{}\n```", new_string),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
        }
    }

    async fn execute_insert_after(
        &self,
        pattern: &str,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let finder = Finder::new(pattern.as_bytes());
        let bytes = content.as_bytes();
        let mut hits: Vec<usize> = Vec::new();
        for pos in finder.find_iter(bytes) {
            hits.push(pos);
            if hits.len() >= 2 {
                break;
            }
        }

        if hits.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Pattern not found in file".into()),
            });
        }

        if hits.len() > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Pattern matches {} times; must match exactly once",
                    finder.find_iter(bytes).count()
                )),
            });
        }

        let pos = hits[0];
        let insert_pos = pos + pattern.len();
        let new_content = format!("{}{}{}", &content[..insert_pos], new_string, &content[insert_pos..]);

        match self
            .dispatch_full_file_rewrite(resolved_target, &content, &new_content)
            .await
        {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!(
                    "Inserted after pattern in {display_path}:\n```\n{}\n```",
                    new_string
                ),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
        }
    }

    async fn execute_insert_before(
        &self,
        pattern: &str,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let finder = Finder::new(pattern.as_bytes());
        let bytes = content.as_bytes();
        let mut hits: Vec<usize> = Vec::new();
        for pos in finder.find_iter(bytes) {
            hits.push(pos);
            if hits.len() >= 2 {
                break;
            }
        }

        if hits.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Pattern not found in file".into()),
            });
        }

        if hits.len() > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Pattern matches {} times; must match exactly once",
                    finder.find_iter(bytes).count()
                )),
            });
        }

        let pos = hits[0];
        let new_content = format!("{}{}{}", &content[..pos], new_string, &content[pos..]);

        match self
            .dispatch_full_file_rewrite(resolved_target, &content, &new_content)
            .await
        {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!(
                    "Inserted before pattern in {display_path}:\n```\n{}\n```",
                    new_string
                ),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
        }
    }

    fn generate_diff(&self, path: &str, old_string: &str, new_string: &str) -> String {
        let old_lines: Vec<&str> = old_string.lines().collect();
        let new_lines: Vec<&str> = new_string.lines().collect();
        let mut diff_buf = format!("--- a/{path}\n+++ b/{path}\n");
        let old_line_count = old_lines.len();
        let new_line_count = new_lines.len();
        diff_buf.push_str(&format!("@@ -{old_line_count} +{new_line_count} @@\n"));
        for line in &old_lines {
            diff_buf.push_str(&format!("-{line}\n"));
        }
        for line in &new_lines {
            diff_buf.push_str(&format!("+{line}\n"));
        }
        diff_buf
    }
}

fn locate_line(content: &str, byte_offset: usize) -> (usize, &str) {
    let mut line_no = 1usize;
    let mut line_start = 0usize;
    for (i, c) in content.char_indices() {
        if i >= byte_offset {
            break;
        }
        if c == '\n' {
            line_no += 1;
            line_start = i + 1;
        }
    }
    let line_end = content[line_start..]
        .find('\n')
        .map(|p| line_start + p)
        .unwrap_or(content.len());
    (line_no, content[line_start..line_end].trim_end_matches('\r'))
}

impl EditMode {
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "replace" | "default" => Ok(EditMode::Replace),
            "append" => Ok(EditMode::Append),
            "insert_after" => Ok(EditMode::InsertAfter),
            "insert_before" => Ok(EditMode::InsertBefore),
            _ => Err(format!(
                "Unknown edit mode: {}. Valid modes: replace, append, insert_after, insert_before",
                s
            )),
        }
    }
}
