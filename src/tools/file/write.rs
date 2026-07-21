// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::edit_history::EditHistory;
use super::super::traits::{Tool, ToolResult};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct FileWriteTool {
    security: Arc<SecurityPolicy>,
    edit_history: Option<Arc<EditHistory>>,
    ops_applier: Arc<OpsApplier>,
}

impl FileWriteTool {
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
                    "file_write".to_string(),
                    "write file".to_string(),
                )
                .await;
        }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write contents to a file in the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Relative paths resolve from workspace; outside paths require policy allowlist."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                },
                "expected_mtime_ms": {
                    "type": "integer",
                    "description": "Expected file modification time in milliseconds since epoch. \
                                    If the file exists and has been modified since this timestamp, \
                                    the write is rejected to prevent overwriting manual edits. \
                                    Obtain this value from a prior file_read result."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        let expected_mtime_ms = args
            .get("expected_mtime_ms")
            .and_then(|v| v.as_i64())
            .map(|v| v as u64);

        const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024;
        if content.len() > MAX_WRITE_SIZE {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Content too large: {} bytes exceeds 10 MB limit",
                    content.len()
                )),
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

        let resolved_parent = crate::util::normalize_path_for_containment(parent);

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
                        "Refusing to write through symlink: {}",
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

        if let Some(expected) = expected_mtime_ms {
            if let Ok(meta) = tokio::fs::metadata(&resolved_target).await {
                if let Ok(modified) = meta.modified() {
                    if let Ok(current) = modified.duration_since(std::time::UNIX_EPOCH) {
                        if current.as_millis() as u64 != expected {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "File '{}' was modified externally (expected mtime {}, found {}). \
                                     Re-read the file and retry with updated content.",
                                    path,
                                    expected,
                                    current.as_millis() as u64
                                )),
                            });
                        }
                    }
                }
            }
        }

        self.snapshot_before_write(&resolved_target).await;

        let existed = tokio::fs::metadata(&resolved_target).await.is_ok();
        let original_bytes: Option<Vec<u8>> = if existed {
            tokio::fs::read(&resolved_target).await.ok()
        } else {
            None
        };
        let original_text: Option<String> = original_bytes
            .as_deref()
            .and_then(|b| std::str::from_utf8(b).ok().map(str::to_string));

        if let Some(sentinel) = crate::agent::profile::pii_sanitize::introduced_redaction_sentinel(
            original_text.as_deref(),
            content,
        ) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(crate::agent::profile::pii_sanitize::redaction_writeback_error(
                    sentinel, path,
                )),
            });
        }
        // When the file already exists in a non-UTF-8 encoding (e.g. GBK), preserve
        // that encoding on write so we don't corrupt it by forcing UTF-8 bytes.
        let preserved_encoding: Option<String> = match (&original_bytes, &original_text) {
            (Some(bytes), None) => {
                let label = crate::tools::file::encoding::detect_label(bytes);
                if crate::tools::file::encoding::is_utf8_label(label) {
                    None
                } else {
                    Some(label.to_string())
                }
            }
            _ => None,
        };
        let op = match (existed, original_text) {
            (true, Some(original_text)) => {
                let original_len = original_text.len();
                EditOp::Replace {
                    path: resolved_target.clone(),
                    byte_range: 0..original_len,
                    old_text: original_text,
                    new_text: content.to_string(),
                    anchor: None,
                }
            }
            _ => EditOp::CreateFile {
                path: resolved_target.clone(),
                contents: content.to_string(),
                overwrite: true,
                encoding: preserved_encoding,
            },
        };
        let batch = EditBatch::new(EditOrigin::FileWriteTool).with_op(op);
        let batch_id = batch.batch_id.clone();
        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
                crate::session::record_write_for_current_session(&resolved_target);
                crate::agent::file_edit_emitter::emit_file_edit(
                    &resolved_target,
                    original_bytes.as_deref(),
                    Some(content.as_bytes()),
                    Some(batch_id),
                )
                .await;
                let preview_lines: Vec<&str> = content.lines().take(10).collect();
                let suffix = if content.lines().count() > 10 {
                    format!("\n... ({} more lines)", content.lines().count() - 10)
                } else {
                    String::new()
                };
                let preview = preview_lines.join("\n");
                let mtime_note = match tokio::fs::metadata(&resolved_target).await {
                    Ok(meta) => meta
                        .modified()
                        .ok()
                        .and_then(|t| {
                            t.duration_since(std::time::UNIX_EPOCH).ok()
                        })
                        .map(|d| format!(" [mtime_ms: {}]", d.as_millis() as u64))
                        .unwrap_or_default(),
                    Err(_) => String::new(),
                };
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Written {} bytes to {path}{mtime_note}\n+++ b/{path}\n{preview}{suffix}",
                        content.len()
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
}
