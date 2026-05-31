// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::runtime::RuntimeAdapter;
use crate::security::SecurityPolicy;
use crate::security::job_object::{spawn_in_job, JobLimits, JobObjectGuard};
use crate::security::traits::Sandbox;
use crate::token_saver::{self, CompactedOutput};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_SHELL_TIMEOUT_SECS: u64 = 60;

const MAX_OUTPUT_BYTES: usize = 1_048_576;

const DEFAULT_LLM_OUTPUT_CAP: usize = 32_768;

#[cfg(not(target_os = "windows"))]
const SAFE_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "USER", "SHELL", "TMPDIR",
];

#[cfg(target_os = "windows")]
const SAFE_ENV_VARS: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "TEMP",
    "TMP",
    "TERM",
    "LANG",
    "USERNAME",
];

pub struct ShellTool {
    security: Arc<SecurityPolicy>,
    runtime: Arc<dyn RuntimeAdapter>,
    sandbox: Arc<dyn Sandbox>,
    timeout_secs: u64,

    job_limits: Option<JobLimits>,
}

impl ShellTool {
    pub fn new(security: Arc<SecurityPolicy>, runtime: Arc<dyn RuntimeAdapter>) -> Self {
        Self {
            security,
            runtime,
            sandbox: Arc::new(crate::security::NoopSandbox),
            timeout_secs: DEFAULT_SHELL_TIMEOUT_SECS,
            job_limits: None,
        }
    }

    pub fn new_with_sandbox(
        security: Arc<SecurityPolicy>,
        runtime: Arc<dyn RuntimeAdapter>,
        sandbox: Arc<dyn Sandbox>,
    ) -> Self {

        let job_limits = if sandbox.name() == "windows-job-object" {
            Some(JobLimits::default())
        } else {
            None
        };
        Self {
            security,
            runtime,
            sandbox,
            timeout_secs: DEFAULT_SHELL_TIMEOUT_SECS,
            job_limits,
        }
    }

    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_job_limits(mut self, limits: Option<JobLimits>) -> Self {
        self.job_limits = limits;
        self
    }
}

async fn spawn_background(
    mut cmd: tokio::process::Command,
    command_text: &str,
) -> anyhow::Result<ToolResult> {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to spawn background command: {e}")),
            });
        }
    };

    let id = format!(
        "bg-{}",
        uuid::Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let session_id = crate::session::current_session_context().map(|c| c.session_id);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    super::super::background_registry::register(
        id.clone(),
        command_text.to_string(),
        kill_tx,
        session_id.clone(),
    );

    if let Some(mut out) = stdout {
        let id_clone = id.clone();
        let sid_clone = session_id.clone();
        crate::runtime::spawn_supervised("tools.shell.bg.stdout", async move {
            use tokio::io::AsyncReadExt;
            let mut raw = Vec::new();
            let _ = out.read_to_end(&mut raw).await;
            let text = crate::util::decode_subprocess_bytes(&raw);
            for line in text.lines() {
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Chunk {
                        id: id_clone.clone(),
                        stream: super::super::background_registry::BgStream::Stdout,
                        line: line.to_string(),
                        session_id: sid_clone.clone(),
                    },
                );
            }
        });
    }
    if let Some(mut err) = stderr {
        let id_clone = id.clone();
        let sid_clone = session_id.clone();
        crate::runtime::spawn_supervised("tools.shell.bg.stderr", async move {
            use tokio::io::AsyncReadExt;
            let mut raw = Vec::new();
            let _ = err.read_to_end(&mut raw).await;
            let text = crate::util::decode_subprocess_bytes(&raw);
            for line in text.lines() {
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Chunk {
                        id: id_clone.clone(),
                        stream: super::super::background_registry::BgStream::Stderr,
                        line: line.to_string(),
                        session_id: sid_clone.clone(),
                    },
                );
            }
        });
    }

    let id_for_watchdog = id.clone();
    let sid_for_watchdog = session_id.clone();
    crate::runtime::spawn_supervised("tools.shell.bg.watchdog", async move {
        let started = std::time::Instant::now();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.tick().await;
        let exit_status = loop {
            tokio::select! {
                _ = tick.tick() => {
                    super::super::background_registry::publish(
                        super::super::background_registry::BackgroundShellSignal::Heartbeat {
                            id: id_for_watchdog.clone(),
                            elapsed_secs: started.elapsed().as_secs(),
                            session_id: sid_for_watchdog.clone(),
                        },
                    );
                }
                _ = &mut kill_rx => {
                    let _ = child.kill().await;
                    let status = child.wait().await.ok();
                    break status;
                }
                status = child.wait() => {
                    break status.ok();
                }
            }
        };
        let exit_code = exit_status.and_then(|s| s.code());
        super::super::background_registry::publish(
            super::super::background_registry::BackgroundShellSignal::Exited {
                id: id_for_watchdog.clone(),
                elapsed_secs: started.elapsed().as_secs(),
                exit_code,
                session_id: sid_for_watchdog.clone(),
            },
        );
        super::super::background_registry::unregister(&id_for_watchdog);
    });

    Ok(ToolResult {
        success: true,
        output: format!(
            "[background-shell:{id}] command spawned\n\
             $ {command_text}\n\
             Live stdout/stderr is streaming to the GUI's background-shell card.\n\
             Use the GUI 'Stop' button or `KillBackgroundShell {{ id: \"{id}\" }}` to terminate."
        ),
        error: None,
    })
}

