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
                            },
                            "expected_mtime_ms": {
                                "type": "integer",
                                "description": "Optional file mtime (ms since epoch, from file_read) for conflict detection; the edit is rejected if the file changed since."
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

            let resolved_parent = crate::util::normalize_path_for_containment(parent);

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

        let mut file_order: Vec<std::path::PathBuf> = Vec::new();
        let mut originals: std::collections::HashMap<std::path::PathBuf, Option<String>> =
            std::collections::HashMap::new();
        let mut currents: std::collections::HashMap<std::path::PathBuf, String> =
            std::collections::HashMap::new();
        let mut edit_counts: std::collections::HashMap<std::path::PathBuf, usize> =
            std::collections::HashMap::new();

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

            if !originals.contains_key(&path) {
                let existing = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => Some(c),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Edit {i}: cannot read '{}': {e}", path.display())),
                        });
                    }
                };
                file_order.push(path.clone());
                if let Some(ref c) = existing {
                    currents.insert(path.clone(), c.clone());
                }
                originals.insert(path.clone(), existing);
            }

            let next_content = if let Some(old) = old_string {
                if old.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: old_string must not be empty; omit old_string to write \
                             full file content instead"
                        )),
                    });
                }
                let Some(content) = currents.get(&path) else {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: file '{}' does not exist",
                            path.display()
                        )),
                    });
                };
                let count = content.matches(old).count();
                if count == 0 {
                    let had_read = crate::session::has_read_in_current_session(&path);
                    let detail = super::super::file::match_diagnostics::failure_message(
                        content,
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
                content.replacen(old, new_string, 1)
            } else {
                new_string.to_string()
            };
            currents.insert(path.clone(), next_content);
            *edit_counts.entry(path).or_insert(0) += 1;
        }

        let mut summary_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut emit_records: Vec<(Option<Vec<u8>>, Vec<u8>)> = Vec::new();
        for path in file_order {
            let original = originals.remove(&path).flatten();
            let Some(final_content) = currents.remove(&path) else {
                continue;
            };

            if let Some(sentinel) =
                crate::agent::profile::pii_sanitize::introduced_redaction_sentinel(
                    original.as_deref(),
                    &final_content,
                )
            {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        crate::agent::profile::pii_sanitize::redaction_writeback_error(
                            sentinel,
                            &path.display().to_string(),
                        ),
                    ),
                });
            }

            self.snapshot_before_write(&path).await;

            emit_records.push((
                original.as_ref().map(|s| s.as_bytes().to_vec()),
                final_content.as_bytes().to_vec(),
            ));
            let op = match original {
                Some(orig) => EditOp::Replace {
                    path: path.clone(),
                    byte_range: 0..orig.len(),
                    old_text: orig,
                    new_text: final_content,
                    anchor: None,
                },
                None => EditOp::CreateFile {
                    path: path.clone(),
                    contents: final_content,
                    overwrite: true,
                    encoding: None,
                },
            };
            batch.push(op);
            summary_paths.push(path);
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
                let total_edits: usize = edit_counts.values().sum();
                let mut summary: Vec<String> = Vec::with_capacity(summary_paths.len());
                for p in &summary_paths {
                    let n = edit_counts.get(p).copied().unwrap_or(1);
                    let mtime_note = match tokio::fs::metadata(p).await {
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
                    if n > 1 {
                        summary.push(format!(
                            "  \u{2713} {} ({n} edits){mtime_note}",
                            p.display()
                        ));
                    } else {
                        summary.push(format!("  \u{2713} {}{mtime_note}", p.display()));
                    }
                }
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Applied {} edit(s) across {} file(s) atomically:\n{}",
                        total_edits,
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
