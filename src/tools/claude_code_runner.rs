// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::config::ClaudeCodeRunnerConfig;
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::process::Command;

const SAFE_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "USER", "SHELL", "TMPDIR",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeHookEvent {

    pub session_id: String,

    pub event_type: String,

    #[serde(default)]
    pub tool_name: Option<String>,

    #[serde(default)]
    pub summary: Option<String>,
}

pub struct ClaudeCodeRunnerTool {
    security: Arc<SecurityPolicy>,
    config: ClaudeCodeRunnerConfig,

    gateway_url: String,
}

impl ClaudeCodeRunnerTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        config: ClaudeCodeRunnerConfig,
        gateway_url: String,
    ) -> Self {
        Self {
            security,
            config,
            gateway_url,
        }
    }

    fn session_name(&self, id: &str) -> String {
        format!("{}{}", self.config.tmux_prefix, id)
    }

    fn ssh_attach_command(&self, session_name: &str) -> Option<String> {
        self.config
            .ssh_host
            .as_ref()
            .map(|host| format!("ssh -t {host} tmux attach-session -t {session_name}"))
    }
}

#[async_trait]
impl Tool for ClaudeCodeRunnerTool {
    fn name(&self) -> &str {
        "claude_code_runner"
    }

    fn description(&self) -> &str {
        "Spawn a Claude Code task in a tmux session with live Slack progress updates and SSH handoff. Returns immediately with session ID and attach command."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The coding task to delegate to Claude Code"
                },
                "working_directory": {
                    "type": "string",
                    "description": "Working directory within the workspace (must be inside workspace_dir)"
                },
                "slack_channel": {
                    "type": "string",
                    "description": "Slack channel ID to post progress updates to"
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
            .enforce_tool_operation(ToolOperation::Act, "claude_code_runner")
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
            if !canonical_wd.starts_with(&canonical_workspace_dir) {
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

        let slack_channel = args
            .get("slack_channel")
            .and_then(|v| v.as_str())
            .map(String::from);

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let session_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let session_name = self.session_name(&session_id);

        let hook_url = format!("{}/hooks/claude-code", self.gateway_url);

        let mut claude_args = vec![
            "claude".to_string(),
            "-p".to_string(),
            prompt.to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ];

        claude_args.push("--hook-url".to_string());
        claude_args.push(hook_url.clone());

        let mut env_exports = String::new();
        for var in SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                use std::fmt::Write;
                let _ = write!(env_exports, "{}={} ", var, shell_escape(&val));
            }
        }

        use std::fmt::Write;
        let _ = write!(env_exports, "CLAUDE_CODE_SESSION_ID={} ", &session_id);
        if let Some(ref ch) = slack_channel {
            let _ = write!(env_exports, "CLAUDE_CODE_SLACK_CHANNEL={} ", ch);
        }
        let _ = write!(env_exports, "CLAUDE_CODE_HOOK_URL={} ", &hook_url);

        let create_result = Command::new("tmux")
            .args(["new-session", "-d", "-s", &session_name])
            .arg("-c")
            .arg(work_dir.to_str().unwrap_or("."))
            .output()
            .await;

        match create_result {
            Ok(output) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to create tmux session: {stderr}")),
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "tmux not found or failed to execute: {e}. Install tmux to use claude_code_runner."
                    )),
                });
            }
            _ => {}
        }

        let full_command = format!(
            "{env_exports}{cmd}",
            env_exports = env_exports,
            cmd = claude_args
                .iter()
                .map(|a| shell_escape(a))
                .collect::<Vec<_>>()
                .join(" ")
        );

        let send_result = Command::new("tmux")
            .args(["send-keys", "-t", &session_name, &full_command, "Enter"])
            .output()
            .await;

        if let Err(e) = send_result {

            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &session_name])
                .output()
                .await;
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to send command to tmux session: {e}")),
            });
        }

        let ttl = self.config.session_ttl;
        let cleanup_session = session_name.clone();
        let _ttl_cleanup = crate::runtime::spawn_supervised(
            format!("tools.claude_code_runner.ttl_cleanup.{}", session_name),
            async move {
                tokio::time::sleep(std::time::Duration::from_secs(ttl)).await;
                let _ = Command::new("tmux")
                    .args(["kill-session", "-t", &cleanup_session])
                    .output()
                    .await;
                tracing::info!(
                    session = cleanup_session,
                    "Claude Code runner session TTL expired, cleaned up"
                );
            },
        );

        let mut output_parts = vec![
            format!("Session started: {session_name}"),
            format!("Session ID: {session_id}"),
            format!("Hook URL: {hook_url}"),
        ];

        if let Some(ssh_cmd) = self.ssh_attach_command(&session_name) {
            output_parts.push(format!("SSH attach: {ssh_cmd}"));
        } else {
            output_parts.push(format!(
                "Local attach: tmux attach-session -t {session_name}"
            ));
        }

        if let Some(ref ch) = slack_channel {
            output_parts.push(format!("Slack channel: {ch} (progress updates enabled)"));
        }

        Ok(ToolResult {
            success: true,
            output: output_parts.join("\n"),
            error: None,
        })
    }
}

fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '+'))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}
