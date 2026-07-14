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

pub struct MultiEditTool {
    security: Arc<SecurityPolicy>,
    edit_history: Option<Arc<EditHistory>>,
    ops_applier: Arc<OpsApplier>,
}

impl MultiEditTool {
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
                    "multi_edit".to_string(),
                    "atomic batch edit".to_string(),
                )
                .await;
        }
    }
}

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "multi_edit"
    }

    fn description(&self) -> &str {
        "Apply SEVERAL exact-string edits atomically in one call (same file or across files): \
         all succeed or none are applied. Each edit gives a path plus old_string/new_string \
         (omit old_string to write full content). Use this instead of chained file_edit calls \
         when changes belong together; for diff-formatted changes use diff_apply."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "description": "Array of file edits to apply atomically",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path to the file to edit"
                            },
                            "old_string": {
                                "type": "string",
                                "description": "Text to find and replace (if omitted, writes full content)"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement text (or full file content if old_string is omitted)"
                            }
                        },
                        "required": ["path", "new_string"]
                    }
                }
            },
            "required": ["edits"]
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

        let edits = args
            .get("edits")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'edits' array"))?;

        if edits.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("No edits provided".into()),
            });
        }

        let mut planned_paths: Vec<std::path::PathBuf> = Vec::with_capacity(edits.len());
        for (i, edit) in edits.iter().enumerate() {
            let path_str = edit
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'path'"))?;

            if !self.security.is_path_allowed(path_str) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: security policy blocked path '{path_str}'"
                    )),
                });
            }

            let full_path = self.security.resolve_tool_path(path_str);

            let Some(parent) = full_path.parent() else {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Edit {i}: invalid path (missing parent directory)")),
                });
            };

            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: cannot create parent directory for '{}': {e}",
                        full_path.display()
                    )),
                });
            }

            let resolved_parent = match tokio::fs::canonicalize(parent).await {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Edit {i}: failed to resolve file path: {e}")),
                    });
                }
            };

            if !self.security.is_resolved_path_allowed(&resolved_parent) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: {}",
                        self.security
                            .resolved_path_violation_message(&resolved_parent)
                    )),
                });
            }

            if !crate::security::sandbox_allows_path(&resolved_parent) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: sandbox policy denies write to {}",
                        resolved_parent.display()
                    )),
                });
            }

            let Some(file_name) = full_path.file_name() else {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Edit {i}: invalid path (missing file name)")),
                });
            };

            let resolved_target = resolved_parent.join(file_name);

            if self.security.is_runtime_config_path(&resolved_target) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: {}",
                        self.security
                            .runtime_config_violation_message(&resolved_target)
                    )),
                });
            }

            if let Ok(meta) = tokio::fs::symlink_metadata(&resolved_target).await {
                if meta.file_type().is_symlink() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: refusing to edit through symlink '{}'",
                            resolved_target.display()
                        )),
                    });
                }
            }

            planned_paths.push(resolved_target);
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let mut batch = EditBatch::new(EditOrigin::MultiEditTool).with_atomic(true);
        let mut summary_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut emit_records: Vec<(Option<Vec<u8>>, Vec<u8>)> = Vec::new();
        let _resource_guards = match crate::session::acquire_many_file_writes_for_current_session(
            planned_paths.clone(),
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

        for p in &planned_paths {
            if crate::session::is_stale_for_current_session(p) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(crate::session::stale_file_error_message(p)),
                });
            }
        }

        for (i, edit) in edits.iter().enumerate() {
            let new_string = edit
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'new_string'"))?;
            let old_string = edit.get("old_string").and_then(|v| v.as_str());
            let expected_mtime_ms = edit
                .get("expected_mtime_ms")
                .and_then(|v| v.as_i64())
                .map(|v| v as u64);

            let path = planned_paths[i].clone();

            if let Some(expected) = expected_mtime_ms {
                let actual_mtime = tokio::fs::metadata(&path)
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64);
                if let Some(actual) = actual_mtime
                    && actual != expected
                {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: file '{}' was modified externally (expected mtime {}, found {}). \
                             Re-read the file and retry with the updated content.",
                            path.display(),
                            expected,
                            actual
                        )),
                    });
                }
            }

            let (original, new_content) = if let Some(old) = old_string {
                let content = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Edit {i}: file '{}' does not exist",
                                path.display()
                            )),
                        });
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Edit {i}: cannot read '{}': {e}", path.display())),
                        });
                    }
                };
                let count = content.matches(old).count();
                if count == 0 {
                    let had_read = crate::session::has_read_in_current_session(&path);
                    let detail = super::super::file::match_diagnostics::failure_message(
                        &content,
                        old,
                        &path.display().to_string(),
                        had_read,
                    );
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Edit {i}: {detail}")),
                    });
                }
                if count > 1 {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: old_string matches {count} times in '{}'. Include more \
                             surrounding lines (a longer, unique old_string) so exactly one \
                             location is targeted.",
                            path.display()
                        )),
                    });
                }
                let new_content = content.replacen(old, new_string, 1);
                (Some(content), new_content)
            } else {
                let existing = tokio::fs::read_to_string(&path).await.ok();
                (existing, new_string.to_string())
            };

            self.snapshot_before_write(&path).await;

            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }

            let mut emit_payload: (Option<Vec<u8>>, Vec<u8>) = (
                original.as_ref().map(|s| s.as_bytes().to_vec()),
                new_content.as_bytes().to_vec(),
            );
            let op = match original {
                Some(orig) => EditOp::Replace {
                    path: path.clone(),
                    byte_range: 0..orig.len(),
                    old_text: orig,
                    new_text: new_content,
                    anchor: None,
                },
                None => EditOp::CreateFile {
                    path: path.clone(),
                    contents: new_content,
                    overwrite: true,
                },
            };
            batch.push(op);
            summary_paths.push(path);
            emit_records.push(std::mem::take(&mut emit_payload));
        }

        let emit_records_for_apply = emit_records;
        let batch_id_for_emit = batch.batch_id.clone();
        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
                for (p, (before, after)) in
                    summary_paths.iter().zip(emit_records_for_apply.into_iter())
                {
                    crate::session::record_write_for_current_session(p);
                    crate::agent::file_edit_emitter::emit_file_edit(
                        p,
                        before.as_deref(),
                        Some(after.as_slice()),
                        Some(batch_id_for_emit.clone()),
                    )
                    .await;
                }
                let summary: Vec<String> = summary_paths
                    .iter()
                    .map(|p| format!("  \u{2713} {}", p.display()))
                    .collect();
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Applied {} edit(s) atomically:\n{}",
                        summary_paths.len(),
                        summary.join("\n")
                    ),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Multi-edit failed (rolled back): {e}")),
            }),
        }
    }
}
