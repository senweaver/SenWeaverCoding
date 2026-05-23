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
            copy_dir_recursive(&src_path, &dst_path).await?;
        } else {
            if let Some(parent) = dst_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&src_path, &dst_path).await?;
        }

        Ok(ToolResult {
            success: true,
            output: format!("Copied {src} → {dst}"),
            error: None,
        })
    }
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

        if let Some(parent) = dst_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&src_path, &dst_path).await?;

        Ok(ToolResult {
            success: true,
            output: format!("Moved {src} → {dst}"),
            error: None,
        })
    }
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

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        if full.is_dir() {
            tokio::fs::remove_dir_all(&full).await?;
        } else {
            tokio::fs::remove_file(&full).await?;
        }

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