fn is_valid_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

const MIRROR_MAX_LINES_PER_STREAM: usize = 2048;

fn emit_mirror_chunks(
    id: &str,
    body: &str,
    stream: super::super::background_registry::BgStream,
    session_id: Option<&str>,
) {
    if body.is_empty() {
        return;
    }
    let sid_owned = session_id.map(|s| s.to_string());
    for (count, line) in body.split_inclusive('\n').enumerate() {
        if count >= MIRROR_MAX_LINES_PER_STREAM {
            super::super::background_registry::publish(
                super::super::background_registry::BackgroundShellSignal::Chunk {
                    id: id.to_string(),
                    stream,
                    line: "... [mirror output truncated; agent still sees full result]\n"
                        .to_string(),
                    session_id: sid_owned.clone(),
                },
            );
            break;
        }
        super::super::background_registry::publish(
            super::super::background_registry::BackgroundShellSignal::Chunk {
                id: id.to_string(),
                stream,
                line: line.to_string(),
                session_id: sid_owned.clone(),
            },
        );
    }
}

fn collect_allowed_shell_env_vars(security: &SecurityPolicy) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for key in SAFE_ENV_VARS
        .iter()
        .copied()
        .chain(security.shell_env_passthrough.iter().map(|s| s.as_str()))
    {
        let candidate = key.trim();
        if candidate.is_empty() || !is_valid_env_var_name(candidate) {
            continue;
        }
        if seen.insert(candidate.to_string()) {
            out.push(candidate.to_string());
        }
    }
    out
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        concat!(
            "Execute a shell command in the workspace directory. ",
            "**For long-running processes** (HTTP servers like `python -m http.server`, `vite`, `next dev`, ",
            "`cargo watch`, `npm run dev`, `tail -f`, etc.), set `background: true`  - otherwise the default ",
            "60s timeout will kill the process and any subsequent `browser` navigate to its URL will fail with ",
            "`ERR_CONNECTION_REFUSED`. Background mode returns a `bg-<id>` handle immediately so you can keep ",
            "issuing other tool calls (e.g. `browser` open) in parallel."
        )
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "approved": {
                    "type": "boolean",
                    "description": "Set true to explicitly approve medium/high-risk commands in supervised mode",
                    "default": false
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Override the default timeout in milliseconds (default: 60000). Use higher values for long-running commands."
                },
                "compact": {
                    "type": "boolean",
                    "description": "Enable token-saver output compaction. Defaults to true. Set false to receive the raw, uncompacted command output (still subject to the 32KB safety cap).",
                    "default": true
                },
                "background": {
                    "type": "boolean",
                    "description": "When true, spawn the command in the background and return immediately with a 'bg-<id>' handle. The GUI shows live stdout/stderr via the BackgroundShell card and the agent can issue further tool calls in parallel. Use for long-running watchers like `cargo watch`, `ping`, dev servers.",
                    "default": false
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;
        let approved = args
            .get("approved")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        match self.security.validate_command_execution(command, approved) {
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

        let mut cmd = match self
            .runtime
            .build_shell_command(command, &self.security.workspace_dir())
        {
            Ok(cmd) => cmd,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to build runtime command: {e}")),
                });
            }
        };

        self.sandbox
            .wrap_command(cmd.as_std_mut())
            .map_err(|e| anyhow::anyhow!("Sandbox error: {}", e))?;

        if self.security.should_filter_shell_env() {
            cmd.env_clear();

            for var in collect_allowed_shell_env_vars(&self.security) {
                if let Ok(val) = std::env::var(&var) {
                    cmd.env(&var, val);
                }
            }
        }

        for (k, v) in crate::python_env::activation_env(&self.security.workspace_dir()) {
            cmd.env(k, v);
        }
        cmd.env_remove("PYTHONHOME");

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());
        cmd.kill_on_drop(true);

        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if background {
            return spawn_background(cmd, command).await;
        }

        let timeout_duration = if let Some(ms) = args.get("timeout_ms").and_then(|v| v.as_u64()) {
            Duration::from_millis(ms)
        } else {
            Duration::from_secs(self.timeout_secs)
        };
        let timeout_secs = timeout_duration.as_secs();
        let job_limits = self.job_limits;

        let mirror_id = format!(
            "sync-{}",
            uuid::Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>(),
        );
        let mirror_session_id = crate::session::current_session_context().map(|c| c.session_id);
        let mirror_started = std::time::Instant::now();
        super::super::background_registry::publish(
            super::super::background_registry::BackgroundShellSignal::Spawned {
                id: mirror_id.clone(),
                command: command.to_string(),
                session_id: mirror_session_id.clone(),
            },
        );

        let spawn_result: std::io::Result<(Option<JobObjectGuard>, tokio::process::Child)> =
            if let Some(limits) = job_limits {
                match spawn_in_job(cmd, limits).await {
                    Ok((g, c)) => Ok((Some(g), c)),
                    Err(e) => Err(e),
                }
            } else {
                match cmd.spawn() {
                    Ok(c) => Ok((None, c)),
                    Err(e) => Err(e),
                }
            };
        let (_job_guard, mut child): (Option<JobObjectGuard>, tokio::process::Child) =
            match spawn_result {
                Ok(pair) => pair,
                Err(e) => {
                let error_text = format!("Failed to execute command: {e}");
                emit_mirror_chunks(
                    &mirror_id,
                    &format!("{error_text}\n"),
                    super::super::background_registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Exited {
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

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let (stdout_tx, stdout_rx) = tokio::sync::oneshot::channel();
        crate::runtime::spawn_supervised("tools.shell.stdout", async move {
            let mut buf = String::new();
            if let Some(mut out) = stdout_pipe {
                use tokio::io::AsyncReadExt;
                let mut raw = Vec::new();
                let _ = out.read_to_end(&mut raw).await;
                buf = crate::util::decode_subprocess_bytes(&raw);
            }
            let _ = stdout_tx.send(buf);
        });

        let (stderr_tx, stderr_rx) = tokio::sync::oneshot::channel();
        crate::runtime::spawn_supervised("tools.shell.stderr", async move {
            let mut buf = String::new();
            if let Some(mut err) = stderr_pipe {
                use tokio::io::AsyncReadExt;
                let mut raw = Vec::new();
                let _ = err.read_to_end(&mut raw).await;
                buf = crate::util::decode_subprocess_bytes(&raw);
            }
            let _ = stderr_tx.send(buf);
        });

        enum WaitOutcome {
            Exited(std::process::ExitStatus),
            WaitError(std::io::Error),
            Timeout,
        }

        let wait_outcome = match tokio::time::timeout(timeout_duration, child.wait()).await {
            Ok(Ok(status)) => WaitOutcome::Exited(status),
            Ok(Err(e)) => WaitOutcome::WaitError(e),
            Err(_) => {
                let _ = child.start_kill();
                if tokio::time::timeout(Duration::from_secs(3), child.wait())
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        command = %command,
                        timeout_secs,
                        "shell child did not exit within 3s after kill; treating as orphan"
                    );
                }
                WaitOutcome::Timeout
            }
        };

        let drained_stdout = tokio::time::timeout(Duration::from_millis(500), stdout_rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
        let drained_stderr = tokio::time::timeout(Duration::from_millis(500), stderr_rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let result: Result<Result<(std::process::ExitStatus, String, String), anyhow::Error>, (String, String)> =
            match wait_outcome {
                WaitOutcome::Exited(status) => Ok(Ok((status, drained_stdout, drained_stderr))),
                WaitOutcome::WaitError(e) => Ok(Err(anyhow::Error::from(e))),
                WaitOutcome::Timeout => Err((drained_stdout, drained_stderr)),
            };

        match result {
            Ok(Ok((status, mut stdout, mut stderr))) => {
                emit_mirror_chunks(
                    &mirror_id,
                    &stdout,
                    super::super::background_registry::BgStream::Stdout,
                    mirror_session_id.as_deref(),
                );
                emit_mirror_chunks(
                    &mirror_id,
                    &stderr,
                    super::super::background_registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Exited {
                        id: mirror_id.clone(),
                        elapsed_secs: mirror_started.elapsed().as_secs(),
                        exit_code: status.code(),
                        session_id: mirror_session_id.clone(),
                    },
                );

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

                let compact_requested = args
                    .get("compact")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let exit_code = status.code().unwrap_or(if status.success() { 0 } else { -1 });
                let compacted: CompactedOutput = if compact_requested && token_saver::is_enabled() {
                    let ctx = token_saver::global();
                    token_saver::compact_command_output(command, &stdout, &stderr, exit_code, &ctx)
                } else {
                    CompactedOutput::passthrough(stdout.clone(), stderr.clone())
                };
                stdout = compacted.stdout;
                stderr = compacted.stderr;

                if stdout.len() > DEFAULT_LLM_OUTPUT_CAP {
                    let total = stdout.len();
                    let mut b = DEFAULT_LLM_OUTPUT_CAP.min(total);
                    while b > 0 && !stdout.is_char_boundary(b) {
                        b -= 1;
                    }
                    tracing::debug!(
                        target: "shell.output_truncated",
                        command = %command,
                        total_bytes = total,
                        shown_bytes = b,
                        stdout_full = %stdout,
                        "shell stdout exceeded LLM cap; full content logged at debug",
                    );
                    stdout.truncate(b);
                    stdout.push_str(&format!(
                        "\n... [output truncated: showing {b}/{total} bytes. \
                         Use `head`/`tail`/`grep` to filter if needed]"
                    ));
                }
                if stderr.len() > DEFAULT_LLM_OUTPUT_CAP {
                    let total = stderr.len();
                    let mut b = DEFAULT_LLM_OUTPUT_CAP.min(total);
                    while b > 0 && !stderr.is_char_boundary(b) {
                        b -= 1;
                    }
                    tracing::debug!(
                        target: "shell.output_truncated",
                        command = %command,
                        total_bytes = total,
                        shown_bytes = b,
                        stderr_full = %stderr,
                        "shell stderr exceeded LLM cap; full content logged at debug",
                    );
                    stderr.truncate(b);
                    stderr.push_str(&format!(
                        "\n... [stderr truncated: showing {b}/{total} bytes]"
                    ));
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
            Ok(Err(e)) => {
                let error_text = format!("Failed to execute command: {e}");
                emit_mirror_chunks(
                    &mirror_id,
                    &format!("{error_text}\n"),
                    super::super::background_registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Exited {
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
            Err((partial_stdout, partial_stderr)) => {
                let mut detail = String::new();
                if !partial_stdout.is_empty() {
                    detail.push_str("--- partial stdout before timeout ---\n");
                    detail.push_str(&partial_stdout);
                    if !detail.ends_with('\n') {
                        detail.push('\n');
                    }
                }
                if !partial_stderr.is_empty() {
                    detail.push_str("--- partial stderr before timeout ---\n");
                    detail.push_str(&partial_stderr);
                    if !detail.ends_with('\n') {
                        detail.push('\n');
                    }
                }
                let banner = format!(
                    "Command timed out after {timeout_secs}s and was killed. \
                     DO NOT retry the same command verbatim. \
                     For long-running installs / builds (pip, npm, cargo, apt), \
                     either pass a larger `timeout_ms` or set `background: true` and poll, \
                     or split the work into smaller steps. \
                     If the command is genuinely interactive or hung, report the situation \
                     to the user via the ask tool instead of retrying."
                );
                let error_text = if detail.is_empty() {
                    banner
                } else {
                    format!("{banner}\n{detail}")
                };
                emit_mirror_chunks(
                    &mirror_id,
                    &format!("{error_text}\n"),
                    super::super::background_registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Exited {
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
        }
    }
}
