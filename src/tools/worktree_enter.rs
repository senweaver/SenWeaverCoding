// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct WorktreeEnterTool {
    security: Arc<SecurityPolicy>,
}

impl WorktreeEnterTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for WorktreeEnterTool {
    fn name(&self) -> &str {
        "worktree_enter"
    }

    fn description(&self) -> &str {
        "Create and enter a git worktree for isolated session work. Creates a new branch and working directory separate from the main workspace."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "branch_name": {
                    "type": "string",
                    "description": "Name for the new branch to create in the worktree"
                },
                "base_branch": {
                    "type": "string",
                    "description": "Base branch to create the worktree from (default: current branch)"
                }
            },
            "required": ["branch_name"]
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

        let branch_name = args
            .get("branch_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'branch_name' parameter"))?;
        let base_branch = args.get("base_branch").and_then(|v| v.as_str());

        let workspace = self.security.workspace_dir();
        let worktree_dir = workspace
            .parent()
            .unwrap_or(workspace.as_path())
            .join(format!(".worktrees/{}", branch_name));

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded".into()),
            });
        }

        if let Err(e) = tokio::fs::create_dir_all(&worktree_dir).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to create worktree directory: {}", e)),
            });
        }

        let worktree_path = worktree_dir.to_string_lossy().to_string();
        let mut git_args = vec![
            "worktree".to_string(),
            "add".to_string(),
            worktree_path,
            "-b".to_string(),
            branch_name.to_string(),
        ];
        if let Some(base) = base_branch {
            git_args.push(base.to_string());
        }

        let output = tokio::process::Command::new("git")
            .args(&git_args)
            .current_dir(workspace)
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => Ok(ToolResult {
                success: true,
                output: json!({
                    "worktree_path": worktree_dir.to_string_lossy(),
                    "branch": branch_name,
                    "message": format!("Entered worktree at {}", worktree_dir.display())
                })
                .to_string(),
                error: None,
            }),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("git worktree add failed: {}", stderr)),
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to run git: {}", e)),
            }),
        }
    }
}
