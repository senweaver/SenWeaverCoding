// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

fn security_precheck(security: &SecurityPolicy) -> Option<ToolResult> {
    if !security.can_act() {
        return Some(ToolResult {
            success: false,
            output: String::new(),
            error: Some("Action blocked: autonomy is read-only".into()),
        });
    }
    if security.is_rate_limited() {
        return Some(ToolResult {
            success: false,
            output: String::new(),
            error: Some("Rate limit exceeded".into()),
        });
    }
    None
}

fn path_check(security: &SecurityPolicy, path: &str) -> Option<ToolResult> {
    if !security.is_path_allowed(path) {
        return Some(ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("Path not allowed by security policy: {path}")),
        });
    }
    None
}

async fn verify_resolved_path(
    security: &SecurityPolicy,
    path: &std::path::Path,
    label: &str,
) -> Option<ToolResult> {
    if let Ok(meta) = tokio::fs::symlink_metadata(path).await {
        if meta.file_type().is_symlink() {
            return Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Refusing to operate through symlink: {label}")),
            });
        }
    }
    if let Ok(resolved) = tokio::fs::canonicalize(path).await {
        if !security.is_resolved_path_allowed(&resolved) {
            return Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path escapes workspace boundary: {label}")),
            });
        }
    }
    None
}

async fn verify_dst_parent(
    security: &SecurityPolicy,
    dst_path: &std::path::Path,
    label: &str,
) -> Option<ToolResult> {
    if let Some(parent) = dst_path.parent() {
        if parent.exists() {
            if let Ok(resolved_parent) = tokio::fs::canonicalize(parent).await {
                if !security.is_resolved_path_allowed(&resolved_parent) {
                    return Some(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Destination escapes workspace boundary: {label}")),
                    });
                }
            }
        }
    }
    None
}

pub struct CopyPathTool {
    security: Arc<SecurityPolicy>,
}

impl CopyPathTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for CopyPathTool {
    fn name(&self) -> &str {
        "copy_path"
    }

    fn description(&self) -> &str {
        "Copy a file or directory to a new location within the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source path (relative to workspace)"
                },
                "destination": {
                    "type": "string",
                    "description": "Destination path (relative to workspace)"
                }
            },
            "required": ["source", "destination"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Some(r) = security_precheck(&self.security) {
            return Ok(r);
        }

        let src = args
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'source'"))?;
        let dst = args
            .get("destination")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'destination'"))?;

        if let Some(r) = path_check(&self.security, src) {
            return Ok(r);
        }
        if let Some(r) = path_check(&self.security, dst) {
            return Ok(r);
        }

        let src_path = self.security.resolve_tool_path(src);
        let dst_path = self.security.resolve_tool_path(dst);

        if !src_path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Source does not exist: {src}")),
            });
        }

        if let Some(r) = verify_resolved_path(&self.security, &src_path, src).await {
            return Ok(r);
        }
        if let Some(r) = verify_dst_parent(&self.security, &dst_path, dst).await {
            return Ok(r);
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        if src_path.is_dir() {
            let src_norm = crate::util::normalize_path_for_containment(&src_path);
            let dst_norm = crate::util::normalize_path_for_containment(&dst_path);
            if dst_norm == src_norm || crate::util::path_is_within(&dst_norm, &src_norm) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to copy directory {src} into itself or a subdirectory ({dst})"
                    )),
                });
            }
        }

        let _write_guard = match crate::session::acquire_file_write_guard(&dst_path).await {
            Ok(guard) => guard,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
        };

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path).await?;
        } else {
            if let Some(parent) = dst_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&src_path, &dst_path).await?;
        }

        record_writes_for_tree(&dst_path);
        notify_indexes_paths_changed(std::slice::from_ref(&dst_path));

        Ok(ToolResult {
            success: true,
            output: format!("Copied {src} → {dst}"),
            error: None,
        })
    }
}

pub(crate) fn collect_files_bounded(
    root: &std::path::Path,
    limit: usize,
) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => {
                    out.push(path);
                    if out.len() >= limit {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let ty = entry.file_type().await?;
        let dest_child = dst.join(entry.file_name());
        if ty.is_dir() {
            Box::pin(copy_dir_recursive(&entry.path(), &dest_child)).await?;
        } else {
            tokio::fs::copy(entry.path(), &dest_child).await?;
        }
    }
    Ok(())
}

pub struct MovePathTool {
    security: Arc<SecurityPolicy>,
}

impl MovePathTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for MovePathTool {
    fn name(&self) -> &str {
        "move_path"
    }

