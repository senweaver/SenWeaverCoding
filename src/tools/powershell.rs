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

        match self.security.validate_command_execution(command, false) {
            Ok(_) => {}
            Err(reason) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(reason),
                });
            }
        }

        if let Some(path) = self.security.forbidden_path_argument(command) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path blocked by security policy: {path}")),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let _resource_guard = match crate::session::acquire_shell_for_current_session().await {
            Some(Ok(g)) => Some(g),
            Some(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
            None => None,
        };

        let mut cmd = crate::util::hidden_async_command("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", command]);

        if let Some(dir) = working_dir {
            let full_dir = self.security.resolve_tool_path(dir);
            cmd.current_dir(full_dir);
        }

        for (k, v) in crate::python_env::activation_env(&self.security.workspace_dir()) {
            cmd.env(k, v);
        }
        cmd.env_remove("PYTHONHOME");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());
        cmd.kill_on_drop(true);

        let mirror_id = format!(
            "ps-{}",
            uuid::Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>(),
        );
        let mirror_session_id = crate::session::current_session_context().map(|c| c.session_id);
        let mirror_started = std::time::Instant::now();
        crate::tools::background_registry::publish(
            crate::tools::background_registry::BackgroundShellSignal::Spawned {
                id: mirror_id.clone(),
                command: command.to_string(),
                session_id: mirror_session_id.clone(),
            },
        );

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let error_text = format!("Failed to execute PowerShell: {e}");
                crate::tools::shell::core::emit_mirror_chunks(
                    &mirror_id,
                    &format!("{error_text}\n"),
                    crate::tools::background_registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                crate::tools::background_registry::publish(
                    crate::tools::background_registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: None,
                        session_id: mirror_session_id.clone(),
                    },
                );
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(error_text),
                });
            }
        };

        let outcome = crate::tools::shell::foreground::run_foreground_streamed(
            child,
            &mirror_id,
            mirror_session_id.as_deref(),
            mirror_started,
            std::time::Duration::from_secs(timeout),
        )
        .await;

        use crate::tools::shell::foreground::ForegroundOutcome;

        match outcome {
            ForegroundOutcome::Cancelled(part_stdout, part_stderr) => {
                crate::tools::background_registry::publish(
                    crate::tools::background_registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: None,
                        session_id: mirror_session_id.clone(),
                    },
                );
                Ok(ToolResult {
                    success: true,
                    output: crate::tools::shell::foreground::build_cancelled_output(
                        &part_stdout,
                        &part_stderr,
                    ),
                    error: None,
                })
            }
            ForegroundOutcome::WaitError(e) => {
                let error_text = format!("Failed to execute PowerShell: {e}");
                crate::tools::background_registry::publish(
                    crate::tools::background_registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: None,
                        session_id: mirror_session_id.clone(),
                    },
                );
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(error_text),
                })
            }
            ForegroundOutcome::Timeout(part_stdout, part_stderr) => {
                let banner = format!(
                    "PowerShell command timed out after {timeout}s and was killed. \
                     DO NOT retry the same command verbatim; pass a larger `timeout_secs` \
                     or split the work into smaller steps."
                );
                crate::tools::background_registry::publish(
                    crate::tools::background_registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: None,
                        session_id: mirror_session_id.clone(),
                    },
                );
                let mut detail = String::new();
                if !part_stdout.is_empty() {
                    detail.push_str("--- partial stdout before timeout ---\n");
                    detail.push_str(&part_stdout);
                    if !detail.ends_with('\n') {
                        detail.push('\n');
                    }
                }
                if !part_stderr.is_empty() {
                    detail.push_str("--- partial stderr before timeout ---\n");
                    detail.push_str(&part_stderr);
                    if !detail.ends_with('\n') {
                        detail.push('\n');
                    }
                }
                let error_text = if detail.is_empty() {
                    banner
                } else {
                    format!("{banner}\n{detail}")
                };
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(error_text),
                })
            }
            ForegroundOutcome::Exited(status, stdout, stderr) => {
                let exit_code = status.code().unwrap_or(-1);
                crate::tools::background_registry::publish(
                    crate::tools::background_registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: Some(exit_code),
                        session_id: mirror_session_id.clone(),
                    },
                );

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
                    success: status.success(),
                    output: combined,
                    error: if status.success() {
                        None
                    } else {
                        Some(format!("Exit code: {}", exit_code))
                    },
                })
            }
        }
    }
}
