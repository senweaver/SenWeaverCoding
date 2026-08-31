// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::edit_history::EditHistory;
use super::super::traits::{Tool, ToolResult};
use super::eol::{
    adapt_replacement_eol, adapt_text_to_eol, dominant_eol, find_eol_insensitive_spans, EolSpan,
};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use memchr::memmem::Finder;
use serde_json::json;
use std::sync::Arc;

use crate::agent::file_edit_emitter::WHOLE_FILE_EMIT_THRESHOLD;

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

async fn read_text_for_edit(
    path: &std::path::Path,
) -> std::io::Result<(String, Option<String>)> {
    let bytes = tokio::fs::read(path).await?;
    if crate::tools::file::encoding::is_probably_binary(&bytes) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to edit a binary file as text",
        ));
    }
    let (text, label) = crate::tools::file::encoding::decode_for_edit(&bytes)?;
    let non_utf8 = if crate::tools::file::encoding::is_utf8_label(label) {
        None
    } else {
        Some(label.to_string())
    };
    Ok((text, non_utf8))
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

fn offset_spans(mut spans: Vec<EolSpan>, base: usize) -> Vec<EolSpan> {
    for span in &mut spans {
        span.start += base;
        span.end += base;
    }
    spans
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
                },
                "near_line": {
                    "type": "integer",
                    "description": "Optional 1-based line-number anchor. When old_string matches \
                                    multiple locations, the match whose start is closest to this \
                                    line is used instead of failing on ambiguity. Copy the line \
                                    number from a fresh file_read."
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

        let _resource_guard = match crate::session::acquire_file_write_guard(&resolved_target)
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

        if crate::session::is_stale_for_current_session(&resolved_target) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(crate::session::stale_file_error_message(&resolved_target)),
            });
        }

        if resolved_target.exists()
            && args.get("expected_mtime_ms").is_none()
            && !crate::session::has_read_in_current_session(&resolved_target)
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Refusing to edit '{}': this session has not read the file yet. \
                     Use file_read on it first (the edit needs to be based on the file's \
                     CURRENT contents), then retry the edit. A compacted/Signatures view does \
                     not count: use level=default, paging large files with offset/limit.",
                    resolved_target.display()
                )),
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

        let diag_paths = [resolved_target.clone()];
        let diag_baseline =
            crate::code_intel::post_edit_diagnostics::baseline(&diag_paths).await;

        let mut result = match mode {
            EditMode::Replace => {
                let old_string = old_string
                    .ok_or_else(|| anyhow::anyhow!("'old_string' is required for mode 'replace'"))?;
                self.execute_replace(&args, old_string, new_string, &resolved_target, path)
                    .await?
            }
            EditMode::Append => {
                self.execute_append(new_string, &resolved_target, path)
                    .await?
            }
            EditMode::InsertAfter => {
                let old_string = old_string.ok_or_else(|| {
                    anyhow::anyhow!("'old_string' is required for mode 'insert_after'")
                })?;
                let near_line = args.get("near_line").and_then(|v| v.as_u64());
                self.execute_insert_after(
                    old_string,
                    new_string,
                    near_line,
                    &resolved_target,
                    path,
                )
                .await?
            }
            EditMode::InsertBefore => {
                let old_string = old_string.ok_or_else(|| {
                    anyhow::anyhow!("'old_string' is required for mode 'insert_before'")
                })?;
                let near_line = args.get("near_line").and_then(|v| v.as_u64());
                self.execute_insert_before(
                    old_string,
                    new_string,
                    near_line,
                    &resolved_target,
                    path,
                )
                .await?
            }
        };
        if result.success {
            if let Some(feedback) = crate::code_intel::post_edit_diagnostics::new_error_feedback(
                &diag_paths,
                &diag_baseline,
            )
            .await
            {
                result.output.push_str(&feedback);
            }
        }
        Ok(result)
    }
}

impl FileEditTool {

