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

pub(crate) fn workspace_build_lock_enabled() -> bool {
    crate::util::get_runtime_var("SEN_WORKSPACE_BUILD_LOCK")
        .map(|v| {
            let t = v.trim();
            !(t == "0"
                || t.eq_ignore_ascii_case("false")
                || t.eq_ignore_ascii_case("no")
                || t.eq_ignore_ascii_case("off"))
        })
        .unwrap_or(true)
}

pub(crate) fn extract_shell_write_targets(command: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let tokens: Vec<String> = tokenize_shell_words(command);

    for (i, tok) in tokens.iter().enumerate() {
        let t = tok.as_str();
        let redir_body = t
            .strip_prefix("2>>")
            .or_else(|| t.strip_prefix("1>>"))
            .or_else(|| t.strip_prefix(">>"))
            .or_else(|| t.strip_prefix("2>"))
            .or_else(|| t.strip_prefix("1>"))
            .or_else(|| t.strip_prefix('>'));
        if let Some(rest) = redir_body {
            if !rest.is_empty() && rest != "&1" && rest != "&2" {
                targets.push(rest.to_string());
            } else if let Some(next) = tokens.get(i + 1) {
                if !next.starts_with('&') {
                    targets.push(next.clone());
                }
            }
        }
    }

    let head = tokens
        .first()
        .map(|s| {
            std::path::Path::new(s)
                .file_name()
                .map(|f| f.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_else(|| s.to_ascii_lowercase())
        })
        .unwrap_or_default();
    const WRITE_CMDS: &[&str] = &["tee", "cp", "mv", "install", "touch", "truncate"];
    if WRITE_CMDS.contains(&head.as_str()) {
        for tok in tokens.iter().skip(1) {
            if !tok.starts_with('-') && !tok.contains('=') {
                targets.push(tok.clone());
            }
        }
    }
    for tok in &tokens {
        if let Some(rest) = tok.strip_prefix("of=") {
            if !rest.is_empty() {
                targets.push(rest.to_string());
            }
        }
    }

    targets
        .into_iter()
        .map(|t| t.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|t| !t.is_empty() && t != "/dev/null" && t != "nul" && !t.starts_with('$'))
        .collect()
}

fn tokenize_shell_words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in command.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                '|' | ';' | '&' => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                c => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub(crate) fn command_is_build_like(command: &str) -> bool {
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

    pub fn with_resource_limits(
        self,
        resources: &crate::config::schema::ResourceLimitsConfig,
    ) -> Self {
        let merged = self
            .job_limits
            .map(|limits| limits.with_resource_overrides(resources));
        self.with_job_limits(merged)
    }
}

const BACKGROUND_STREAM_CAP: usize = 1_048_576;

async fn stream_background_output<R>(
    reader: R,
    id: String,
    stream: super::super::background::registry::BgStream,
    session_id: Option<String>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::BufReader;
    let mut buffered = BufReader::new(reader);
    let mut line_bytes: Vec<u8> = Vec::new();
    let mut emitted = 0usize;
    let mut truncated = false;
    loop {
        line_bytes.clear();
        match super::foreground::read_line_capped(
            &mut buffered,
            &mut line_bytes,
            BACKGROUND_STREAM_CAP,
        )
        .await
        {
            Ok(0) => break,
            Ok(n) => {
                if truncated {
                    continue;
                }
                emitted = emitted.saturating_add(n);
                if emitted > BACKGROUND_STREAM_CAP {
                    truncated = true;
                    super::super::background::registry::publish(
                        super::super::background::registry::BackgroundShellSignal::Chunk {
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
                super::super::background::registry::publish(
                    super::super::background::registry::BackgroundShellSignal::Chunk {
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
    cmd: tokio::process::Command,
    command_text: &str,
    job_limits: Option<JobLimits>,
) -> anyhow::Result<ToolResult> {
    let (job_guard, mut child): (Option<JobObjectGuard>, tokio::process::Child) =
        match spawn_with_job_limits(cmd, job_limits).await {
            Ok(pair) => pair,
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
    super::super::background::registry::register(
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
                super::super::background::registry::BgStream::Stdout,
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
                super::super::background::registry::BgStream::Stderr,
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
        let _job_guard = job_guard;
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
                    super::super::background::registry::publish(
                        super::super::background::registry::BackgroundShellSignal::Heartbeat {
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
        super::super::background::registry::publish(
            super::super::background::registry::BackgroundShellSignal::Exited {
                id: id_for_watchdog.clone(),
                elapsed_secs: started.elapsed().as_secs(),
                exit_code,
                session_id: sid_for_watchdog.clone(),
            },
        );
        super::super::background::registry::unregister(&id_for_watchdog);
    });

    Ok(ToolResult {
        success: true,
        output: format!(
            "[background-shell:{id}] command spawned\n\
             $ {command_text}\n\
             Poll `background_status` (id: \"{id}\") for liveness/exit code, \
             read output with `background_logs`, and stop it with `background_kill`."
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

pub(crate) fn is_sensitive_env_var(name: &str) -> bool {
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
    stream: super::super::background::registry::BgStream,
    session_id: Option<&str>,
) {
    if body.is_empty() {
        return;
    }
    let sid_owned = session_id.map(|s| s.to_string());
    for (count, line) in body.split_inclusive('\n').enumerate() {
        if count >= MIRROR_MAX_LINES_PER_STREAM {
            super::super::background::registry::publish(
                super::super::background::registry::BackgroundShellSignal::Chunk {
                    id: id.to_string(),
                    stream,
                    line: "... [mirror output truncated; agent still sees full result]\n"
                        .to_string(),
                    session_id: sid_owned.clone(),
                },
            );
            break;
        }
        super::super::background::registry::publish(
            super::super::background::registry::BackgroundShellSignal::Chunk {
                id: id.to_string(),
                stream,
                line: line.to_string(),
                session_id: sid_owned.clone(),
            },
        );
    }
}

pub(crate) fn collect_allowed_shell_env_vars(security: &SecurityPolicy) -> Vec<String> {
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

pub(crate) fn validate_shell_write_targets(
    security: &SecurityPolicy,
    command: &str,
) -> Option<String> {
    let ws = security.workspace_dir();
    for target in extract_shell_write_targets(command) {
        let resolved = if std::path::Path::new(&target).is_absolute() {
            std::path::PathBuf::from(&target)
        } else {
            ws.join(&target)
        };
        if !crate::security::sandbox::sandbox_allows_path(&resolved) {
            return Some(format!(
                "Shell write target '{target}' is outside the sandbox workspace \
                 confinement. Add it to [autonomy].allowed_roots, or disable \
                 [security.sandbox].confine_filesystem to permit it."
            ));
        }
    }
    None
}

pub(crate) fn prepare_isolated_command(
    cmd: &mut tokio::process::Command,
    security: &SecurityPolicy,
    sandbox: &dyn Sandbox,
) -> std::io::Result<()> {
    sandbox.wrap_command(cmd.as_std_mut())?;
    if security.should_filter_shell_env() {
        cmd.env_clear();
        for var in collect_allowed_shell_env_vars(security) {
            if let Ok(val) = std::env::var(&var) {
                cmd.env(&var, val);
            }
        }
    } else {
        let passthrough: HashSet<&str> = security
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
    for (k, v) in crate::python_env::activation_env(&security.workspace_dir()) {
        cmd.env(k, v);
    }
    cmd.env_remove("PYTHONHOME");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    cmd.kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    Ok(())
}

pub(crate) async fn spawn_with_job_limits(
    mut cmd: tokio::process::Command,
    job_limits: Option<JobLimits>,
) -> std::io::Result<(Option<JobObjectGuard>, tokio::process::Child)> {
    match job_limits {
        Some(limits) => {
            let (guard, child) = spawn_in_job(cmd, limits).await?;
            Ok((Some(guard), child))
        }
        None => Ok((None, cmd.spawn()?)),
    }
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
                "timeout_ms": {
                    "type": "integer",
                    "description": "Override the default timeout in milliseconds (default: 60000). Use higher values for long-running commands."
                },
                "block_until_ms": {
                    "type": "integer",
                    "description": "Run in the foreground for at most this many ms; if the command has not finished by then it is auto-moved to the background and a 'bg-<id>' handle is returned so you can keep working and poll with background_status / background_logs / background_wait. Use for commands that may be long-running (dev servers, watchers, builds) without committing to background: true up front."
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

        let _preflight = match crate::tools::shell::preflight::acquire_shell_execution_clearance(
            &self.security,
            command,
        )
        .await
        {
            Ok(guards) => guards,
            Err(result) => return Ok(result),
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

        prepare_isolated_command(&mut cmd, &self.security, self.sandbox.as_ref())
            .map_err(|e| anyhow::anyhow!("Sandbox error: {}", e))?;

        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if background {
            return spawn_background(cmd, command, self.job_limits).await;
        }

        let timeout_duration = if let Some(ms) = args.get("timeout_ms").and_then(|v| v.as_u64()) {
            Duration::from_millis(ms)
        } else {
            Duration::from_secs(self.timeout_secs)
        };
        let timeout_secs = timeout_duration.as_secs();
        let job_limits = self.job_limits;

        let block_until = args
            .get("block_until_ms")
            .and_then(|v| v.as_u64())
            .filter(|ms| *ms > 0)
            .map(Duration::from_millis);
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
        super::super::background::registry::publish(
            super::super::background::registry::BackgroundShellSignal::Spawned {
                id: mirror_id.clone(),
                command: command.to_string(),
                session_id: mirror_session_id.clone(),
            },
        );

        let (job_guard, child): (Option<JobObjectGuard>, tokio::process::Child) =
            match spawn_with_job_limits(cmd, job_limits).await {
                Ok(pair) => pair,
                Err(e) => {
                let error_text = format!("Failed to execute command: {e}");
                emit_mirror_chunks(
                    &mirror_id,
                    &format!("{error_text}\n"),
                    super::super::background::registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background::registry::publish(
                    super::super::background::registry::BackgroundShellSignal::Exited {
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

        let outcome = super::foreground::run_foreground_streamed_inner(
            child,
            job_guard,
            &mirror_id,
            mirror_session_id.as_deref(),
            mirror_started,
            timeout_duration,
            block_until,
            command,
        )
        .await;

        if let super::foreground::ForegroundOutcome::Backgrounded {
            partial_stdout,
            partial_stderr,
        } = &outcome
        {
            let mut msg = format!(
                "Command still running after {}ms; moved to background as '{mirror_id}'. \
                 Poll with background_status / background_logs(id=\"{mirror_id}\") / \
                 background_wait(id=\"{mirror_id}\"), or stop it with background_kill.",
                block_until.map(|d| d.as_millis()).unwrap_or(0)
            );
            let preview: String = partial_stdout
                .lines()
                .chain(partial_stderr.lines())
                .take(20)
                .collect::<Vec<_>>()
                .join("\n");
            if !preview.trim().is_empty() {
                msg.push_str("\n\n--- output so far ---\n");
                msg.push_str(&preview);
            }
            return Ok(ToolResult {
                success: true,
                output: msg,
                error: None,
            });
        }

        if let super::foreground::ForegroundOutcome::Cancelled(part_stdout, part_stderr) = &outcome {
            super::super::background::registry::publish(
                super::super::background::registry::BackgroundShellSignal::Exited {
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
                super::foreground::ForegroundOutcome::Backgrounded { .. } => {
                    unreachable!("Backgrounded handled before result mapping");
                }
            };

        match result {
            Ok(Ok((status, mut stdout, mut stderr))) => {
                super::super::background::registry::publish(
                    super::super::background::registry::BackgroundShellSignal::Exited {
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

                if let Some(clipped) =
                    crate::util::truncate_head_tail(&stdout, DEFAULT_LLM_OUTPUT_CAP, 25)
                {
                    tracing::debug!(
                        target: "shell.output_truncated",
                        command = %command,
                        total_bytes = stdout.len(),
                        stdout_full = %stdout,
                        "shell stdout exceeded LLM cap; full content logged at debug",
                    );
                    stdout = clipped;
                    let filter_hint = if cfg!(target_os = "windows") {
                        "Use `findstr`/`more` or the `content_search` tool to filter if needed"
                    } else {
                        "Use `head`/`tail`/`grep` to filter if needed"
                    };
                    stdout.push_str(&format!("\n[{filter_hint}]"));
                }
                if let Some(clipped) =
                    crate::util::truncate_head_tail(&stderr, DEFAULT_LLM_OUTPUT_CAP, 25)
                {
                    tracing::debug!(
                        target: "shell.output_truncated",
                        command = %command,
                        total_bytes = stderr.len(),
                        stderr_full = %stderr,
                        "shell stderr exceeded LLM cap; full content logged at debug",
                    );
                    stderr = clipped;
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
                    super::super::background::registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background::registry::publish(
                    super::super::background::registry::BackgroundShellSignal::Exited {
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
                let partial_stdout = crate::util::truncate_head_tail(
                    &partial_stdout,
                    DEFAULT_LLM_OUTPUT_CAP / 2,
                    25,
                )
                .unwrap_or(partial_stdout);
                let partial_stderr = crate::util::truncate_head_tail(
                    &partial_stderr,
                    DEFAULT_LLM_OUTPUT_CAP / 2,
                    25,
                )
                .unwrap_or(partial_stderr);
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
                    super::super::background::registry::BgStream::Stderr,
                    mirror_session_id.as_deref(),
                );
                super::super::background::registry::publish(
                    super::super::background::registry::BackgroundShellSignal::Exited {
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
