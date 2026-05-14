// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::apply_model::{ApplyBatchError, EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn resolve_and_validate_path_sync_with(
    security: &SecurityPolicy,
    path: &str,
) -> anyhow::Result<PathBuf> {
    let expanded = security.resolve_tool_path(path);

    let parent = expanded
        .parent()
        .context("Invalid path: missing parent directory")?;

    let resolved_parent = std::fs::canonicalize(parent)?;
    if !security.is_resolved_path_allowed(&resolved_parent) {
        anyhow::bail!(
            "Path escapes workspace boundary: {}",
            security.resolved_path_violation_message(&resolved_parent)
        );
    }

    let file_name = expanded
        .file_name()
        .context("Invalid path: missing file name")?;
    let resolved_target = resolved_parent.join(file_name);

    if security.is_runtime_config_path(&resolved_target) {
        anyhow::bail!(
            "Refusing to modify runtime config/state file: {}",
            security.runtime_config_violation_message(&resolved_target)
        );
    }

    Ok(resolved_target)
}

pub struct PatchApplyTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Arc<OpsApplier>,
}

impl PatchApplyTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        let ops_applier =
            Arc::new(OpsApplier::default_for_shared_workspace(security.workspace_root_handle()));
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

    fn validate_path_allowed(&self, path: &str) -> anyhow::Result<()> {
        if !self.security.is_path_allowed(path) {
            anyhow::bail!("Path not allowed by security policy: {path}");
        }
        Ok(())
    }

    fn resolve_and_validate_path_sync(&self, path: &str) -> anyhow::Result<PathBuf> {
        resolve_and_validate_path_sync_with(&self.security, path)
    }

    async fn verify_no_symlink(&self, path: &Path) -> anyhow::Result<()> {
        if let Ok(meta) = tokio::fs::symlink_metadata(path).await {
            if meta.file_type().is_symlink() {
                anyhow::bail!(
                    "Refusing to apply patch through symlink: {}",
                    path.display()
                );
            }
        }

        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            if let Ok(meta) = tokio::fs::symlink_metadata(parent).await {
                if meta.file_type().is_symlink() {
                    anyhow::bail!(
                        "Refusing to apply patch through symlinked parent directory: {}",
                        parent.display()
                    );
                }
            }
            current = parent.to_path_buf();
        }

        Ok(())
    }

    fn parse_patch(&self, patch_content: &str) -> anyhow::Result<Vec<PatchFile>> {
        parse_patch_with(&self.security, patch_content)
    }
}

fn parse_patch_with(
    security: &SecurityPolicy,
    patch_content: &str,
) -> anyhow::Result<Vec<PatchFile>> {
    let mut files: Vec<PatchFile> = Vec::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut hunks_in_current: usize = 0;
    let mut current_hunk_starts: Vec<u32> = Vec::new();

    let mut iter = patch_content.lines().peekable();
    while let Some(line) = iter.next() {
        if line.starts_with("--- ") || line.starts_with("diff ") {
            if let Some(path) = current_path.take() {
                files.push(PatchFile {
                    path,
                    diff_text: current_lines.join("\n"),
                    hunks: current_hunk_starts
                        .iter()
                        .map(|s| PatchHunk { old_start: *s })
                        .collect(),
                });
            }
            current_lines.clear();
            current_hunk_starts.clear();
            hunks_in_current = 0;

            let file_path = if line.starts_with("--- ") {
                line.strip_prefix("--- ").unwrap().trim_start_matches("a/")
            } else {
                line.strip_prefix("diff ")
                    .unwrap()
                    .trim_start_matches("--- ")
            };

            current_path = Some(resolve_and_validate_path_sync_with(security, file_path)?);
            current_lines.push(line.to_string());
            continue;
        }

        if current_path.is_some() {
            if let Some(rest) = line.strip_prefix("@@ ") {
                if let Some(end) = rest.find(" @@") {
                    let parts: Vec<&str> = rest[..end].split_whitespace().collect();
                    if let Some(old_token) = parts.first() {
                        let old_start = old_token
                            .trim_start_matches(['+', '-'])
                            .split(',')
                            .next()
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(1);
                        current_hunk_starts.push(old_start);
                        hunks_in_current += 1;
                    }
                }
            }
            current_lines.push(line.to_string());
        }
    }

    if let Some(path) = current_path.take() {
        files.push(PatchFile {
            path,
            diff_text: current_lines.join("\n"),
            hunks: current_hunk_starts
                .iter()
                .map(|s| PatchHunk { old_start: *s })
                .collect(),
        });
    }
    let _ = hunks_in_current;

    Ok(files)
}

#[derive(Debug)]
struct PatchFile {
    path: PathBuf,

    diff_text: String,

    hunks: Vec<PatchHunk>,
}

#[derive(Debug)]
struct PatchHunk {
    old_start: u32,
}

