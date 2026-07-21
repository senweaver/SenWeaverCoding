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

const WHOLE_FILE_EMIT_THRESHOLD: usize = 256 * 1024;

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

async fn fresh_mtime_note(path: &std::path::Path) -> String {
    match tokio::fs::metadata(path).await {
        Ok(meta) => meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| format!("\n[mtime_ms: {}]", d.as_millis() as u64))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn find_eol_insensitive_unique(content: &str, old_string: &str) -> Option<(usize, usize, bool)> {
    if !content.contains('\r') && !old_string.contains('\r') {
        return None;
    }
    let old_lf = old_string.replace("\r\n", "\n");
    if old_lf.is_empty() {
        return None;
    }

    let bytes = content.as_bytes();
    let mut normalized: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut offsets: Vec<usize> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            offsets.push(i);
            normalized.push(b'\n');
            i += 2;
        } else {
            offsets.push(i);
            normalized.push(bytes[i]);
            i += 1;
        }
    }

    let finder = Finder::new(old_lf.as_bytes());
    let mut matches = finder.find_iter(&normalized);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    let norm_end = first + old_lf.len();
    let orig_start = offsets[first];
    let last_norm_idx = norm_end - 1;
    let last_orig_idx = offsets[last_norm_idx];
    let last_width = if bytes[last_orig_idx] == b'\r' { 2 } else { 1 };
    let orig_end = last_orig_idx + last_width;
    let span_had_crlf = content[orig_start..orig_end].contains("\r\n");
    Some((orig_start, orig_end, span_had_crlf))
}

