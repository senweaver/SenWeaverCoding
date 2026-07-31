// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct WorktreeExitTool {
    security: Arc<SecurityPolicy>,
}

impl WorktreeExitTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for WorktreeExitTool {
    fn name(&self) -> &str {
        "worktree_exit"
    }

    fn description(&self) -> &str {
        "Exit the current git worktree session and optionally remove the worktree. Returns to the main workspace."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "worktree_path": {
                    "type": "string",
                    "description": "Path to the worktree to exit/remove"
                },
                "remove": {
                    "type": "boolean",
                    "description": "Whether to remove the worktree after exiting (default: false)",
                    "default": false
                },
                "force": {
                    "type": "boolean",
                    "description": "Force removal even if there are uncommitted changes (default: false)",
                    "default": false
                }
            },
            "required": ["worktree_path"]
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

        let worktree_path = args
            .get("worktree_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'worktree_path' parameter"))?;
        let remove = args
            .get("remove")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        if remove {
            let mut cmd_args = vec!["worktree", "remove"];
            if force {
                cmd_args.push("--force");
            }
            cmd_args.push(worktree_path);

            let mut cmd = crate::util::hidden_async_command("git");
            cmd.args(&cmd_args)
                .current_dir(self.security.workspace_dir())
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
                        error: Some(format!("git worktree remove timed out after {timeout_secs}s")),
                    });
                }
            };

            match output {
                Ok(out) if out.status.success() => Ok(ToolResult {
                    success: true,
                    output: format!("Exited and removed worktree at '{}'", worktree_path),
                    error: None,
                }),
                Ok(out) => {
                    let stderr = crate::util::decode_subprocess_bytes(&out.stderr);
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("git worktree remove failed: {}", stderr)),
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to run git: {}", e)),
                }),
            }
        } else {
            Ok(ToolResult {
                success: true,
                output: format!("Exited worktree at '{}' (not removed)", worktree_path),
                error: None,
            })
        }
    }
}
