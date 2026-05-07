// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct PowerShellTool {
    security: Arc<SecurityPolicy>,
}

impl PowerShellTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        "powershell"
    }

    fn description(&self) -> &str {
        "Execute PowerShell commands on Windows. Provides PowerShell-specific command execution with security validation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The PowerShell command or script block to execute"
                },
                "working_directory": {
                    "type": "string",
                    "description": "Working directory for command execution"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 120, max: 600)",
                    "default": 120
                }
            },
            "required": ["command"]
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
                error: Some("Rate limit exceeded".into()),
            });
        }

        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;

        let working_dir = args.get("working_directory").and_then(|v| v.as_str());
        let timeout = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(120)
            .min(600);

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let mut cmd = crate::util::hidden_async_command("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", command]);

        if let Some(dir) = working_dir {
            let full_dir = self.security.resolve_tool_path(dir);
            cmd.current_dir(full_dir);
        }

        let result =
            tokio::time::timeout(std::time::Duration::from_secs(timeout), cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                let (compacted_stdout, compacted_stderr, tee_hint) =
                    if crate::token_saver::is_enabled() {
                        let ctx = crate::token_saver::global();
                        let compacted = crate::token_saver::compact_command_output(
                            command, &stdout, &stderr, exit_code, &ctx,
                        );
                        let hint = compacted
                            .tee_path
                            .as_ref()
                            .map(|p| format!("\n[full output: {}]", p.display()))
                            .unwrap_or_default();
                        (compacted.stdout, compacted.stderr, hint)
                    } else {
                        (stdout, stderr, String::new())
                    };

                let mut combined = if compacted_stderr.is_empty() {
                    compacted_stdout
                } else {
                    format!("{}\n--- stderr ---\n{}", compacted_stdout, compacted_stderr)
                };
                if !tee_hint.is_empty() && !combined.contains(&tee_hint) {
                    combined.push_str(&tee_hint);
                }

                Ok(ToolResult {
                    success: output.status.success(),
                    output: combined,
                    error: if output.status.success() {
                        None
                    } else {
                        Some(format!("Exit code: {}", exit_code))
                    },
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute PowerShell: {}", e)),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "PowerShell command timed out after {} seconds",
                    timeout
                )),
            }),
        }
    }
}
