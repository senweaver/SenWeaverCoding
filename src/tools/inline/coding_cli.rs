// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::config::InlineCodingCliConfig;
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
const SAFE_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "USER", "SHELL", "TMPDIR",
];

pub struct InlineCodingCliTool {
    security: Arc<SecurityPolicy>,
    config: InlineCodingCliConfig,
}

impl InlineCodingCliTool {
    pub fn new(security: Arc<SecurityPolicy>, config: InlineCodingCliConfig) -> Self {
        Self { security, config }
    }
}

#[async_trait]
impl Tool for InlineCodingCliTool {
    fn name(&self) -> &str {
        "inline_coding_cli"
    }

    fn description(&self) -> &str {
        "Delegate a coding task to an inline (quiet-mode) external coding CLI (binary 'codex -q'). Supports file editing and bash execution. Use for one-shot inline coding sub-tasks."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The coding task to delegate to the inline coding CLI"
                },
                "working_directory": {
                    "type": "string",
                    "description": "Working directory within the workspace (must be inside workspace_dir)"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "inline_coding_cli")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'prompt' parameter"))?;

        let work_dir = if let Some(wd) = args.get("working_directory").and_then(|v| v.as_str()) {
            let wd_path = std::path::PathBuf::from(wd);
            let workspace = self.security.workspace_dir();
            let canonical_wd = match wd_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "working_directory '{}' does not exist or is not accessible",
                            wd
                        )),
                    });
                }
            };
            let canonical_workspace_dir = match workspace.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "workspace directory '{}' does not exist or is not accessible",
                            workspace.display()
                        )),
                    });
                }
            };
            if !self.security.is_resolved_path_allowed(&canonical_wd) {
                let _ = canonical_workspace_dir;
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "working_directory '{}' is outside the workspace '{}'",
                        wd,
                        workspace.display()
                    )),
                });
            }
            canonical_wd
        } else {
            self.security.workspace_dir()
        };

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let cli_bin = if cfg!(target_os = "windows") {
            "codex.cmd"
        } else {
            "codex"
        };
        let mut cmd = crate::util::hidden_async_command(cli_bin);
        cmd.arg("-q").arg(prompt);

        cmd.env_clear();
        for var in SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
        for var in &self.config.env_passthrough {
            let trimmed = var.trim();
            if !trimmed.is_empty() {
                if let Ok(val) = std::env::var(trimmed) {
                    cmd.env(trimmed, val);
                }
            }
        }

        cmd.current_dir(&work_dir);

        let timeout = Duration::from_secs(self.config.timeout_secs);
        cmd.kill_on_drop(true);

        let result = tokio::time::timeout(timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if stdout.len() > self.config.max_output_bytes {
                    let mut b = self.config.max_output_bytes.min(stdout.len());
                    while b > 0 && !stdout.is_char_boundary(b) {
                        b -= 1;
                    }
                    stdout.truncate(b);
                    stdout.push_str("\n... [output truncated]");
                }

                Ok(ToolResult {
                    success: output.status.success(),
                    output: stdout,
                    error: if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    },
                })
            }
            Ok(Err(e)) => {
                let err_msg = e.to_string();
                let msg = if err_msg.contains("No such file or directory")
                    || err_msg.contains("not found")
                    || err_msg.contains("cannot find")
                {
                    "Inline coding CLI ('codex') not found in PATH. Configure the binary via PATH or update [inline_coding_cli] in config.".into()
                } else {
                    format!("Failed to execute inline coding CLI: {e}")
                };
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(msg),
                })
            }
            Err(_) => {

                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Inline coding CLI timed out after {}s and was killed",
                        self.config.timeout_secs
                    )),
                })
            }
        }
    }
}
