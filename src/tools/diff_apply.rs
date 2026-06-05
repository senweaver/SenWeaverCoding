// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::diff_session::DiffSession;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct DiffApplyTool {
    security: Arc<SecurityPolicy>,
}

impl DiffApplyTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for DiffApplyTool {
    fn name(&self) -> &str {
        "diff_apply"
    }

    fn description(&self) -> &str {
        "Atomically apply a set of unified-diff patches across multiple files as a single \
         transaction. If any file fails to apply, every change in the set is rolled back. Use \
         this for coordinated multi-file edits that must succeed or fail together."
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
        let mut session = DiffSession::new(root);
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
            if let Err(e) = session.stage(path, diff) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("failed to stage diff for {path}: {e}")),
                });
            }
            resolved_paths.push(self.security.resolve_tool_path(path));
        }

        let pre_contents: Vec<Option<Vec<u8>>> = {
            let paths = resolved_paths.clone();
            tokio::task::spawn_blocking(move || {
                paths.iter().map(|p| std::fs::read(p).ok()).collect()
            })
            .await
            .unwrap_or_default()
        };

        match session.apply_all().await {
            Ok(report) => {
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
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Applied {} file(s) atomically via diff session.",
                        report.files_touched.len()
                    ),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("diff session apply failed (changes rolled back): {e}")),
            }),
        }
    }
}
