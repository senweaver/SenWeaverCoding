// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct RestoreFileTool {
    security: Arc<SecurityPolicy>,
    edit_history: Option<Arc<super::edit_history::EditHistory>>,
}

impl RestoreFileTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self {
            security,
            edit_history: None,
        }
    }

    #[must_use]
    pub fn with_edit_history(
        mut self,
        history: Arc<super::edit_history::EditHistory>,
    ) -> Self {
        self.edit_history = Some(history);
        self
    }
}

#[async_trait]
impl Tool for RestoreFileTool {
    fn name(&self) -> &str {
        "restore_file"
    }

    fn description(&self) -> &str {
        "Restore a file to its previous state. By default reverts to the latest edit-history \
         snapshot (works offline, undoes the most recent edit; the pre-restore state is \
         stashed so the restore itself can be undone). Pass revision=<git rev> to restore \
         from git instead."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to restore (relative to workspace)"
                },
                "revision": {
                    "type": "string",
                    "description": "Optional git revision to restore from (e.g. HEAD). When omitted, the latest edit-history snapshot is used and git is only a fallback."
                }
            },
            "required": ["path"]
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

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let revision_explicit = args
            .get("revision")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if !self.security.is_path_allowed(path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        let full_path = self.security.resolve_tool_path(path);

        if let Ok(meta) = tokio::fs::symlink_metadata(&full_path).await {
            if meta.file_type().is_symlink() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to restore through symlink: {}",
                        full_path.display()
                    )),
                });
            }
        }
        let canonical = tokio::fs::canonicalize(&full_path)
            .await
            .unwrap_or_else(|_| full_path.clone());
        if !self.security.is_resolved_path_allowed(&canonical) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(self.security.resolved_path_violation_message(&canonical)),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        let before_bytes = tokio::fs::read(&full_path).await.ok();

        let mut history_note = String::new();
        if revision_explicit.is_none() {
            if let Some(history) = self.edit_history.as_ref() {
                let snapshots = history.file_history_with_batches(&full_path);
                if snapshots.is_empty() {
                    history_note =
                        " (no edit-history snapshot exists for this file; fell back to git)"
                            .to_string();
                } else {
                    let last_index = snapshots.len() - 1;
                    let history_for_task = Arc::clone(history);
                    let path_for_task = full_path.clone();
                    let restore = tokio::task::spawn_blocking(move || {
                        history_for_task.restore_snapshot_with_stash(
                            &path_for_task,
                            last_index,
                            None,
                            "restore_file",
                            "stash current state before snapshot restore",
                        )
                    })
                    .await;
                    match restore {
                        Ok(Ok(())) => {
                            crate::session::record_write_for_current_session(&full_path);
                            let after_bytes = tokio::fs::read(&full_path).await.ok();
                            crate::agent::file_edit_emitter::emit_file_edit(
                                &full_path,
                                before_bytes.as_deref(),
                                after_bytes.as_deref(),
                                None,
                            )
                            .await;
                            return Ok(ToolResult {
                                success: true,
                                output: format!(
                                    "Restored {path} from edit-history snapshot #{last_index} \
                                     (state before the most recent edit). The pre-restore \
                                     content was stashed, so this restore can itself be \
                                     undone. Pass revision=<git rev> to restore from git \
                                     instead."
                                ),
                                error: None,
                            });
                        }
                        Ok(Err(e)) => {
                            history_note = format!(
                                " (edit-history restore failed: {e}; fell back to git)"
                            );
                        }
                        Err(e) => {
                            history_note = format!(
                                " (edit-history restore task failed: {e}; fell back to git)"
                            );
                        }
                    }
                }
            }
        }

        let revision = revision_explicit.unwrap_or("HEAD");
        let ws = self.security.workspace_dir();
        let workspace = full_path.parent().unwrap_or(ws.as_path());

        let mut cmd = crate::util::hidden_async_command("git");
        cmd.args(["checkout", revision, "--", &full_path.to_string_lossy()])
            .current_dir(workspace)
            .kill_on_drop(true);
        let timeout_secs = crate::services::try_get_services()
            .and_then(|s| s.config().pacing.tool_timeout_secs)
            .unwrap_or(120);
        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            cmd.output(),
        )
        .await
        {
            Ok(out) => out,
            Err(_) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "git checkout timed out after {timeout_secs}s while restoring {path}{history_note}"
                    )),
                });
            }
        };

        match output {
            Ok(out) if out.status.success() => {
                crate::session::record_write_for_current_session(&full_path);
                let after_bytes = tokio::fs::read(&full_path).await.ok();
                crate::agent::file_edit_emitter::emit_file_edit(
                    &full_path,
                    before_bytes.as_deref(),
                    after_bytes.as_deref(),
                    None,
                )
                .await;
                Ok(ToolResult {
                    success: true,
                    output: format!("Restored {path} from git revision {revision}{history_note}"),
                    error: None,
                })
            }
            Ok(out) => {
                let stderr = crate::util::decode_subprocess_bytes(&out.stderr);

                if stderr.contains("not a git repository") {
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Not a git repository and no usable edit-history snapshot for \
                             {path}{history_note}. Consider using file_read to check the \
                             current state, or edit_history tools to inspect snapshots."
                        )),
                    })
                } else {
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "git checkout failed: {}{history_note}",
                            stderr.trim()
                        )),
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute git: {e}{history_note}")),
            }),
        }
    }
}
