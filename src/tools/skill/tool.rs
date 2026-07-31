// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;

const SKILL_SHELL_TIMEOUT_SECS: u64 = 60;

const MAX_OUTPUT_BYTES: usize = 1_048_576;

pub struct SkillShellTool {
    tool_name: String,
    tool_description: String,
    command_template: String,
    args: HashMap<String, String>,
    security: Arc<SecurityPolicy>,
}

impl SkillShellTool {

    pub fn new(
        skill_name: &str,
        tool: &crate::skills::SkillTool,
        security: Arc<SecurityPolicy>,
    ) -> Self {
        let tool_name = format!("{}.{}", skill_name, tool.name);
        crate::hooks::script_runner::register_shell_capable_tool(&tool_name);
        Self {
            tool_name,
            tool_description: tool.description.clone(),
            command_template: tool.command.clone(),
            args: tool.args.clone(),
            security,
        }
    }

    fn build_parameters_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for (name, description) in &self.args {
            properties.insert(
                name.clone(),
                serde_json::json!({
                    "type": "string",
                    "description": description
                }),
            );
            required.push(serde_json::Value::String(name.clone()));
        }

        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }

    fn substitute_args(&self, args: &serde_json::Value) -> String {
        let mut command = self.command_template.clone();
        if let Some(obj) = args.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{{{}}}}}", key);
                let raw = value.as_str().unwrap_or_default();
                let replacement = shell_escape_arg(raw);
                command = command.replace(&placeholder, &replacement);
            }
        }
        command
    }
}

fn shell_escape_arg(value: &str) -> String {
    #[cfg(windows)]
    {
        if value.contains(['\r', '\n', '\0']) {
            return "\"\"".to_string();
        }
        let escaped = value.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }
    #[cfg(not(windows))]
    {
        if value.contains('\0') {
            return "''".to_string();
        }
        let escaped = value.replace('\'', "'\\''");
        format!("'{escaped}'")
    }
}

#[async_trait]
impl Tool for SkillShellTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.build_parameters_schema()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let command = self.substitute_args(&args);

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        let command_approved = crate::agent::loop_::current_tool_runtime_approved();
        match self
            .security
            .validate_command_execution(&command, command_approved)
        {
            Ok(_) => {}
            Err(reason) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(reason),
                });
            }
        }

        if let Some(path) = self.security.forbidden_path_argument(&command) {
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

        if let Err(reason) = crate::security::detect::ensure_sandbox_available() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(reason),
            });
        }

        #[cfg(windows)]
        let mut cmd = {
            use std::os::windows::process::CommandExt;
            let mut c = crate::util::hidden_sync_command("cmd.exe");
            c.arg("/S").arg("/C").raw_arg(&command);
            tokio::process::Command::from(c)
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = crate::util::hidden_async_command("sh");
            c.arg("-c").arg(&command);
            c
        };
        cmd.current_dir(self.security.workspace_dir());
        cmd.env_clear();

        const UNIX_ENV_PASSTHROUGH: &[&str] = &[
            "PATH", "HOME", "TERM", "LANG", "LC_ALL", "USER", "SHELL", "TMPDIR",
        ];
        const WINDOWS_ENV_PASSTHROUGH: &[&str] = &[
            "PATH",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "TEMP",
            "TMP",
            "SYSTEMROOT",
            "WINDIR",
            "COMSPEC",
            "PATHEXT",
        ];
        let passthrough = if cfg!(windows) {
            WINDOWS_ENV_PASSTHROUGH
        } else {
            UNIX_ENV_PASSTHROUGH
        };
        for var in passthrough {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let (_job_guard, mut child) = match crate::security::job_object::spawn_in_job(
            cmd,
            crate::security::job_object::JobLimits::unlimited(),
        )
        .await
        {
            Ok(pair) => pair,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to execute command: {e}")),
                });
            }
        };

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stdout_pipe {
                let _ = pipe.read_to_end(&mut buf).await;
            }
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stderr_pipe {
                let _ = pipe.read_to_end(&mut buf).await;
            }
            buf
        });

        let status = match tokio::time::timeout(
            Duration::from_secs(SKILL_SHELL_TIMEOUT_SECS),
            child.wait(),
        )
        .await
        {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to execute command: {e}")),
                });
            }
            Err(_) => {
                crate::util::kill_child_process_tree(&mut child).await;
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Command timed out after {SKILL_SHELL_TIMEOUT_SECS}s and was killed"
                    )),
                });
            }
        };

        let stdout_bytes = stdout_task.await.unwrap_or_default();
        let stderr_bytes = stderr_task.await.unwrap_or_default();

        let mut stdout = crate::util::decode_subprocess_bytes(&stdout_bytes);
        let mut stderr = crate::util::decode_subprocess_bytes(&stderr_bytes);

        if stdout.len() > MAX_OUTPUT_BYTES {
            let mut b = MAX_OUTPUT_BYTES.min(stdout.len());
            while b > 0 && !stdout.is_char_boundary(b) {
                b -= 1;
            }
            stdout.truncate(b);
            stdout.push_str("\n... [output truncated at 1MB]");
        }
        if stderr.len() > MAX_OUTPUT_BYTES {
            let mut b = MAX_OUTPUT_BYTES.min(stderr.len());
            while b > 0 && !stderr.is_char_boundary(b) {
                b -= 1;
            }
            stderr.truncate(b);
            stderr.push_str("\n... [stderr truncated at 1MB]");
        }

        Ok(ToolResult {
            success: status.success(),
            output: stdout,
            error: if stderr.is_empty() {
                None
            } else {
                Some(stderr)
            },
        })
    }
}