    async fn dispatch_full_file_rewrite_encoded(
        &self,
        path: &std::path::Path,
        original: &str,
        new_content: &str,
        encoding: Option<String>,
    ) -> Result<(), String> {
        self.snapshot_before_write(path).await;
        let op = match encoding {
            Some(label) => {
                let expected_pre_sha256 =
                    crate::tools::file::encoding::encode_with_label(&label, original)
                        .map(|b| crate::apply_model::edit_op::sha256_hex(&b));
                EditOp::CreateFile {
                    path: path.to_path_buf(),
                    contents: new_content.to_string(),
                    overwrite: true,
                    encoding: Some(label),
                    expected_pre_sha256,
                }
            }
            None => EditOp::Replace {
                path: path.to_path_buf(),
                byte_range: 0..original.len(),
                old_text: original.to_string(),
                new_text: new_content.to_string(),
                anchor: None,
            },
        };
        let batch = EditBatch::new(EditOrigin::FileEditTool).with_op(op);
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
                    Self::emit_edit_if_small(
                        path,
                        old_slice.as_bytes(),
                        new_slice.as_bytes(),
                        batch_id,
                    )
                    .await;
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
        if before.len() > WHOLE_FILE_EMIT_THRESHOLD || after.len() > WHOLE_FILE_EMIT_THRESHOLD {
            crate::agent::file_edit_emitter::emit_file_edit_large(
                path,
                before.len(),
                after.len(),
                Some(batch_id),
            )
            .await;
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

        let (content, encoding_label) = match read_text_for_edit(resolved_target).await {
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
            let eol_spans = offset_spans(
                find_eol_insensitive_spans(
                    &content[search_range.clone()],
                    old_string,
                    usize::MAX,
                ),
                search_range.start,
            );
            if !eol_spans.is_empty() {
                if replace_all {
                    let mut out = String::with_capacity(content.len() + new_string.len());
                    let mut cursor = 0usize;
                    for span in &eol_spans {
                        out.push_str(&content[cursor..span.start]);
                        out.push_str(&super::eol::adapt_new_text_for_span(
                            new_string,
                            span.had_crlf,
                        ));
                        cursor = span.end;
                    }
                    out.push_str(&content[cursor..]);
                    let count = eol_spans.len();
                    return match self
                        .dispatch_full_file_rewrite_encoded(
                            resolved_target,
                            &content,
                            &out,
                            encoding_label.clone(),
                        )
                        .await
                    {
                        Ok(()) => {
                            let base_line = locate_line(&content, eol_spans[0].start).0;
                            let diff = self.generate_diff(
                                display_path,
                                old_string,
                                new_string,
                                base_line,
                            );
                            Ok(ToolResult {
                                success: true,
                                output: format!(
                                    "Edited {display_path}: replaced {count} occurrence(s) \
                                     [auto-recovered a CRLF/LF line-ending mismatch between old_string and the file; replacements use the file's original line endings]{}\n{diff}",
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
                let chosen: Option<EolSpan> = if eol_spans.len() == 1 {
                    Some(eol_spans[0])
                } else if let Some(anchor_line) =
                    args.get("near_line").and_then(|v| v.as_u64())
                {
                    eol_spans.iter().copied().min_by_key(|span| {
                        let (line_no, _) = locate_line(&content, span.start);
                        (line_no as i64 - anchor_line as i64).unsigned_abs()
                    })
                } else {
                    None
                };
                let Some(span) = chosen else {
                    let mut msg = format!(
                        "old_string matches {} times after line-ending normalization (the \
                         file's CRLF/LF differs from your old_string; that difference is \
                         handled automatically). Showing first {} hit locations:\n",
                        eol_spans.len(),
                        eol_spans.len().min(3)
                    );
                    for span in eol_spans.iter().take(3) {
                        let (line_no, line) = locate_line(&content, span.start);
                        msg.push_str(&format!("  - line {line_no} : {line}\n"));
                    }
                    msg.push_str(
                        "Use exact, longer old_string (include surrounding lines) to \
                         disambiguate, or pass near_line=<line number> to anchor the intended \
                         match.",
                    );
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(msg),
                    });
                };
                let adapted_new =
                    super::eol::adapt_new_text_for_span(new_string, span.had_crlf);
                let mut out = String::with_capacity(content.len() + adapted_new.len());
                out.push_str(&content[..span.start]);
                out.push_str(&adapted_new);
                out.push_str(&content[span.end..]);
                let eol_note = if eol_spans.len() > 1 {
                    " [disambiguated by near_line: nearest match selected]"
                } else {
                    ""
                };
                return match self
                    .dispatch_full_file_rewrite_encoded(
                        resolved_target,
                        &content,
                        &out,
                        encoding_label.clone(),
                    )
                    .await
                {
                    Ok(()) => {
                        let base_line = locate_line(&content, span.start).0;
                        let diff = self.generate_diff(
                            display_path,
                            &content[span.start..span.end],
                            &adapted_new,
                            base_line,
                        );
                        Ok(ToolResult {
                            success: true,
                            output: format!(
                                "Edited {display_path}: replaced 1 occurrence(s) \
                                 [auto-recovered a CRLF/LF line-ending mismatch between old_string and the file; the replacement uses the file's original line endings]{eol_note}{}\n{diff}",
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
            if !replace_all && scope_name.is_none() {
                if let Some((ws_start, ws_end, adjusted_new)) =
                    super::match_diagnostics::find_whitespace_insensitive_unique(
                        &content, old_string, new_string,
                    )
                {
                    let adjusted_new = adapt_text_to_eol(&adjusted_new, dominant_eol(&content));
                    let mut out =
                        String::with_capacity(content.len() + adjusted_new.len());
                    out.push_str(&content[..ws_start]);
                    out.push_str(&adjusted_new);
                    out.push_str(&content[ws_end..]);
                    return match self
                        .dispatch_full_file_rewrite_encoded(
                            resolved_target,
                            &content,
                            &out,
                            encoding_label.clone(),
                        )
                        .await
                    {
                        Ok(()) => {
                            let base_line = locate_line(&content, ws_start).0;
                            let diff = self.generate_diff(
                                display_path,
                                &content[ws_start..ws_end],
                                &adjusted_new,
                                base_line,
                            );
                            Ok(ToolResult {
                                success: true,
                                output: format!(
                                    "Edited {display_path}: replaced 1 occurrence(s) \
                                     [auto-recovered a whitespace/indentation mismatch; the \
                                     replacement was re-indented to the file's actual \
                                     indentation]{}\n{diff}",
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

        let mut hits = hits;
        let mut near_line_note = "";
        if !replace_all && hits.len() > 1 {
            let near_line = args.get("near_line").and_then(|v| v.as_u64());
            if let Some(anchor_line) = near_line {
                let all_hits: Vec<usize> = finder
                    .find_iter(&bytes[search_range.clone()])
                    .map(|pos| search_range.start + pos)
                    .collect();
                let nearest = all_hits.iter().copied().min_by_key(|pos| {
                    let (line_no, _) = locate_line(&content, *pos);
                    (line_no as i64 - anchor_line as i64).unsigned_abs()
                });
                if let Some(best) = nearest {
                    hits = vec![best];
                    near_line_note =
                        " [disambiguated by near_line: nearest match selected]";
                }
            }
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
                "Use exact, longer old_string (include surrounding lines) to disambiguate, \
                 or pass near_line=<line number> to anchor the intended match.",
            );
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(msg),
            });
        }

        let adapted_exact = adapt_replacement_eol(old_string, new_string, dominant_eol(&content));
        let new_string = adapted_exact.as_str();

        if !replace_all && encoding_label.is_none() {
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
                    let base_line = locate_line(&content, pos).0;
                    let diff =
                        self.generate_diff(display_path, old_string, new_string, base_line);
                    Ok(ToolResult {
                        success: true,
                        output: format!(
                            "Edited {display_path}: replaced 1 occurrence(s){near_line_note}{}\n{diff}",
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

        let (new_content, replaced_count) = if replace_all {
            let mut out = String::with_capacity(bytes.len() + new_string.len());
            let mut cursor = 0usize;
            let mut count = 0usize;
            for pos in finder.find_iter(&bytes[search_range.clone()]) {
                let abs_pos = search_range.start + pos;
                out.push_str(&content[cursor..abs_pos]);
                out.push_str(new_string);
                cursor = abs_pos + old_string.len();
                count += 1;
            }
            out.push_str(&content[cursor..]);
            (out, count)
        } else {
            let pos = hits[0];
            let mut out = String::with_capacity(content.len() + new_string.len());
            out.push_str(&content[..pos]);
            out.push_str(new_string);
            out.push_str(&content[pos + old_string.len()..]);
            (out, 1usize)
        };

        match self
            .dispatch_full_file_rewrite_encoded(
                resolved_target,
                &content,
                &new_content,
                encoding_label.clone(),
            )
            .await
        {
            Ok(()) => {
                let diff = if replaced_count > 1 {
                    if content.len() <= WHOLE_FILE_EMIT_THRESHOLD
                        && new_content.len() <= WHOLE_FILE_EMIT_THRESHOLD
                    {
                        self.generate_diff(display_path, &content, &new_content, 1)
                    } else {
                        let base_line = locate_line(&content, hits[0]).0;
                        self.generate_diff(display_path, old_string, new_string, base_line)
                    }
                } else {
                    let base_line = locate_line(&content, hits[0]).0;
                    self.generate_diff(display_path, old_string, new_string, base_line)
                };
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Edited {display_path}: replaced {replaced_count} occurrence(s){near_line_note}{}\n{diff}",
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
        let (content, encoding_label) = match read_text_for_edit(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let eol = dominant_eol(&content);
        let adapted = adapt_text_to_eol(new_string, eol);
        let needs_newline = !content.is_empty() && !content.ends_with('\n');
        let to_append = if needs_newline {
            format!("{eol}{adapted}")
        } else {
            adapted
        };
        let new_content = format!("{content}{to_append}");

        match self
            .dispatch_full_file_rewrite_encoded(
                resolved_target,
                &content,
                &new_content,
                encoding_label,
            )
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
        near_line: Option<u64>,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let (content, encoding_label) = match read_text_for_edit(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let (insert_pos, recovery_note) = match locate_insert_anchor(
            &content,
            pattern,
            near_line,
            resolved_target,
            display_path,
            InsertSide::After,
        ) {
            Ok(located) => located,
            Err(error) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(error),
                });
            }
        };

        let new_string =
            adapt_replacement_eol(pattern, new_string, dominant_eol(&content));
        let new_string = new_string.as_str();
        let new_content = format!(
            "{}{}{}",
            &content[..insert_pos],
            new_string,
            &content[insert_pos..]
        );

        match self
            .dispatch_full_file_rewrite_encoded(
                resolved_target,
                &content,
                &new_content,
                encoding_label,
            )
            .await
        {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!(
                    "Inserted after pattern in {display_path}{recovery_note}:\n```\n{}\n```",
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
        near_line: Option<u64>,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let (content, encoding_label) = match read_text_for_edit(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let (insert_pos, recovery_note) = match locate_insert_anchor(
            &content,
            pattern,
            near_line,
            resolved_target,
            display_path,
            InsertSide::Before,
        ) {
            Ok(located) => located,
            Err(error) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(error),
                });
            }
        };

        let new_string =
            adapt_replacement_eol(pattern, new_string, dominant_eol(&content));
        let new_string = new_string.as_str();
        let new_content = format!(
            "{}{}{}",
            &content[..insert_pos],
            new_string,
            &content[insert_pos..]
        );

        match self
            .dispatch_full_file_rewrite_encoded(
                resolved_target,
                &content,
                &new_content,
                encoding_label,
            )
            .await
        {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!(
                    "Inserted before pattern in {display_path}{recovery_note}:\n```\n{}\n```",
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

    fn generate_diff(
        &self,
        path: &str,
        old_string: &str,
        new_string: &str,
        base_line: usize,
    ) -> String {
        use similar::TextDiff;
        const MAX_TOOL_DIFF_PAYLOAD: usize = 64 * 1024;
        let line_offset = base_line.saturating_sub(1);
        let diff = TextDiff::from_lines(old_string, new_string);
        let mut out = format!("--- a/{path}\n+++ b/{path}\n");
        for group in diff.grouped_ops(3).iter() {
            let (first, last) = (group.first(), group.last());
            let (Some(first), Some(last)) = (first, last) else {
                continue;
            };
            let old_start = first.old_range().start;
            let old_end = last.old_range().end;
            let new_start = first.new_range().start;
            let new_end = last.new_range().end;
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                old_start + 1 + line_offset,
                old_end - old_start,
                new_start + 1 + line_offset,
                new_end - new_start,
            ));
            for op in group {
                for change in diff.iter_changes(op) {
                    let sign = match change.tag() {
                        similar::ChangeTag::Delete => '-',
                        similar::ChangeTag::Insert => '+',
                        similar::ChangeTag::Equal => ' ',
                    };
                    out.push(sign);
                    out.push_str(change.value());
                    if !change.value().ends_with('\n') {
                        out.push('\n');
                    }
                    if out.len() > MAX_TOOL_DIFF_PAYLOAD {
                        crate::util::truncate_string_bytes(&mut out, MAX_TOOL_DIFF_PAYLOAD);
                        out.push_str("\n... (diff truncated)\n");
                        return out;
                    }
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy)]
enum InsertSide {
    After,
    Before,
}

fn locate_insert_anchor(
    content: &str,
    pattern: &str,
    near_line: Option<u64>,
    resolved_target: &std::path::Path,
    display_path: &str,
    side: InsertSide,
) -> Result<(usize, &'static str), String> {
    const MAX_ANCHOR_HITS: usize = 5000;
    let finder = Finder::new(pattern.as_bytes());
    let hits: Vec<usize> = finder
        .find_iter(content.as_bytes())
        .take(MAX_ANCHOR_HITS)
        .collect();
    if hits.len() >= MAX_ANCHOR_HITS {
        return Err(format!(
            "Pattern matches {MAX_ANCHOR_HITS}+ times; it is too generic to anchor an insert \
             reliably. Include more surrounding lines in old_string to make it unique."
        ));
    }

    let pick = |start: usize, end: usize| match side {
        InsertSide::After => end,
        InsertSide::Before => start,
    };

    if hits.len() == 1 {
        return Ok((pick(hits[0], hits[0] + pattern.len()), ""));
    }
    if hits.len() > 1 {
        if let Some(anchor_line) = near_line {
            if let Some(best) = hits.iter().copied().min_by_key(|pos| {
                let (line_no, _) = locate_line(content, *pos);
                (line_no as i64 - anchor_line as i64).unsigned_abs()
            }) {
                return Ok((
                    pick(best, best + pattern.len()),
                    " [disambiguated by near_line: nearest match selected]",
                ));
            }
        }
        let mut msg = format!(
            "Pattern matches {} times; must match exactly once. Showing first 3 hit locations:\n",
            hits.len()
        );
        for hit in hits.iter().take(3) {
            let (line_no, line) = locate_line(content, *hit);
            msg.push_str(&format!("  - line {line_no} : {line}\n"));
        }
        msg.push_str(
            "Include more surrounding lines in old_string to disambiguate, or pass \
             near_line=<line number> to anchor the intended match.",
        );
        return Err(msg);
    }

    let eol_spans = find_eol_insensitive_spans(content, pattern, usize::MAX);
    if eol_spans.len() == 1 {
        return Ok((pick(eol_spans[0].start, eol_spans[0].end), ""));
    }
    if eol_spans.len() > 1 {
        if let Some(anchor_line) = near_line {
            if let Some(span) = eol_spans.iter().copied().min_by_key(|span| {
                let (line_no, _) = locate_line(content, span.start);
                (line_no as i64 - anchor_line as i64).unsigned_abs()
            }) {
                return Ok((
                    pick(span.start, span.end),
                    " [disambiguated by near_line: nearest match selected]",
                ));
            }
        }
        let mut msg = format!(
            "Pattern matches {} times after line-ending normalization; must match exactly \
             once. Showing first 3 hit locations:\n",
            eol_spans.len()
        );
        for span in eol_spans.iter().take(3) {
            let (line_no, line) = locate_line(content, span.start);
            msg.push_str(&format!("  - line {line_no} : {line}\n"));
        }
        msg.push_str(
            "Include more surrounding lines in old_string to disambiguate, or pass \
             near_line=<line number> to anchor the intended match.",
        );
        return Err(msg);
    }

    if let Some((ws_start, ws_end, _)) =
        super::match_diagnostics::find_whitespace_insensitive_unique(content, pattern, pattern)
    {
        return Ok((
            pick(ws_start, ws_end),
            " [auto-recovered a whitespace/indentation mismatch in the anchor pattern]",
        ));
    }

    let had_read = crate::session::has_read_in_current_session(resolved_target);
    Err(super::match_diagnostics::failure_message(
        content,
        pattern,
        display_path,
        had_read,
    ))
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
