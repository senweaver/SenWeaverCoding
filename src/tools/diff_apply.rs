// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::apply_model::OpsApplier;
use crate::diff_session::DiffSession;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct DiffApplyTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Option<Arc<OpsApplier>>,
}

impl DiffApplyTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self {
            security,
            ops_applier: None,
        }
    }

    #[must_use]
    pub fn with_ops_applier(mut self, ops_applier: Arc<OpsApplier>) -> Self {
        self.ops_applier = Some(ops_applier);
        self
    }

    async fn retry_with_ladder(
        &self,
        files: &[serde_json::Value],
        resolved_paths: &[PathBuf],
    ) -> Option<Result<crate::diff_session::ApplyReport, crate::diff_session::DiffSessionError>>
    {
        let refiner = crate::apply_model::fast_apply::runtime_ladder_refiner()?;
        let root = self.security.workspace_dir();
        let mut session = DiffSession::new(root)
            .with_allowed_roots(self.security.allowed_roots.clone());
        if let Some(ops) = self.ops_applier.clone() {
            session = session.with_ops_applier(ops);
        }
        let mut refined_any = false;
        for (entry, resolved) in files.iter().zip(resolved_paths.iter()) {
            let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let diff = entry.get("diff").and_then(|v| v.as_str()).unwrap_or("");
            if resolved.exists() {
                if let Some(refined) =
                    crate::apply_model::fast_apply::refine_failing_diff_to_content(
                        refiner.as_ref(),
                        resolved,
                        diff,
                        3,
                    )
                    .await
                {
                    refined_any = true;
                    if let Err(e) = session.stage_full_content(
                        path,
                        refined.contents,
                        refined.encoding,
                        refined.pre_sha256,
                    ) {
                        return Some(Err(e));
                    }
                    continue;
                }
            }
            if let Err(e) = session.stage(path, diff) {
                return Some(Err(e));
            }
        }
        if !refined_any {
            return None;
        }
        Some(session.apply_all().await)
    }

    async fn verify_no_symlink(&self, path: &Path) -> anyhow::Result<()> {
        if let Ok(meta) = tokio::fs::symlink_metadata(path).await {
            if meta.file_type().is_symlink() {
                anyhow::bail!("Refusing to apply diff through symlink: {}", path.display());
            }
        }

        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            if let Ok(meta) = tokio::fs::symlink_metadata(parent).await {
                if meta.file_type().is_symlink() {
                    anyhow::bail!(
                        "Refusing to apply diff through symlinked parent directory: {}",
                        parent.display()
                    );
                }
            }
            current = parent.to_path_buf();
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for DiffApplyTool {
    fn name(&self) -> &str {
        "diff_apply"
    }

    fn description(&self) -> &str {
        "Atomically apply per-file unified-diff hunks given as explicit {path, diff} pairs \
         (no `---`/`+++` headers needed): a single transaction where any failure rolls back \
         every change. Use for coordinated multi-file edits expressed as diffs; for a complete \
         patch document use patch_apply, for exact-string edits use file_edit/multi_edit."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "description": "Files and their unified diffs to apply atomically.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path (relative to workspace or absolute)."
                            },
                            "diff": {
                                "type": "string",
                                "description": "Unified diff text to apply to the file."
                            }
                        },
                        "required": ["path", "diff"]
                    }
                }
            },
            "required": ["files"]
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

        let files = match args.get("files").and_then(|v| v.as_array()) {
            Some(f) if !f.is_empty() => f.clone(),
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("diff_apply requires a non-empty `files` array".into()),
                });
            }
        };

        let root = self.security.workspace_dir();
        let mut session = DiffSession::new(root)
            .with_allowed_roots(self.security.allowed_roots.clone());
        if let Some(ops) = self.ops_applier.clone() {
            session = session.with_ops_applier(ops);
        }
        let mut resolved_paths: Vec<PathBuf> = Vec::with_capacity(files.len());

        for entry in &files {
            let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let diff = entry.get("diff").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() || diff.is_empty() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("each file entry needs a non-empty `path` and `diff`".into()),
                });
            }
            if !self.security.is_path_allowed(path) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Path not allowed by security policy: {path}")),
                });
            }
            let resolved = self.security.resolve_tool_path(path);
            if self.security.is_runtime_config_path(&resolved) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to modify runtime config/state file: {}",
                        self.security.runtime_config_violation_message(&resolved)
                    )),
                });
            }
            if let Err(e) = self.verify_no_symlink(&resolved).await {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Security check failed: {e}")),
                });
            }
            if let Err(e) = session.stage(path, diff) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("failed to stage diff for {path}: {e}")),
                });
            }
            resolved_paths.push(resolved);
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let _resource_guards = match crate::session::acquire_many_file_write_guards(
            resolved_paths.clone(),
        )
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

        for p in &resolved_paths {
            if crate::session::is_stale_for_current_session(p) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(crate::session::stale_file_error_message(p)),
                });
            }
            if p.exists() && !crate::session::has_read_in_current_session(p) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to apply diff to '{}': this session has not read the file \
                         yet. Use file_read on it first (the diff must be based on the \
                         file's CURRENT contents), then retry. A compacted/Signatures view \
                         does not count: use level=default, paging large files with offset/limit.",
                        p.display()
                    )),
                });
            }
        }

        const MAX_DIFF_FILE_BYTES: u64 = 10 * 1024 * 1024;
        for p in &resolved_paths {
            if let Ok(meta) = std::fs::metadata(p) {
                if meta.len() > MAX_DIFF_FILE_BYTES {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "refusing to patch '{}' ({} bytes exceeds the {} byte limit); split the change or edit a smaller region",
                            p.display(),
                            meta.len(),
                            MAX_DIFF_FILE_BYTES
                        )),
                    });
                }
            }
        }

        let pre_contents: Vec<Option<Vec<u8>>> = {
            let paths = resolved_paths.clone();
            tokio::task::spawn_blocking(move || {
                paths.iter().map(|p| std::fs::read(p).ok()).collect()
            })
            .await
            .unwrap_or_default()
        };

        let diag_baseline =
            crate::code_intel::post_edit_diagnostics::baseline(&resolved_paths).await;

        let report = match session.apply_all().await {
            Ok(report) => report,
            Err(first_err) => {
                let Some(retry_report) = self
                    .retry_with_ladder(&files, &resolved_paths)
                    .await
                else {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "diff session apply failed (changes rolled back): {first_err}"
                        )),
                    });
                };
                match retry_report {
                    Ok(report) => report,
                    Err(retry_err) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "diff session apply failed (changes rolled back): {retry_err}"
                            )),
                        });
                    }
                }
            }
        };

        for (idx, path) in resolved_paths.iter().enumerate() {
            let after = tokio::fs::read(path).await.ok();
            let before = pre_contents.get(idx).cloned().flatten();
            if let Some(after_bytes) = after.as_deref() {
                crate::agent::file_edit_emitter::emit_file_edit(
                    path,
                    before.as_deref(),
                    Some(after_bytes),
                    None,
                )
                .await;
            }
            crate::session::record_write_for_current_session(path);
        }
        let mut output = format!(
            "Applied {} file(s) atomically via diff session ({} hunk(s) exact, {} fuzzy).",
            report.files_touched.len(),
            report.total_hunks_exact,
            report.total_hunks_fuzzy
        );
        if report.total_hunks_fuzzy > 0 {
            output.push_str(
                "\n[Note: fuzzy hunks were anchored away from their stated line numbers; \
                 re-read the affected regions to confirm placement.]",
            );
        }
        if let Some(feedback) = crate::code_intel::post_edit_diagnostics::new_error_feedback(
            &resolved_paths,
            &diag_baseline,
        )
        .await
        {
            output.push_str(&feedback);
        }
        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