impl FileEditTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        let ops_applier = Arc::new(
            OpsApplier::default_for_shared_workspace(security.workspace_root_handle())
                .with_allowed_roots(security.allowed_roots.clone()),
        );
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
        "PREFERRED tool for a single targeted change in ONE file: exact-string replace \
         (default), append, insert_after, or insert_before. old_string must match the file \
         exactly (copy it from a fresh file_read, including whitespace). For several related \
         changes use multi_edit; if you already hold a unified diff use diff_apply/patch_apply."
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
                                    replaced; matches outside are left untouched. If the scope cannot \
                                    be located, the edit fails unless force_whole_file is true."
                },
                "force_whole_file": {
                    "type": "boolean",
                    "description": "When true and scope cannot be located, search/replace across the \
                                    whole file instead of failing (default false)."
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

        let _resource_guard = match crate::session::acquire_file_write_locked(
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
                Self::emit_edit_if_small(
                    path,
                    original.as_bytes(),
                    new_content.as_bytes(),
                    batch_id,
                )
                .await;
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    /// Apply a single contiguous slice replacement as a range-scoped `EditOp::Replace`
    /// (carrying only the changed bytes, not the whole file). `whole_before` is the
    /// current full file content, used to render the UI diff only when the file is
    /// small enough; above the threshold the whole-file emit is skipped entirely.
    async fn dispatch_range_replace(
        &self,
        path: &std::path::Path,
        byte_range: std::ops::Range<usize>,
        old_slice: &str,
        new_slice: &str,
        whole_before: &str,
    ) -> Result<(), String> {
        self.snapshot_before_write(path).await;
        let batch = EditBatch::new(EditOrigin::FileEditTool).with_op(EditOp::Replace {
            path: path.to_path_buf(),
            byte_range: byte_range.clone(),
            old_text: old_slice.to_string(),
            new_text: new_slice.to_string(),
            anchor: None,
        });
        let batch_id = batch.batch_id.clone();
        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
                crate::session::record_write_for_current_session(path);
                if whole_before.len() <= WHOLE_FILE_EMIT_THRESHOLD {
                    let mut whole_after =
                        String::with_capacity(whole_before.len() + new_slice.len());
                    whole_after.push_str(&whole_before[..byte_range.start]);
                    whole_after.push_str(new_slice);
                    whole_after.push_str(&whole_before[byte_range.end..]);
                    Self::emit_edit_if_small(
                        path,
                        whole_before.as_bytes(),
                        whole_after.as_bytes(),
                        batch_id,
                    )
                    .await;
                } else {
                    tracing::debug!(
                        target: "tools.file_edit",
                        path = %path.display(),
                        bytes = whole_before.len(),
                        "skipping whole-file edit emit for a large file; range op applied and journaled"
                    );
                }
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    async fn emit_edit_if_small(
        path: &std::path::Path,
        before: &[u8],
        after: &[u8],
        batch_id: String,
    ) {
        if before.len() > WHOLE_FILE_EMIT_THRESHOLD && after.len() > WHOLE_FILE_EMIT_THRESHOLD {
            tracing::debug!(
                target: "tools.file_edit",
                path = %path.display(),
                "skipping whole-file edit emit for a large file"
            );
            return;
        }
        crate::agent::file_edit_emitter::emit_file_edit(
            path,
            Some(before),
            Some(after),
            Some(batch_id),
        )
        .await;
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

        let force_whole_file = args
            .get("force_whole_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

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
            let range = crate::code_intel::outline::locate_named_scope_in(&content, name)
                .map(|r| r.start.min(content.len())..r.end.min(content.len()));
            if range.is_none() {
                if force_whole_file {
                    tracing::warn!(
                        scope = %name,
                        path = %display_path,
                        "scope not found - force_whole_file enabled, searching whole file"
                    );
                    None
                } else {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "scope '{name}' not found in {display_path}; \
                             refuse whole-file replace (pass force_whole_file=true to override)"
                        )),
                    });
                }
            } else {
                range
            }
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
            if !replace_all && scope_name.is_none() {
                if let Some((span_start, span_end, span_had_crlf)) =
                    find_eol_insensitive_unique(&content, old_string)
                {
                    let adapted_new = if span_had_crlf {
                        new_string.replace("\r\n", "\n").replace('\n', "\r\n")
                    } else {
                        new_string.replace("\r\n", "\n")
                    };
                    let mut out = String::with_capacity(
                        content.len() + adapted_new.len(),
                    );
                    out.push_str(&content[..span_start]);
                    out.push_str(&adapted_new);
                    out.push_str(&content[span_end..]);
                    return match self
                        .dispatch_full_file_rewrite(resolved_target, &content, &out)
                        .await
                    {
                        Ok(()) => {
                            let diff =
                                self.generate_diff(display_path, old_string, &adapted_new);
                            Ok(ToolResult {
                                success: true,
                                output: format!(
                                    "Edited {display_path}: replaced 1 occurrence(s) \
                                     [auto-recovered a CRLF/LF line-ending mismatch between old_string and the file; the replacement uses the file's original line endings]{}\n{diff}",
                                    fresh_mtime_note(resolved_target).await
                                ),
                                error: None,
                            })
                        }
                        Err(e) => Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Failed to write file: {e}")),
                        }),
                    };
                }
            }
            let error = if let Some(name) = scope_name.as_deref() {
                // Scoped search failed inside the named scope; still analyze the
                // whole file so the model sees where the text actually lives.
                let mut msg = format!("old_string not found in scope '{name}'.");
                if let Some(diag) = super::match_diagnostics::diagnose(&content, old_string) {
                    msg.push('\n');
                    msg.push_str(&diag.message);
                }
                msg
            } else {
                let had_read =
                    crate::session::has_read_in_current_session(resolved_target);
                super::match_diagnostics::failure_message(
                    &content,
                    old_string,
                    display_path,
                    had_read,
                )
            };
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
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

        // Single, unambiguous occurrence: emit a range-scoped Replace carrying only
        // the changed slice instead of the whole file (avoids sending/logging the
        // entire file for a small edit). replace_all still rewrites whole-file since
        // its multiple non-contiguous slices cannot be one contiguous range op.
        if !replace_all {
            let pos = hits[0];
            let range = pos..(pos + old_string.len());
            return match self
                .dispatch_range_replace(
                    resolved_target,
                    range,
                    old_string,
                    new_string,
                    &content,
                )
                .await
            {
                Ok(()) => {
                    let diff = self.generate_diff(display_path, old_string, new_string);
                    Ok(ToolResult {
                        success: true,
                        output: format!(
                            "Edited {display_path}: replaced 1 occurrence(s){}\n{diff}",
                            fresh_mtime_note(resolved_target).await
                        ),
                        error: None,
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to write file: {e}")),
                }),
            };
        }

        let new_content = {
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
        };

        // `hits` is capped at 4 for the ambiguity check; count the real total.
        let replaced_count = finder.find_iter(&bytes[search_range.clone()]).count();

        match self
            .dispatch_full_file_rewrite(resolved_target, &content, &new_content)
            .await
        {
            Ok(()) => {
                let diff = self.generate_diff(display_path, old_string, new_string);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Edited {display_path}: replaced {replaced_count} occurrence(s){}\n{diff}",
                        fresh_mtime_note(resolved_target).await
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
            let had_read = crate::session::has_read_in_current_session(resolved_target);
            let error = super::match_diagnostics::failure_message(
                &content,
                pattern,
                display_path,
                had_read,
            );
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
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
            let had_read = crate::session::has_read_in_current_session(resolved_target);
            let error = super::match_diagnostics::failure_message(
                &content,
                pattern,
                display_path,
                had_read,
            );
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
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
