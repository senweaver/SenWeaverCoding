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

fn workspace_build_lock_enabled() -> bool {
    crate::util::get_runtime_var("SEN_WORKSPACE_BUILD_LOCK")
        .map(|v| {
            let t = v.trim();
            t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

// Heuristic: does this command mutate shared build/VCS state such that two
// concurrent same-directory runs would conflict? Kept intentionally narrow so
// read-only commands still run in parallel across sessions.
fn command_is_build_like(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "cargo build",
        "cargo test",
        "cargo run",
        "cargo check",
        "cargo clippy",
        "cargo fix",
        "npm install",
        "npm ci",
        "npm run build",
        "pnpm install",
        "pnpm build",
        "yarn install",
        "yarn build",
        "bun install",
        "bun run build",
        "go build",
        "go test",
        "make",
        "cmake --build",
        "gradle",
        "mvn ",
        "pip install",
        "git checkout",
        "git merge",
        "git rebase",
        "git pull",
        "git reset",
        "git stash",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

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

const BACKGROUND_STREAM_CAP: usize = 1_048_576;

async fn stream_background_output<R>(
    reader: R,
    id: String,
    stream: super::super::background_registry::BgStream,
    session_id: Option<String>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut buffered = BufReader::new(reader);
    let mut line_bytes: Vec<u8> = Vec::new();
    let mut emitted = 0usize;
    let mut truncated = false;
    loop {
        line_bytes.clear();
        match buffered.read_until(b'\n', &mut line_bytes).await {
            Ok(0) => break,
            Ok(n) => {
                if truncated {
                    continue;
                }
                emitted = emitted.saturating_add(n);
                if emitted > BACKGROUND_STREAM_CAP {
                    truncated = true;
                    super::super::background_registry::publish(
                        super::super::background_registry::BackgroundShellSignal::Chunk {
                            id: id.clone(),
                            stream,
                            line:
                                "... [background output truncated at 1MB; process still running]"
                                    .to_string(),
                            session_id: session_id.clone(),
                        },
                    );
                    continue;
                }
                let mut text = crate::util::decode_subprocess_bytes(&line_bytes);
                while text.ends_with('\n') || text.ends_with('\r') {
                    text.pop();
                }
                super::super::background_registry::publish(
                    super::super::background_registry::BackgroundShellSignal::Chunk {
                        id: id.clone(),
                        stream,
                        line: text,
                        session_id: session_id.clone(),
                    },
                );
            }
            Err(_) => break,
        }
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

    if let Some(out) = stdout {
        let id_clone = id.clone();
        let sid_clone = session_id.clone();
        crate::runtime::spawn_supervised("tools.shell.bg.stdout", async move {
            stream_background_output(
                out,
                id_clone,
                super::super::background_registry::BgStream::Stdout,
                sid_clone,
            )
            .await;
        });
    }
    if let Some(err) = stderr {
        let id_clone = id.clone();
        let sid_clone = session_id.clone();
        crate::runtime::spawn_supervised("tools.shell.bg.stderr", async move {
            stream_background_output(
                err,
                id_clone,
                super::super::background_registry::BgStream::Stderr,
                sid_clone,
            )
            .await;
        });
    }

    let id_for_watchdog = id.clone();
    let sid_for_watchdog = session_id.clone();
    let max_lifetime_secs = std::env::var("SEN_BACKGROUND_SHELL_MAX_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0);
    crate::runtime::spawn_supervised("tools.shell.bg.watchdog", async move {
        let started = std::time::Instant::now();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.tick().await;
        let lifetime_cap = async {
            match max_lifetime_secs {
                Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(lifetime_cap);
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
                    crate::util::kill_child_process_tree(&mut child).await;
                    let status = child.wait().await.ok();
                    break status;
                }
                _ = &mut lifetime_cap => {
                    tracing::warn!(
                        target: "tools.shell.bg",
                        id = %id_for_watchdog,
                        max_secs = max_lifetime_secs.unwrap_or(0),
                        "background shell exceeded configured max lifetime; terminating"
                    );
                    crate::util::kill_child_process_tree(&mut child).await;
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

fn is_sensitive_env_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const NEEDLES: &[&str] = &[
        "API_KEY",
        "APIKEY",
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "PRIVATE_KEY",
        "PRIVATEKEY",
        "ACCESS_KEY",
        "SESSION_KEY",
        "BEARER",
        "OPENAI",
        "ANTHROPIC",
        "GEMINI",
        "CLAUDE",
        "AWS_SECRET",
        "AWS_SESSION",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "SENAGENTOS",
    ];
    NEEDLES.iter().any(|n| upper.contains(n))
}

const MIRROR_MAX_LINES_PER_STREAM: usize = 2048;

pub(crate) fn emit_mirror_chunks(
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
        #[cfg(target_os = "windows")]
        {
            concat!(
                "Execute a shell command in the workspace directory via `cmd.exe /C` (Windows, NOT bash). ",
                "Unix utilities like `grep`, `head`, `tail`, `wc`, `sed`, `awk`, `cat`, `ls` are NOT available ",
                "and will error with 'not recognized as an internal or external command'. Use the `content_search` ",
                "tool to search file contents, the `read_file` tool to read files, and Windows/CMD equivalents ",
                "(`dir`, `type`, `findstr`, `where`, `more`) or the `powershell` tool for anything else. ",
                "**For long-running processes** (HTTP servers like `python -m http.server`, `vite`, `next dev`, ",
                "`cargo watch`, `npm run dev`, etc.), set `background: true`  - otherwise the default ",
                "60s timeout will kill the process and any subsequent `browser` navigate to its URL will fail with ",
                "`ERR_CONNECTION_REFUSED`. Background mode returns a `bg-<id>` handle immediately so you can keep ",
                "issuing other tool calls (e.g. `browser` open) in parallel."
            )
        }
        #[cfg(not(target_os = "windows"))]
        {
            concat!(
                "Execute a shell command in the workspace directory. ",
                "**For long-running processes** (HTTP servers like `python -m http.server`, `vite`, `next dev`, ",
                "`cargo watch`, `npm run dev`, `tail -f`, etc.), set `background: true`  - otherwise the default ",
                "60s timeout will kill the process and any subsequent `browser` navigate to its URL will fail with ",
                "`ERR_CONNECTION_REFUSED`. Background mode returns a `bg-<id>` handle immediately so you can keep ",
                "issuing other tool calls (e.g. `browser` open) in parallel."
            )
        }
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let command_desc = if cfg!(target_os = "windows") {
            "The shell command to execute (runs via cmd.exe; use Windows syntax, not Unix tools like grep/head/tail)"
        } else {
            "The shell command to execute"
        };
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": command_desc
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

        // Opt-in cross-session serialization for build/VCS commands so two
        // parallel sessions sharing a directory cannot run conflicting builds
        // simultaneously. Enabled via SEN_WORKSPACE_BUILD_LOCK=1.
        let _workspace_guard = if workspace_build_lock_enabled()
            && command_is_build_like(command)
        {
            match crate::session::acquire_workspace_exclusive_for_current_session().await {
                Some(Ok(g)) => Some(g),
                Some(Err(e)) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("{e}")),
                    });
                }
                None => None,
            }
        } else {
            None
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
        } else {
            let passthrough: HashSet<&str> = self
                .security
                .shell_env_passthrough
                .iter()
                .map(|s| s.as_str())
                .collect();
            for (key, _) in std::env::vars_os() {
                if let Some(k) = key.to_str() {
                    if is_sensitive_env_var(k) && !passthrough.contains(k) {
                        cmd.env_remove(k);
                    }
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
        let (_job_guard, child): (Option<JobObjectGuard>, tokio::process::Child) =
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

        let outcome = super::foreground::run_foreground_streamed(
            child,
            &mirror_id,
            mirror_session_id.as_deref(),
            mirror_started,
            timeout_duration,
        )
        .await;

        if let super::foreground::ForegroundOutcome::Cancelled(part_stdout, part_stderr) = &outcome {
            super::super::background_registry::publish(
                super::super::background_registry::BackgroundShellSignal::Exited {
                    id: mirror_id.clone(),
                    elapsed_secs: mirror_started.elapsed().as_secs(),
                    exit_code: None,
                    session_id: mirror_session_id.clone(),
                },
            );
            return Ok(ToolResult {
                success: true,
                output: super::foreground::build_cancelled_output(part_stdout, part_stderr),
                error: None,
            });
        }

        let result: Result<Result<(std::process::ExitStatus, String, String), anyhow::Error>, (String, String)> =
            match outcome {
                super::foreground::ForegroundOutcome::Exited(status, stdout, stderr) => {
                    Ok(Ok((status, stdout, stderr)))
                }
                super::foreground::ForegroundOutcome::WaitError(e) => {
                    Ok(Err(anyhow::Error::from(e)))
                }
                super::foreground::ForegroundOutcome::Timeout(stdout, stderr) => {
                    Err((stdout, stderr))
                }
                super::foreground::ForegroundOutcome::Cancelled(part_stdout, part_stderr) => {
                    return Ok(ToolResult {
                        success: true,
                        output: super::foreground::build_cancelled_output(
                            &part_stdout,
                            &part_stderr,
                        ),
                        error: None,
                    });
                }
            };

        match result {
            Ok(Ok((status, mut stdout, mut stderr))) => {
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
                    let filter_hint = if cfg!(target_os = "windows") {
                        "Use `findstr`/`more` or the `content_search` tool to filter if needed"
                    } else {
                        "Use `head`/`tail`/`grep` to filter if needed"
                    };
                    stdout.push_str(&format!(
                        "\n... [output truncated: showing {b}/{total} bytes. {filter_hint}]"
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
                    banner.clone()
                } else {
                    format!("{banner}\n{detail}")
                };
                emit_mirror_chunks(
                    &mirror_id,
                    &format!("{banner}\n"),
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