#[async_trait]
impl Tool for PatchApplyTool {
    fn name(&self) -> &str {
        "patch_apply"
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to files. Supports viewing patch statistics, \
         dry-run mode, and applying patches. By default the entire patch is \
         applied atomically: any hunk failure rolls back the rest. Pass \
         atomic=false to keep partial successes (the result will be marked \
         degraded=true)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "The unified diff patch content (patch format)"
                },
                "action": {
                    "type": "string",
                    "description": "Action to perform",
                    "enum": ["apply", "stats", "preview"]
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would be changed without applying (default: false)"
                },
                "atomic": {
                    "type": "boolean",
                    "description": "If true (default), the whole patch succeeds or rolls back. If false, hunks that succeed remain on disk and the result is marked degraded."
                }
            },
            "required": ["patch", "action"]
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

        let cli_dry_run = std::env::var("SEN_DRY_RUN").as_deref() == Ok("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        let atomic = args
            .get("atomic")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let patch_content = args
            .get("patch")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'patch' parameter"))?;

        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let patch_files = {
            let security_arc = Arc::clone(&self.security);
            let patch_content_owned = patch_content.to_string();
            tokio::task::spawn_blocking(move || {
                parse_patch_with(&security_arc, &patch_content_owned)
            })
            .await
            .map_err(|e| anyhow::anyhow!("patch_apply parse join error: {e}"))??
        };

        if patch_files.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid or empty patch content".into()),
            });
        }

        for file in &patch_files {
            if let Err(e) = self.validate_path_allowed(&file.path.to_string_lossy()) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Security check failed: {e}")),
                });
            }
        }

        match action {
            "stats" => {
                let mut stats = vec![format!("Patch contains {} file(s):\n", patch_files.len())];
                for file in &patch_files {
                    stats.push(format!(
                        "  {} ({} hunk(s))",
                        file.path.display(),
                        file.hunks.len()
                    ));
                }
                Ok(ToolResult {
                    success: true,
                    output: stats.join("\n"),
                    error: None,
                })
            }
            "preview" | "apply" => {
                if action == "apply" && !self.security.record_action() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("Rate limit exceeded: action budget exhausted".into()),
                    });
                }

                for file in &patch_files {
                    if let Err(e) = self.verify_no_symlink(&file.path).await {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Security check failed: {e}")),
                        });
                    }
                }

                const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
                for file in &patch_files {
                    if let Ok(meta) = tokio::fs::metadata(&file.path).await {
                        if meta.len() > MAX_FILE_SIZE {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "File too large: {} ({:.1} MB > 10 MB)",
                                    file.path.display(),
                                    meta.len() as f64 / (1024.0 * 1024.0)
                                )),
                            });
                        }
                    }
                }

                let planned_paths: Vec<std::path::PathBuf> =
                    patch_files.iter().map(|f| f.path.clone()).collect();
                let _resource_guards = if action == "apply" {
                    match crate::session::acquire_many_file_writes_for_current_session(
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
                    }
                } else {
                    None
                };

                if action == "apply" {
                    for p in &planned_paths {
                        if crate::session::is_stale_for_current_session(p) {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(crate::session::stale_file_error_message(p)),
                            });
                        }
                    }
                }

                let mut batch = EditBatch::new(EditOrigin::PatchTool).with_atomic(atomic);
                for file in &patch_files {
                    batch.push(EditOp::ApplyHunk {
                        path: file.path.clone(),
                        diff: file.diff_text.clone(),
                        fuzz: 3,
                        scope_anchor: None,
                    });
                }

                let total_hunks: usize = patch_files.iter().map(|f| f.hunks.len()).sum();

                if dry_run || action == "preview" {
                    match self.ops_applier.dry_run(&batch).await {
                        Ok(preview) => {
                            let mut out = vec![format!(
                                "[DRY RUN] Would apply patch to {} file(s):\n",
                                patch_files.len()
                            )];
                            for diff in &preview.diffs {
                                out.push(format!("  {}", diff.path.display()));
                            }
                            out.push(format!(
                                "\n[DRY RUN] {} hunk(s) would be applied",
                                total_hunks
                            ));
                            Ok(ToolResult {
                                success: true,
                                output: out.join("\n"),
                                error: None,
                            })
                        }
                        Err(e) => Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Patch preview failed: {e}")),
                        }),
                    }
                } else {
                    match self.ops_applier.apply_batch(batch).await {
                        Ok(outcome) => {
                            let mut applied = 0usize;
                            let mut failed = 0usize;
                            let mut details: Vec<String> =
                                Vec::with_capacity(outcome.per_op.len());
                            for (idx, op) in outcome.per_op.iter().enumerate() {
                                let path = patch_files
                                    .get(idx)
                                    .map(|f| f.path.display().to_string())
                                    .unwrap_or_else(|| op.touched_path.display().to_string());
                                if op.success {
                                    applied += 1;
                                    crate::session::record_write_for_current_session(
                                        &op.touched_path,
                                    );
                                    details.push(format!(
                                        "  Successfully applied hunk(s) to {path}"
                                    ));
                                } else {
                                    failed += 1;
                                    details.push(format!(
                                        "  Failed to apply hunk(s) to {path}: {}",
                                        op.error.as_deref().unwrap_or("unknown")
                                    ));
                                }
                            }

                            let mut out =
                                vec![format!("Applying patch to {} file(s):\n", patch_files.len())];
                            out.extend(details);
                            out.push(format!(
                                "\nApplied {}/{} file(s) ({} failed){}",
                                applied,
                                patch_files.len(),
                                failed,
                                if outcome.degraded { ", degraded=true" } else { "" },
                            ));
                            if let Some(jp) = outcome.journal_path.as_ref() {
                                out.push(format!("Journal: {}", jp.display()));
                            }
                            let success = failed == 0;
                            Ok(ToolResult {
                                success,
                                output: out.join("\n"),
                                error: None,
                            })
                        }
                        Err(ApplyBatchError::Hunk { source, path, .. }) => Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Failed to apply patch to {}: {source}",
                                path.display()
                            )),
                        }),
                        Err(e) => Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Patch apply failed: {e}")),
                        }),
                    }
                }
            }
            _ => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action: '{}'. Use 'apply', 'stats', or 'preview'.",
                    action
                )),
            }),
        }
    }
}
