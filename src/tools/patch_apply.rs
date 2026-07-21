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

fn canonicalize_allowing_missing_tail(parent: &Path) -> anyhow::Result<PathBuf> {
    match std::fs::canonicalize(parent) {
        Ok(p) => Ok(p),
        Err(_) => {
            let mut existing = parent.to_path_buf();
            let mut suffix: Vec<std::ffi::OsString> = Vec::new();
            loop {
                match std::fs::canonicalize(&existing) {
                    Ok(canon) => {
                        let mut out = canon;
                        for part in suffix.iter().rev() {
                            out.push(part);
                        }
                        return Ok(out);
                    }
                    Err(_) => match (existing.parent(), existing.file_name()) {
                        (Some(p), Some(name)) => {
                            suffix.push(name.to_os_string());
                            existing = p.to_path_buf();
                        }
                        _ => anyhow::bail!(
                            "Failed to resolve parent directory: {}",
                            parent.display()
                        ),
                    },
                }
            }
        }
    }
}

fn resolve_and_validate_path_sync_with(
    security: &SecurityPolicy,
    path: &str,
) -> anyhow::Result<PathBuf> {
    let expanded = security.resolve_tool_path(path);

    let parent = expanded
        .parent()
        .context("Invalid path: missing parent directory")?;

    let resolved_parent = canonicalize_allowing_missing_tail(parent)?;
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

    fn validate_path_allowed(&self, path: &str) -> anyhow::Result<()> {
        if !self.security.is_path_allowed(path) {
            anyhow::bail!("Path not allowed by security policy: {path}");
        }
        Ok(())
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

}

fn git_path_from_marker(raw: &str) -> Option<String> {
    // Markers look like `a/foo.rs`, `b/foo.rs`, `foo.rs`, or `/dev/null`, and may
    // carry a trailing tab-separated timestamp.
    let t = raw.split('\t').next().unwrap_or(raw).trim();
    if t == "/dev/null" || t.is_empty() {
        return None;
    }
    let stripped = t
        .strip_prefix("a/")
        .or_else(|| t.strip_prefix("b/"))
        .unwrap_or(t);
    Some(stripped.to_string())
}

fn parse_diff_git_target(line: &str) -> Option<String> {
    // `diff --git a/path b/path` -> prefer the `b/` (destination) token.
    let rest = line.strip_prefix("diff --git ")?;
    let b_token = rest.rsplit(' ').next()?;
    git_path_from_marker(b_token)
}

fn is_dev_null_marker(rest: &str) -> bool {
    rest.split('\t').next().unwrap_or(rest).trim() == "/dev/null"
}

fn extract_new_file_contents(diff_lines: &[String]) -> String {
    let mut out = String::new();
    let mut in_hunk = false;
    let mut trailing_newline = true;
    for line in diff_lines {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            out.push_str(added);
            out.push('\n');
            trailing_newline = true;
        } else if line.starts_with('\\') {
            // "\ No newline at end of file"
            trailing_newline = false;
        }
    }
    if !trailing_newline && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn parse_patch_with(
    security: &SecurityPolicy,
    patch_content: &str,
) -> anyhow::Result<Vec<PatchFile>> {
    struct PendingFile {
        lines: Vec<String>,
        hunk_starts: Vec<u32>,
        path_hint: Option<String>,
        from_hint: Option<String>,
        to_hint: Option<String>,
        from_dev_null: bool,
        to_dev_null: bool,
    }

    let mut files: Vec<PatchFile> = Vec::new();
    let mut pending: Option<PendingFile> = None;

    let finalize = |pending: PendingFile,
                    files: &mut Vec<PatchFile>|
     -> anyhow::Result<()> {
        // Prefer the destination path (`+++ b/...`); for a deletion (`+++
        // /dev/null`) fall back to the source; then the `diff --git` hint.
        let chosen = pending
            .to_hint
            .clone()
            .or_else(|| pending.from_hint.clone())
            .or_else(|| pending.path_hint.clone());
        if let Some(path_str) = chosen {
            let path = resolve_and_validate_path_sync_with(security, &path_str)?;
            let action = if pending.from_dev_null && !pending.to_dev_null {
                PatchFileAction::Create {
                    contents: extract_new_file_contents(&pending.lines),
                }
            } else if pending.to_dev_null && !pending.from_dev_null {
                PatchFileAction::Delete
            } else {
                PatchFileAction::Modify
            };
            files.push(PatchFile {
                path,
                diff_text: pending.lines.join("\n"),
                hunks: pending
                    .hunk_starts
                    .iter()
                    .map(|s| PatchHunk { old_start: *s })
                    .collect(),
                action,
            });
        }
        Ok(())
    };

    let lines: Vec<&str> = patch_content.lines().collect();
    let mut remaining_old: Option<i64> = None;
    let mut remaining_new: Option<i64> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line = *line;
        let is_git_header = line.starts_with("diff --git ");

        let in_hunk = pending
            .as_ref()
            .is_some_and(|p| !p.hunk_starts.is_empty());
        let counts_known = remaining_old.is_some() && remaining_new.is_some();
        let body_open = in_hunk
            && match (remaining_old, remaining_new) {
                (Some(o), Some(n)) => o > 0 || n > 0,
                _ => true,
            };

        // A `--- ` line starts a new file only when we are not inside an open hunk
        // body; inside a body it is content (e.g. deleting a `-- comment` line).
        let is_plain_old_marker = line.starts_with("--- ")
            && pending
                .as_ref()
                .map(|p| p.path_hint.is_some() || p.from_hint.is_some() || !p.hunk_starts.is_empty())
                .unwrap_or(true)
            && (!in_hunk
                || !body_open
                || (!counts_known
                    && lines.get(idx + 1).is_some_and(|n| n.starts_with("+++ "))));

        if is_git_header || is_plain_old_marker {
            if let Some(prev) = pending.take() {
                finalize(prev, &mut files)?;
            }
            remaining_old = None;
            remaining_new = None;
            let mut fresh = PendingFile {
                lines: Vec::new(),
                hunk_starts: Vec::new(),
                path_hint: None,
                from_hint: None,
                to_hint: None,
                from_dev_null: false,
                to_dev_null: false,
            };
            if is_git_header {
                fresh.path_hint = parse_diff_git_target(line);
            } else if let Some(rest) = line.strip_prefix("--- ") {
                fresh.from_hint = git_path_from_marker(rest);
                fresh.from_dev_null = is_dev_null_marker(rest);
            }
            fresh.lines.push(line.to_string());
            pending = Some(fresh);
            continue;
        }

        let Some(cur) = pending.as_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix("@@ ") {
            let mut old_start: Option<u32> = None;
            let mut old_count: Option<i64> = None;
            let mut new_count: Option<i64> = None;
            for token in rest.split_whitespace() {
                if token == "@@" {
                    break;
                }
                if let Some(spec) = token.strip_prefix('-') {
                    if old_start.is_none() {
                        let mut parts = spec.splitn(2, ',');
                        old_start = parts.next().and_then(|s| s.parse::<u32>().ok());
                        if old_start.is_some() {
                            old_count = parts.next().and_then(|s| s.parse::<i64>().ok());
                        }
                    }
                } else if let Some(spec) = token.strip_prefix('+') {
                    if new_count.is_none() && spec.split(',').next().is_some_and(|s| s.parse::<u32>().is_ok()) {
                        new_count = spec.splitn(2, ',').nth(1).and_then(|s| s.parse::<i64>().ok());
                    }
                }
            }
            cur.hunk_starts.push(old_start.unwrap_or(1));
            remaining_old = old_count;
            remaining_new = new_count;
            cur.lines.push(line.to_string());
            continue;
        }

        if !body_open {
            if let Some(rest) = line.strip_prefix("--- ") {
                cur.from_hint = git_path_from_marker(rest);
                cur.from_dev_null = is_dev_null_marker(rest);
                cur.lines.push(line.to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix("+++ ") {
                cur.to_hint = git_path_from_marker(rest);
                cur.to_dev_null = is_dev_null_marker(rest);
                cur.lines.push(line.to_string());
                continue;
            }
        }

        if body_open {
            match line.as_bytes().first() {
                Some(b'\\') => {}
                Some(b'-') => {
                    if let Some(o) = remaining_old.as_mut() {
                        *o -= 1;
                    }
                }
                Some(b'+') => {
                    if let Some(n) = remaining_new.as_mut() {
                        *n -= 1;
                    }
                }
                Some(b' ') | None => {
                    if let Some(o) = remaining_old.as_mut() {
                        *o -= 1;
                    }
                    if let Some(n) = remaining_new.as_mut() {
                        *n -= 1;
                    }
                }
                Some(_) => {}
            }
        }
        cur.lines.push(line.to_string());
    }

    if let Some(prev) = pending.take() {
        finalize(prev, &mut files)?;
    }

    Ok(files)
}

#[derive(Debug, Clone)]
enum PatchFileAction {
    Modify,
    Create { contents: String },
    Delete,
}

#[derive(Debug)]
struct PatchFile {
    path: PathBuf,

    diff_text: String,

    hunks: Vec<PatchHunk>,

    action: PatchFileAction,
}

#[derive(Debug)]
#[allow(dead_code)]
struct PatchHunk {
    old_start: u32,
}

#[async_trait]
impl Tool for PatchApplyTool {
    fn name(&self) -> &str {
        "patch_apply"
    }

    fn description(&self) -> &str {
        "Apply ONE multi-file unified diff patch (full `---`/`+++` headers), including \
         creating files (`--- /dev/null`) and deleting files (`+++ /dev/null`). Supports \
         stats/preview/dry-run. Atomic by default: any hunk failure rolls back everything \
         (atomic=false keeps partial successes, marked degraded). Use when you already hold \
         a complete patch; for targeted string edits prefer file_edit/multi_edit."
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
                },
                "fuzz": {
                    "type": "integer",
                    "description": "Maximum fuzz (line drift) allowed when locating hunks (0-5, default 3). Use 0 to require exact context matches."
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

        let cli_dry_run = crate::util::get_runtime_var("SEN_DRY_RUN").as_deref() == Some("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        let atomic = args
            .get("atomic")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let fuzz = args
            .get("fuzz")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(5) as u8)
            .unwrap_or(3);

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
                    let label = match &file.action {
                        PatchFileAction::Create { .. } => " [new file]",
                        PatchFileAction::Delete => " [deleted]",
                        PatchFileAction::Modify => "",
                    };
                    stats.push(format!(
                        "  {}{} ({} hunk(s))",
                        file.path.display(),
                        label,
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
                    match &file.action {
                        PatchFileAction::Create { contents } => {
                            batch.push(EditOp::CreateFile {
                                path: file.path.clone(),
                                contents: contents.clone(),
                                overwrite: false,
                                encoding: None,
                            });
                        }
                        PatchFileAction::Delete => {
                            batch.push(EditOp::DeleteFile {
                                path: file.path.clone(),
                                missing_ok: false,
                            });
                        }
                        PatchFileAction::Modify => {
                            batch.push(EditOp::ApplyHunk {
                                path: file.path.clone(),
                                diff: file.diff_text.clone(),
                                fuzz,
                                scope_anchor: None,
                            });
                        }
                    }
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
                    let pre_paths: Vec<std::path::PathBuf> =
                        patch_files.iter().map(|f| f.path.clone()).collect();
                    let pre_contents: Vec<Option<Vec<u8>>> =
                        tokio::task::spawn_blocking(move || {
                            pre_paths.iter().map(|p| std::fs::read(p).ok()).collect()
                        })
                        .await?;
                    match self.ops_applier.apply_batch(batch).await {
                        Ok(outcome) => {
                            let batch_id = outcome.batch_id.clone();
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
                                    let after_bytes =
                                        tokio::fs::read(&op.touched_path).await.ok();
                                    let before_bytes = pre_contents.get(idx).cloned().flatten();
                                    if let Some(after) = after_bytes.as_deref() {
                                        crate::agent::file_edit_emitter::emit_file_edit(
                                            &op.touched_path,
                                            before_bytes.as_deref(),
                                            Some(after),
                                            Some(batch_id.clone()),
                                        )
                                        .await;
                                    }
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
