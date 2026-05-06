// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `multi_edit` tool — routes the cross-file batch through
//! [`crate::apply_model::OpsApplier`] (`atomic=true`) so the previous
//! hand-rolled "validate → write → rollback on failure" pipeline is
//! replaced by the journal-backed engine shared with every other
//! editing surface.  The legacy mtime / symlink / autonomy checks are
//! preserved verbatim because they describe surface-level pre-conditions
//! that OpsApplier intentionally does not duplicate.
use super::edit_history::EditHistory;
use super::traits::{Tool, ToolResult};
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
        "Apply edits to multiple files atomically. All edits succeed or none are applied. \
         Each edit specifies a file path and either old_string/new_string replacement \
         or full content to write."
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

        let mut batch = EditBatch::new(EditOrigin::MultiEditTool).with_atomic(true);
        let mut summary_paths: Vec<std::path::PathBuf> = Vec::new();

        for (i, edit) in edits.iter().enumerate() {
            let path_str = edit
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'path'"))?;
            let new_string = edit
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'new_string'"))?;
            let old_string = edit.get("old_string").and_then(|v| v.as_str());
            let expected_mtime_ms = edit
                .get("expected_mtime_ms")
                .and_then(|v| v.as_i64())
                .map(|v| v as u64);

            if !self.security.is_path_allowed(path_str) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: security policy blocked path '{}'",
                        path_str
                    )),
                });
            }

            let path = std::path::PathBuf::from(path_str);

            if tokio::fs::symlink_metadata(&path)
                .await
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: refusing to edit through symlink '{}'",
                        path.display()
                    )),
                });
            }

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
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: old_string not found in '{}'",
                            path.display()
                        )),
                    });
                }
                if count > 1 {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: old_string matches {count} times in '{}' (use exact string to disambiguate)",
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

            let op = match original {
                Some(orig) => EditOp::Replace {
                    path: path.clone(),
                    byte_range: 0..orig.as_bytes().len(),
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
        }

        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
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
