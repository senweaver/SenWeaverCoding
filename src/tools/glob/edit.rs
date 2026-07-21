// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use anyhow::Context;
use async_trait::async_trait;
use glob::glob as glob_pattern;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct GlobEditTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Arc<OpsApplier>,
}

impl GlobEditTool {
    const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

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

    async fn resolve_and_validate_path(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let parent = path
            .parent()
            .context("Invalid path: missing parent directory")?;

        let resolved_parent = tokio::fs::canonicalize(parent).await?;
        if !self.security.is_resolved_path_allowed(&resolved_parent) {
            anyhow::bail!(
                "Path escapes workspace boundary: {}",
                self.security
                    .resolved_path_violation_message(&resolved_parent)
            );
        }

        let file_name = path
            .file_name()
            .context("Invalid path: missing file name")?;
        let resolved = resolved_parent.join(file_name);

        if self.security.is_runtime_config_path(&resolved) {
            anyhow::bail!(
                "Refusing to modify runtime config/state file: {}",
                self.security.runtime_config_violation_message(&resolved)
            );
        }

        Ok(resolved)
    }

    async fn verify_no_symlink(&self, path: &Path) -> anyhow::Result<()> {
        if let Ok(meta) = tokio::fs::symlink_metadata(path).await
            && meta.file_type().is_symlink()
        {
            anyhow::bail!("Refusing to edit through symlink: {}", path.display());
        }

        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            if let Ok(meta) = tokio::fs::symlink_metadata(parent).await
                && meta.file_type().is_symlink()
            {
                anyhow::bail!(
                    "Refusing to edit through symlinked parent directory: {}",
                    parent.display()
                );
            }
            current = parent.to_path_buf();
        }

        Ok(())
    }

}

#[async_trait]
impl Tool for GlobEditTool {
    fn name(&self) -> &str {
        "glob_edit"
    }