    fn description(&self) -> &str {
        "Move (rename) a file or directory within the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source path (relative to workspace)"
                },
                "destination": {
                    "type": "string",
                    "description": "Destination path (relative to workspace)"
                }
            },
            "required": ["source", "destination"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Some(r) = security_precheck(&self.security) {
            return Ok(r);
        }

        let src = args
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'source'"))?;
        let dst = args
            .get("destination")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'destination'"))?;

        if let Some(r) = path_check(&self.security, src) {
            return Ok(r);
        }
        if let Some(r) = path_check(&self.security, dst) {
            return Ok(r);
        }

        let src_path = self.security.resolve_tool_path(src);
        let dst_path = self.security.resolve_tool_path(dst);

        if !src_path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Source does not exist: {src}")),
            });
        }

        if let Some(r) = verify_resolved_path(&self.security, &src_path, src).await {
            return Ok(r);
        }
        if let Some(r) = verify_dst_parent(&self.security, &dst_path, dst).await {
            return Ok(r);
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        let _write_guards = match crate::session::acquire_many_file_write_guards(vec![
            src_path.clone(),
            dst_path.clone(),
        ])
        .await
        {
            Ok(guards) => guards,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
        };

        if let Some(parent) = dst_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&src_path, &dst_path).await?;

        record_writes_for_tree(&dst_path);
        notify_indexes_paths_removed(std::slice::from_ref(&src_path));
        notify_indexes_paths_changed(std::slice::from_ref(&dst_path));

        Ok(ToolResult {
            success: true,
            output: format!("Moved {src} → {dst}"),
            error: None,
        })
    }
}

const MAX_RECORDED_WRITES: usize = 200;

fn record_writes_for_tree(root: &std::path::Path) {
    if root.is_dir() {
        for file in collect_files_bounded(root, MAX_RECORDED_WRITES) {
            crate::session::record_write_for_current_session(&file);
        }
    } else if root.is_file() {
        crate::session::record_write_for_current_session(root);
    }
}

fn notify_indexes_paths_changed(paths: &[std::path::PathBuf]) {
    crate::agent::loop_::services::note_code_files_changed(paths);
    crate::code_intel::symbol_graph::incremental::note_files_changed_global(paths);
}

fn notify_indexes_paths_removed(paths: &[std::path::PathBuf]) {
    crate::agent::loop_::services::note_code_files_changed(paths);
    crate::code_intel::symbol_graph::incremental::note_files_removed_global(paths);
}

pub struct DeletePathTool {
    security: Arc<SecurityPolicy>,
}

impl DeletePathTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for DeletePathTool {
    fn name(&self) -> &str {
        "delete_path"
    }

    fn description(&self) -> &str {
        "Delete a file or directory within the workspace. Directories are removed recursively."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to delete (relative to workspace)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Some(r) = security_precheck(&self.security) {
            return Ok(r);
        }

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;

        if let Some(r) = path_check(&self.security, path) {
            return Ok(r);
        }

        let full = self.security.resolve_tool_path(path);

        if !full.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path does not exist: {path}")),
            });
        }

        if let Some(r) = verify_resolved_path(&self.security, &full, path).await {
            return Ok(r);
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        let _write_guard = match crate::session::acquire_file_write_guard(&full).await {
            Ok(guard) => guard,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
        };

        const MAX_DELETE_SNAPSHOTS: usize = 200;
        let workspace = self.security.workspace_dir();
        let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&workspace);
        let to_snapshot: Vec<std::path::PathBuf> = if full.is_dir() {
            collect_files_bounded(&full, MAX_DELETE_SNAPSHOTS)
        } else {
            vec![full.clone()]
        };
        for file in &to_snapshot {
            let _ = history.snapshot_before_write(file, "delete_path", "pre-delete snapshot");
        }

        if full.is_dir() {
            tokio::fs::remove_dir_all(&full).await?;
        } else {
            tokio::fs::remove_file(&full).await?;
        }

        notify_indexes_paths_removed(std::slice::from_ref(&full));

        Ok(ToolResult {
            success: true,
            output: format!("Deleted {path}"),
            error: None,
        })
    }
}

pub struct CreateDirectoryTool {
    security: Arc<SecurityPolicy>,
}

impl CreateDirectoryTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for CreateDirectoryTool {
    fn name(&self) -> &str {
        "create_directory"
    }

    fn description(&self) -> &str {
        "Create a directory (including any missing parent directories) within the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to create (relative to workspace)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Some(r) = security_precheck(&self.security) {
            return Ok(r);
        }

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path'"))?;

        if let Some(r) = path_check(&self.security, path) {
            return Ok(r);
        }

        let full = self.security.resolve_tool_path(path);

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        tokio::fs::create_dir_all(&full).await?;

        Ok(ToolResult {
            success: true,
            output: format!("Created directory {path}"),
            error: None,
        })
    }
}
