// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Restores a file to its last known good state from git or disk backup.
///
/// Uses `git checkout -- <path>` to revert the file to HEAD. This is useful
/// when an agent has made a bad edit and wants to undo its changes.
pub struct RestoreFileTool {
    security: Arc<SecurityPolicy>,
}

impl RestoreFileTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for RestoreFileTool {
    fn name(&self) -> &str {
        "restore_file"
    }

    fn description(&self) -> &str {
        "Restore a file to its last committed state using git checkout. \
         Useful for undoing bad edits or reverting to the original version."
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
                    "description": "Git revision to restore from (default: HEAD)",
                    "default": "HEAD"
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

        let revision = args
            .get("revision")
            .and_then(|v| v.as_str())
            .unwrap_or("HEAD");

        if !self.security.is_path_allowed(path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        let full_path = self.security.resolve_tool_path(path);

        // Determine workspace root for running git
        let workspace = full_path
            .parent()
            .unwrap_or(&self.security.workspace_dir);

        let output = tokio::process::Command::new("git")
            .args(["checkout", revision, "--", &full_path.to_string_lossy()])
            .current_dir(workspace)
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => Ok(ToolResult {
                success: true,
                output: format!("Restored {path} from {revision}"),
                error: None,
            }),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                // Fallback: if git fails (not a repo), try reading from disk backup
                if stderr.contains("not a git repository") {
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Not a git repository. Cannot restore {path}. \
                             Consider using file_read to check the current state."
                        )),
                    })
                } else {
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("git checkout failed: {}", stderr.trim())),
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute git: {e}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name() {
        let sec = Arc::new(SecurityPolicy::default());
        assert_eq!(RestoreFileTool::new(sec).name(), "restore_file");
    }

    #[test]
    fn schema_has_path() {
        let sec = Arc::new(SecurityPolicy::default());
        let schema = RestoreFileTool::new(sec).parameters_schema();
        assert!(schema["properties"]["path"].is_object());
    }
}
