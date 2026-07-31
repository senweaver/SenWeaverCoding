// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::job_object::{JobLimits, JobObjectGuard};
use crate::security::traits::Sandbox;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const MAX_OUTPUT_BYTES: usize = 1_048_576;

const DEFAULT_LLM_OUTPUT_CAP: usize = 32_768;

fn truncate_output(s: &mut String, cap: usize, marker: &str) {
    if let Some(clipped) = crate::util::truncate_head_tail(s, cap, 25) {
        *s = clipped;
        s.push_str(marker);
    }
}

pub struct PowerShellTool {
    security: Arc<SecurityPolicy>,
    sandbox: Arc<dyn Sandbox>,
    job_limits: Option<JobLimits>,
}

impl PowerShellTool {
    pub fn new(security: Arc<SecurityPolicy>, sandbox: Arc<dyn Sandbox>) -> Self {
        let job_limits = if sandbox.name() == "windows-job-object" {
            Some(JobLimits::default())
        } else {
            None
        };
        Self {
            security,
            sandbox,
            job_limits,
        }
    }

    pub fn with_resource_limits(
        mut self,
        resources: &crate::config::schema::ResourceLimitsConfig,
    ) -> Self {
        self.job_limits = self
            .job_limits
            .map(|limits| limits.with_resource_overrides(resources));
        self
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

        let _preflight = match crate::tools::shell::preflight::acquire_shell_execution_clearance(
            &self.security,
            command,
        )
        .await
        {
            Ok(guards) => guards,
            Err(result) => return Ok(result),
        };

        let mut cmd = crate::util::hidden_async_command("powershell");
        let encoded_command = {
            use base64::Engine as _;
            let utf16le: Vec<u8> = command
                .encode_utf16()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            base64::engine::general_purpose::STANDARD.encode(utf16le)
        };
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_command,
        ]);
        cmd.current_dir(self.security.workspace_dir());

        if let Some(dir) = working_dir {
            let full_dir = self.security.resolve_tool_path(dir);
            let resolved = match tokio::fs::canonicalize(&full_dir).await {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Invalid working_directory '{dir}': {e}")),
                    });
                }
            };
            if !self.security.is_resolved_path_allowed(&resolved) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(self.security.resolved_path_violation_message(&resolved)),
                });
            }
            cmd.current_dir(resolved);
        }

        crate::tools::shell::core::prepare_isolated_command(
            &mut cmd,
            &self.security,
            self.sandbox.as_ref(),
        )
        .map_err(|e| anyhow::anyhow!("Sandbox error: {}", e))?;

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
        crate::tools::background::registry::publish(
            crate::tools::background::registry::BackgroundShellSignal::Spawned {
                id: mirror_id.clone(),
                command: command.to_string(),
                session_id: mirror_session_id.clone(),
            },
        );

        let (job_guard, child): (Option<JobObjectGuard>, tokio::process::Child) =
            match crate::tools::shell::core::spawn_with_job_limits(cmd, self.job_limits).await {
                Ok(pair) => pair,
                Err(e) => {
                let error_text = format!("Failed to execute PowerShell: {e}");
                crate::tools::shell::core::emit_mirror_chunks(
                    &mirror_id,
                    &format!("{error_text}\n"),
                    crate::tools::background::registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                crate::tools::background::registry::publish(
                    crate::tools::background::registry::BackgroundShellSignal::Exited {
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

        let outcome = crate::tools::shell::foreground::run_foreground_streamed_inner(
            child,
            job_guard,
            &mirror_id,
            mirror_session_id.as_deref(),
            mirror_started,
            std::time::Duration::from_secs(timeout),
            None,
            command,
        )
        .await;

        use crate::tools::shell::foreground::ForegroundOutcome;

        match outcome {
            ForegroundOutcome::Backgrounded { partial_stdout, partial_stderr } => {
                Ok(ToolResult {
                    success: true,
                    output: format!("{partial_stdout}{partial_stderr}"),
                    error: None,
                })
            }
            ForegroundOutcome::Cancelled(part_stdout, part_stderr) => {
                crate::tools::background::registry::publish(
                    crate::tools::background::registry::BackgroundShellSignal::Exited {
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
                crate::tools::background::registry::publish(
                    crate::tools::background::registry::BackgroundShellSignal::Exited {
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
                crate::tools::background::registry::publish(
                    crate::tools::background::registry::BackgroundShellSignal::Exited {
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
            ForegroundOutcome::Exited(status, mut stdout, mut stderr) => {
                let exit_code = status.code().unwrap_or(-1);
                crate::tools::background::registry::publish(
                    crate::tools::background::registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: Some(exit_code),
                        session_id: mirror_session_id.clone(),
                    },
                );

                truncate_output(
                    &mut stdout,
                    MAX_OUTPUT_BYTES,
                    "\n... [output truncated at 1MB]",
                );
                truncate_output(
                    &mut stderr,
                    MAX_OUTPUT_BYTES,
                    "\n... [stderr truncated at 1MB]",
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

                truncate_output(
                    &mut combined,
                    DEFAULT_LLM_OUTPUT_CAP,
                    "\n... [output truncated: use a more specific command to narrow results]",
                );

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