    fn description(&self) -> &str {
        "Bulk find-and-replace of the SAME old_string across MANY files selected by a glob \
         pattern (e.g. src/**/*.rs), with optional content filter. Best for mechanical \
         renames/sweeps. For a handful of distinct edits use multi_edit; for one file use \
         file_edit."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts')"
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to find in each file"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "filter_contains": {
                    "type": "string",
                    "description": "Optional: only edit files that contain this string (filters before edit)"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would be changed without making edits (default: false)"
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum number of files to edit (default: 100)"
                }
            },
            "required": ["pattern", "old_string", "new_string"]
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

        let cli_dry_run = crate::util::get_runtime_var("SEN_DRY_RUN").as_deref() == Some("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' parameter"))?;

        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string' parameter"))?;

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string' parameter"))?;

        if old_string.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "old_string must not be empty: an empty pattern would insert new_string at \
                     every character boundary of every matched file"
                        .into(),
                ),
            });
        }

        let filter_contains = args.get("filter_contains").and_then(|v| v.as_str());

        let max_files = args
            .get("max_files")
            .and_then(|v| v.as_i64())
            .unwrap_or(100) as usize;

        let security_arc = Arc::clone(&self.security);
        let pattern_owned = pattern.to_string();
        let filter_contains_owned = filter_contains.map(|s| s.to_string());
        let (matches_outcome, total_found, files_to_edit_outcome) =
            tokio::task::spawn_blocking(move || -> (anyhow::Result<Vec<PathBuf>>, usize, Vec<PathBuf>) {
                let validate = if !security_arc.is_path_allowed(&pattern_owned) {
                    Err(anyhow::anyhow!(
                        "Path not allowed by security policy: {pattern_owned}"
                    ))
                } else {
                    Ok(())
                };
                if let Err(e) = validate {
                    return (Err(e), 0, Vec::new());
                }
                let root = security_arc.workspace_dir();
                let full_pattern =
                    if pattern_owned.starts_with('/') || pattern_owned.contains(':') {
                        pattern_owned.clone()
                    } else {
                        format!("{}/{}", root.display(), pattern_owned)
                    };
                let deadline = std::time::Instant::now()
                    + std::time::Duration::from_secs(super::GLOB_WALK_TIMEOUT_SECS);
                let matches: Vec<PathBuf> = match glob_pattern(&full_pattern) {
                    Ok(paths) => {
                        let mut collected: Vec<PathBuf> = Vec::new();
                        for entry in paths {
                            if std::time::Instant::now() >= deadline {
                                break;
                            }
                            let Ok(path) = entry else { continue };
                            // Prune build/vendor/VCS trees so a bulk sweep never edits or even
                            // stats files under node_modules/target/.git/etc.
                            if super::crosses_skip_dir(&path) {
                                continue;
                            }
                            if path.is_file() {
                                collected.push(path);
                            }
                        }
                        collected
                    }
                    Err(e) => return (Err(anyhow::anyhow!(e)), 0, Vec::new()),
                };
                let total = matches.len();
                let filtered: Vec<PathBuf> = if let Some(filter) = filter_contains_owned.as_deref() {
                    matches
                        .into_iter()
                        .filter(|path| {
                            match std::fs::metadata(path) {
                                Ok(meta) if meta.len() > GlobEditTool::MAX_FILE_SIZE => return false,
                                Ok(_) => {}
                                Err(_) => return false,
                            }
                            std::fs::read_to_string(path)
                                .map(|c| c.contains(filter))
                                .unwrap_or(false)
                        })
                        .take(max_files)
                        .collect()
                } else {
                    matches.into_iter().take(max_files).collect()
                };
                (Ok(filtered.clone()), total, filtered)
            })
            .await
            .map_err(|e| anyhow::anyhow!("glob_edit join error: {e}"))?;

        matches_outcome?;
        if total_found == 0 {
            return Ok(ToolResult {
                success: true,
                output: format!("No files found matching pattern: {}", pattern),
                error: None,
            });
        }

        let files_to_edit: Vec<PathBuf> = files_to_edit_outcome;

        if files_to_edit.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!(
                    "No files to edit after filtering. Pattern '{}' matched {} file(s).",
                    pattern, total_found
                ),
                error: None,
            });
        }

        if !dry_run && !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let mut resolved_paths: Vec<PathBuf> = Vec::with_capacity(files_to_edit.len());
        for path in &files_to_edit {
            let resolved = match self.resolve_and_validate_path(path).await {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Security validation failed for {}: {}",
                            path.display(),
                            e
                        )),
                    });
                }
            };
            if let Err(e) = self.verify_no_symlink(&resolved).await {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Security check failed for {}: {}",
                        resolved.display(),
                        e
                    )),
                });
            }
            resolved_paths.push(resolved);
        }

        let mut results: Vec<String> = Vec::new();
        results.push(format!(
            "Found {} file(s) matching pattern '{}'",
            files_to_edit.len(),
            pattern
        ));

        if dry_run {
            results.push(format!(
                "[DRY RUN] Would edit {} file(s):",
                files_to_edit.len()
            ));
            for path in &files_to_edit {
                results.push(format!("  - {}", path.display()));
            }
            return Ok(ToolResult {
                success: true,
                output: results.join("\n"),
                error: None,
            });
        }

        let _resource_guards =
            match crate::session::acquire_many_file_writes_for_current_session(
                resolved_paths.clone(),
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

        let mut batch = EditBatch::new(EditOrigin::GlobEditTool).with_atomic(true);
        let mut planned_paths: Vec<PathBuf> = Vec::new();
        let mut emit_records: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut size_skipped: Vec<String> = Vec::new();
        for resolved in &resolved_paths {
            if let Ok(meta) = tokio::fs::metadata(resolved).await
                && meta.len() > Self::MAX_FILE_SIZE
            {
                size_skipped.push(format!(
                    "  ! Skipped (too large): {} ({:.1} MB)",
                    resolved.display(),
                    meta.len() as f64 / (1024.0 * 1024.0)
                ));
                continue;
            }
            let content = match tokio::fs::read_to_string(resolved).await {
                Ok(c) => c,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Failed to read {}: {e}",
                            resolved.display()
                        )),
                    });
                }
            };
            if !content.contains(old_string) {
                continue;
            }
            let new_content = content.replace(old_string, new_string);
            emit_records.push((
                content.as_bytes().to_vec(),
                new_content.as_bytes().to_vec(),
            ));
            batch.push(EditOp::Replace {
                path: resolved.clone(),
                byte_range: 0..content.len(),
                old_text: content,
                new_text: new_content,
                anchor: None,
            });
            planned_paths.push(resolved.clone());
        }

        if batch.is_empty() {
            results.extend(size_skipped);
            results.push("No files contained old_string; nothing to do.".to_string());
            return Ok(ToolResult {
                success: true,
                output: results.join("\n"),
                error: None,
            });
        }

        if new_string.contains(old_string) {
            results.push(
                "  ! Note: new_string contains old_string; re-running this edit would apply \
                 again (non-idempotent)"
                    .to_string(),
            );
        }

        let batch_id_for_emit = batch.batch_id.clone();
        let emit_records_for_apply = emit_records;
        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
                for (path, (before, after)) in
                    planned_paths.iter().zip(emit_records_for_apply.into_iter())
                {
                    crate::session::record_write_for_current_session(path);
                    crate::agent::file_edit_emitter::emit_file_edit(
                        path,
                        Some(before.as_slice()),
                        Some(after.as_slice()),
                        Some(batch_id_for_emit.clone()),
                    )
                    .await;
                    results.push(format!("  \u{2713} Edited: {}", path.display()));
                }
                results.extend(size_skipped);
                results.push(format!(
                    "\nSuccessfully edited {} file(s)",
                    planned_paths.len()
                ));
                Ok(ToolResult {
                    success: true,
                    output: results.join("\n"),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: results.join("\n"),
                error: Some(format!(
                    "Glob edit failed (rolled back {} planned file(s)): {e}",
                    planned_paths.len()
                )),
            }),
        }
    }
}

