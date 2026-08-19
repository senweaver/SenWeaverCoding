// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::traits::{HookHandler, HookResult};

#[derive(
    Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum HookEvent {

    BeforeShellExecution,

    #[serde(rename = "beforeMCPExecution")]
    BeforeMcpExecution,

    #[serde(rename = "preToolUse")]
    PreToolUse,

    #[serde(rename = "postToolUse")]
    PostToolUse,

    BeforeReadFile,

    BeforeSubmitPrompt,

    AfterFileEdit,

    Stop,

    SessionStart,

    SessionEnd,

    SubagentStop,

    PreCompact,

    Notification,
}

impl HookEvent {

    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::BeforeShellExecution => "beforeShellExecution",
            HookEvent::BeforeMcpExecution => "beforeMCPExecution",
            HookEvent::PreToolUse => "preToolUse",
            HookEvent::PostToolUse => "postToolUse",
            HookEvent::BeforeReadFile => "beforeReadFile",
            HookEvent::BeforeSubmitPrompt => "beforeSubmitPrompt",
            HookEvent::AfterFileEdit => "afterFileEdit",
            HookEvent::Stop => "stop",
            HookEvent::SessionStart => "sessionStart",
            HookEvent::SessionEnd => "sessionEnd",
            HookEvent::SubagentStop => "subagentStop",
            HookEvent::PreCompact => "preCompact",
            HookEvent::Notification => "notification",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailMode {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCommand {

    pub command: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matchers: Option<HookMatchers>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_mode: Option<HookFailMode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookMatchers {

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_prefix: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {

    #[serde(default = "default_version")]
    pub version: u32,

    #[serde(default)]
    pub hooks: BTreeMap<HookEvent, Vec<HookCommand>>,
}

fn default_version() -> u32 {
    1
}

impl HooksConfig {

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let stripped = strip_jsonc_comments(text);
        serde_json::from_str(&stripped)
    }
}

fn strip_jsonc_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escape_next {
                escape_next = false;
            } else if c == '\\' {
                escape_next = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push('"');
            continue;
        }
        if c == '/' {
            match chars.peek() {
                Some('/') => {
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut prev_star = false;
                    for next in chars.by_ref() {
                        if prev_star && next == '/' {
                            break;
                        }
                        prev_star = next == '*';
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookDecision {
    Allow,
    Ask {
        user_message: Option<String>,
    },
    Deny {
        user_message: Option<String>,
        agent_message: Option<String>,
    },
}

impl HookDecision {
    pub fn is_deny(&self) -> bool {
        matches!(self, HookDecision::Deny { .. })
    }
    pub fn is_ask(&self) -> bool {
        matches!(self, HookDecision::Ask { .. })
    }

    pub fn merge(self, other: HookDecision) -> HookDecision {
        use HookDecision::*;
        match (self, other) {
            (deny @ Deny { .. }, _) => deny,
            (_, d @ Deny { .. }) => d,
            (Ask { user_message }, Allow) => Ask { user_message },
            (Allow, Ask { user_message }) => Ask { user_message },
            (Ask { .. }, Ask { user_message }) => Ask { user_message },
            (Allow, Allow) => Allow,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HookPayload {
    pub event: &'static str,

    pub tool_name: Option<String>,

    pub workspace_dir: String,

    #[serde(flatten)]
    pub extras: Value,
}

#[derive(Debug, Clone)]
struct HooksSource {
    origin: PathBuf,

    precedence: u8,
    config: HooksConfig,
}

pub struct ScriptHookRunner {
    sources: Vec<HooksSource>,
    workspace_dir: PathBuf,
    default_timeout: Duration,
    default_fail_mode: Option<HookFailMode>,

    enabled: bool,
}

impl Default for ScriptHookRunner {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            workspace_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            default_timeout: Duration::from_secs(15),
            default_fail_mode: None,
            enabled: true,
        }
    }
}

impl ScriptHookRunner {

    pub fn new(workspace_dir: PathBuf) -> Self {
        Self {
            workspace_dir,
            ..Default::default()
        }
    }

    pub fn load_default(workspace_dir: PathBuf, allow_workspace_hooks: bool) -> Self {
        let mut runner = Self::new(workspace_dir.clone());
        for (path, precedence) in default_lookup_paths(&workspace_dir) {
            let is_workspace_source = precedence == WORKSPACE_HOOKS_PRECEDENCE;
            if is_workspace_source && !allow_workspace_hooks {
                if path.is_file() {
                    tracing::warn!(
                        path = %path.display(),
                        "workspace hooks.json found but blocked by security policy; \
                         set `hooks.allow_workspace_hooks = true` in your config to run \
                         hooks defined inside the workspace (they execute arbitrary shell \
                         commands, so only enable this for workspaces you trust)"
                    );
                }
                continue;
            }
            if let Some(src) = load_one(&path, precedence) {
                runner.sources.push(src);
            }
        }

        runner.sources.sort_by_key(|s| s.precedence);
        runner
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_default_fail_mode(mut self, mode: Option<HookFailMode>) -> Self {
        self.default_fail_mode = mode;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn add_in_memory(&mut self, origin: PathBuf, precedence: u8, config: HooksConfig) {
        self.sources.push(HooksSource {
            origin,
            precedence,
            config,
        });
        self.sources.sort_by_key(|s| s.precedence);
    }

    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    pub fn source_paths(&self) -> Vec<PathBuf> {
        self.sources.iter().map(|s| s.origin.clone()).collect()
    }

    fn commands_for(&self, event: HookEvent, payload: &HookPayload) -> Vec<&HookCommand> {
        let mut out: Vec<&HookCommand> = Vec::new();
        for src in &self.sources {
            if let Some(cmds) = src.config.hooks.get(&event) {
                out.extend(
                    cmds.iter()
                        .filter(|c| matcher_matches(c.matchers.as_ref(), payload)),
                );
            }
        }
        out
    }

    pub async fn dispatch(&self, event: HookEvent, payload: HookPayload) -> HookDecision {
        if !self.enabled {
            return HookDecision::Allow;
        }
        let cmds = self.commands_for(event, &payload);
        if cmds.is_empty() {
            return HookDecision::Allow;
        }
        let payload_json = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{}".to_vec());
        let mut overall = HookDecision::Allow;
        for cmd in cmds {
            let decision = self.run_single(cmd, &payload_json).await;
            let was_deny = decision.is_deny();
            overall = overall.merge(decision);
            if was_deny {
                break;
            }
        }
        overall
    }

    async fn run_single(&self, cmd: &HookCommand, payload: &[u8]) -> HookDecision {
        if let Err(reason) = crate::security::detect::ensure_sandbox_available() {
            tracing::error!(
                command = cmd.command,
                error = %reason,
                "refusing to run hooks.json script: explicitly configured sandbox backend is unavailable"
            );
            return HookDecision::Deny {
                user_message: Some(reason.clone()),
                agent_message: Some(reason),
            };
        }
        let effective_fail_mode = cmd.fail_mode.or(self.default_fail_mode);
        let fail_open_decision = |reason_user: String, reason_agent: String| -> HookDecision {
            match effective_fail_mode {
                Some(HookFailMode::Deny) => HookDecision::Deny {
                    user_message: Some(reason_user),
                    agent_message: Some(reason_agent),
                },
                _ => HookDecision::Allow,
            }
        };
        let timeout = cmd
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(self.default_timeout);
        let mut command = build_shell_command(&cmd.command);
        command
            .current_dir(&self.workspace_dir)
            .env("SEN_HOOK_EVENT", "1")
            .env("CURSOR_HOOK_EVENT", "1")
            .env("CURSOR_WORKSPACE_DIR", self.workspace_dir.as_os_str())
            .env("SEN_WORKSPACE_DIR", self.workspace_dir.as_os_str())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let spawn_result = command.spawn();
        let mut child = match spawn_result {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    command = cmd.command,
                    error = %e,
                    fail_mode = ?effective_fail_mode,
                    "hooks.json script failed to spawn"
                );
                return fail_open_decision(
                    format!("hook `{}` failed to spawn: {e}", cmd.command),
                    "hook spawn failed".to_string(),
                );
            }
        };

        let stdin = child.stdin.take();
        let payload_owned = payload.to_vec();
        let stdin_write_timed_out =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stdin_flag = std::sync::Arc::clone(&stdin_write_timed_out);
        let stdin_write_deadline = Duration::from_secs(2).min(timeout);
        let output_future = async move {
            if let Some(mut stdin) = stdin {
                use tokio::io::AsyncWriteExt;
                let write = async {
                    let _ = stdin.write_all(&payload_owned).await;
                    let _ = stdin.shutdown().await;
                };
                if tokio::time::timeout(stdin_write_deadline, write)
                    .await
                    .is_err()
                {
                    stdin_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            child.wait_with_output().await
        };
        let output = match tokio::time::timeout(timeout, output_future).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                tracing::warn!(command = cmd.command, error = %e, fail_mode = ?effective_fail_mode, "hook script wait failed");
                return fail_open_decision(
                    format!("hook `{}` wait failed: {e}", cmd.command),
                    "hook wait failed".to_string(),
                );
            }
            Err(_) => {
                if stdin_write_timed_out.load(std::sync::atomic::Ordering::Relaxed) {
                    tracing::warn!(
                        command = cmd.command,
                        timeout_ms = timeout.as_millis() as u64,
                        fail_mode = ?effective_fail_mode,
                        "hook script did not consume its stdin payload and then timed out"
                    );
                    return fail_open_decision(
                        format!(
                            "hook `{}` did not read stdin and timed out after {}ms",
                            cmd.command,
                            timeout.as_millis() as u64
                        ),
                        "hook stdin timeout".to_string(),
                    );
                }
                tracing::warn!(
                    command = cmd.command,
                    timeout_ms = timeout.as_millis() as u64,
                    fail_mode = ?effective_fail_mode,
                    "hook script timed out"
                );
                if effective_fail_mode == Some(HookFailMode::Allow) {
                    return HookDecision::Allow;
                }
                return HookDecision::Deny {
                    user_message: Some(format!(
                        "hook script `{}` timed out after {}ms",
                        cmd.command,
                        timeout.as_millis() as u64
                    )),
                    agent_message: Some("hook timeout".to_string()),
                };
            }
        };

        if stdin_write_timed_out.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::warn!(
                command = cmd.command,
                "hook script did not consume its stdin payload within {}ms; \
                 stdin was closed early",
                stdin_write_deadline.as_millis() as u64
            );
        }

        if !output.status.success() {

            let stderr = crate::util::decode_subprocess_bytes(&output.stderr)
                .trim()
                .to_string();
            return HookDecision::Deny {
                user_message: Some(format!("hook `{}` exited non-zero", cmd.command)),
                agent_message: if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                },
            };
        }

        let stdout = crate::util::decode_subprocess_bytes(&output.stdout);
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return HookDecision::Allow;
        }
        match serde_json::from_str::<HookScriptResponse>(trimmed) {
            Ok(resp) => resp.into_decision(),
            Err(_) => {

                tracing::debug!(
                    command = cmd.command,
                    stdout = %crate::agent::profile::pii_sanitize::scrub_credentials(trimmed),
                    "hook script returned non-JSON output; allowing"
                );
                HookDecision::Allow
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct HookScriptResponse {
    #[serde(default)]
    permission: Option<String>,
    #[serde(default, alias = "userMessage")]
    user_message: Option<String>,
    #[serde(default, alias = "agentMessage")]
    agent_message: Option<String>,
}

impl HookScriptResponse {
    fn into_decision(self) -> HookDecision {
        match self.permission.as_deref().unwrap_or("allow") {
            "deny" | "block" => HookDecision::Deny {
                user_message: self.user_message,
                agent_message: self.agent_message,
            },
            "ask" | "prompt" => HookDecision::Ask {
                user_message: self.user_message,
            },
            _ => HookDecision::Allow,
        }
    }
}

fn matcher_matches(m: Option<&HookMatchers>, payload: &HookPayload) -> bool {
    let Some(m) = m else {
        return true;
    };
    if let Some(name_pat) = &m.tool_name {
        let actual = payload.tool_name.as_deref().unwrap_or("");
        if !glob_match(name_pat, actual) {
            return false;
        }
    }
    if let Some(prefix) = &m.command_prefix {
        let actual = payload
            .extras
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !actual.contains(prefix) {
            return false;
        }
    }
    true
}

fn glob_match(pattern: &str, text: &str) -> bool {

    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text == pattern;
    }
    let mut cursor = 0;
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if idx == 0 {
            if !text[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
        } else if idx == parts.len() - 1 {
            return text[cursor..].ends_with(part);
        } else {
            match text[cursor..].find(part) {
                Some(pos) => cursor += pos + part.len(),
                None => return false,
            }
        }
    }
    true
}

fn truncate_for_payload(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str("…[truncated]");
    out
}

const PRE_TOOL_PAYLOAD_STRING_MAX: usize = 4096;

fn truncate_json_strings(value: &mut Value, max: usize) {
    match value {
        Value::String(s) => {
            if s.len() > max {
                *s = truncate_for_payload(s, max);
            }
        }
        Value::Array(items) => {
            for item in items {
                truncate_json_strings(item, max);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                truncate_json_strings(v, max);
            }
        }
        _ => {}
    }
}

fn sanitize_pre_tool_extras(args: &Value) -> Value {
    let mut extras = args.clone();
    truncate_json_strings(&mut extras, PRE_TOOL_PAYLOAD_STRING_MAX);
    if let Some(obj) = extras.as_object_mut() {
        obj.remove("event");
        obj.remove("tool_name");
        obj.remove("workspace_dir");
    }
    extras
}

fn build_shell_command(line: &str) -> tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut c = crate::util::hidden_sync_command("cmd.exe");
        c.arg("/S").arg("/C").raw_arg(line);
        tokio::process::Command::from(c)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut c = crate::util::hidden_async_command("sh");
        c.arg("-c").arg(line);
        c
    }
}

const WORKSPACE_HOOKS_PRECEDENCE: u8 = 1;

fn default_lookup_paths(workspace_dir: &Path) -> Vec<(PathBuf, u8)> {
    let mut out = Vec::new();
    if let Some(home) = home_dir() {
        out.push((home.join(".cursor").join("hooks.json"), 0));
        out.push((home.join(".sen").join("hooks.json"), 0));
    }
    out.push((
        workspace_dir.join(".cursor").join("hooks.json"),
        WORKSPACE_HOOKS_PRECEDENCE,
    ));
    out.push((
        workspace_dir.join(".sen").join("hooks.json"),
        WORKSPACE_HOOKS_PRECEDENCE,
    ));
    out
}

fn load_one(path: &Path, precedence: u8) -> Option<HooksSource> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return None,
    };
    match HooksConfig::from_json(&raw) {
        Ok(config) => Some(HooksSource {
            origin: path.to_path_buf(),
            precedence,
            config,
        }),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to parse hooks.json; skipping");
            None
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
}

static SHELL_CAPABLE_TOOLS: std::sync::LazyLock<
    parking_lot::RwLock<std::collections::HashSet<String>>,
> = std::sync::LazyLock::new(|| parking_lot::RwLock::new(std::collections::HashSet::new()));

pub fn register_shell_capable_tool(name: &str) {
    SHELL_CAPABLE_TOOLS.write().insert(name.to_string());
}

fn is_shell_capable_tool(tool: &str) -> bool {
    if SHELL_CAPABLE_TOOLS.read().contains(tool) {
        return true;
    }
    matches!(
        tool.to_ascii_lowercase().as_str(),
        "shell_exec" | "shell" | "bash" | "terminal_run" | "process_exec" | "powershell"
    )
}

fn is_mcp_tool(tool: &str) -> bool {
    let lower = tool.to_ascii_lowercase();
    if lower.starts_with("mcp.") || lower.starts_with("mcp_") {
        return true;
    }
    if let Some((head, tail)) = tool.split_once("__") {
        if !head.is_empty() && !tail.is_empty() {
            return match crate::tools::mcp::client::global_registry() {
                Some(registry) => registry.has_tool(tool),
                None => true,
            };
        }
    }
    false
}

pub fn events_for_tool_pre(tool: &str) -> Vec<HookEvent> {
    let lower = tool.to_ascii_lowercase();
    let mut events = Vec::with_capacity(2);
    if is_mcp_tool(tool) {
        events.push(HookEvent::BeforeMcpExecution);
    } else if is_shell_capable_tool(tool) {
        events.push(HookEvent::BeforeShellExecution);
    } else {
        match lower.as_str() {
            "file_read" | "read_file" | "fs_read" => {
                events.push(HookEvent::BeforeReadFile);
            }
            _ => {}
        }
    }
    events.push(HookEvent::PreToolUse);
    events
}

pub fn event_for_tool_post(tool: &str) -> Option<HookEvent> {
    let lower = tool.to_ascii_lowercase();
    match lower.as_str() {
        "file_write" | "write_file" | "fs_write" | "apply_diff" | "inline_edit"
        | "edit_file" | "search_replace" | "multi_edit" | "file_edit" | "patch_apply"
        | "diff_apply" | "glob_edit" | "notebook_edit" => Some(HookEvent::AfterFileEdit),
        _ => None,
    }
}

#[async_trait]
impl HookHandler for ScriptHookRunner {
    fn name(&self) -> &str {
        "hooks_json_script_runner"
    }

    fn priority(&self) -> i32 {

        -100
    }

    async fn before_tool_call(
        &self,
        name: String,
        args: Value,
    ) -> HookResult<(String, Value)> {
        let extras = sanitize_pre_tool_extras(&args);
        let mut overall = HookDecision::Allow;
        for event in events_for_tool_pre(&name) {
            let payload = HookPayload {
                event: event.as_str(),
                tool_name: Some(name.clone()),
                workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
                extras: extras.clone(),
            };
            let decision = self.dispatch(event, payload).await;
            let was_deny = decision.is_deny();
            overall = overall.merge(decision);
            if was_deny {
                break;
            }
        }
        match overall {
            HookDecision::Allow => HookResult::Continue((name, args)),
            HookDecision::Ask { user_message } => {
                HookResult::RequireApproval((name, args), user_message)
            }
            HookDecision::Deny {
                user_message,
                agent_message,
            } => {
                let msg = agent_message
                    .or(user_message)
                    .unwrap_or_else(|| "denied by hooks.json".into());
                HookResult::Cancel(msg)
            }
        }
    }

    async fn on_after_tool_call(
        &self,
        tool: &str,
        result: &crate::tools::traits::ToolResult,
        _duration: Duration,
    ) {
        let mut events: Vec<HookEvent> = vec![HookEvent::PostToolUse];
        if let Some(specific) = event_for_tool_post(tool) {
            events.push(specific);
        }
        for event in events {
            let payload = HookPayload {
                event: event.as_str(),
                tool_name: Some(tool.to_string()),
                workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
                extras: json!({
                    "success": result.success,
                    "output": truncate_for_payload(&result.output, 4096),
                    "error": result.error.clone(),
                }),
            };
            let _ = self.dispatch(event, payload).await;
        }
    }

    async fn before_prompt_build(&self, prompt: String) -> HookResult<String> {
        let payload = HookPayload {
            event: HookEvent::BeforeSubmitPrompt.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({ "prompt": prompt }),
        };
        match self.dispatch(HookEvent::BeforeSubmitPrompt, payload).await {
            HookDecision::Allow => HookResult::Continue(prompt),
            HookDecision::Ask { user_message } => HookResult::Cancel(format!(
                "hooks.json requested user confirmation: {}",
                user_message.unwrap_or_else(|| "manual approval required".into())
            )),
            HookDecision::Deny {
                user_message,
                agent_message,
            } => HookResult::Cancel(
                agent_message
                    .or(user_message)
                    .unwrap_or_else(|| "denied by hooks.json".into()),
            ),
        }
    }

    async fn on_session_start(&self, session_id: &str, channel: &str) {
        let payload = HookPayload {
            event: HookEvent::SessionStart.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({
                "session_id": session_id,
                "channel": channel,
            }),
        };
        let _ = self.dispatch(HookEvent::SessionStart, payload).await;
    }

    async fn on_session_end(&self, session_id: &str, channel: &str) {
        let payload = HookPayload {
            event: HookEvent::SessionEnd.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({
                "session_id": session_id,
                "channel": channel,
            }),
        };
        let _ = self.dispatch(HookEvent::SessionEnd, payload).await;
    }

    async fn on_turn_end(&self, channel: &str, final_text: &str, tools_used: &[String]) {
        let payload = HookPayload {
            event: HookEvent::Stop.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({
                "channel": channel,
                "response_text": truncate_for_payload(final_text, 8192),
                "tools_used": tools_used,
            }),
        };
        let _ = self.dispatch(HookEvent::Stop, payload).await;
    }

    async fn on_subagent_stop(&self, worker_id: &str, status: &str, summary: &str) {
        let payload = HookPayload {
            event: HookEvent::SubagentStop.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({
                "worker_id": worker_id,
                "status": status,
                "summary": truncate_for_payload(summary, 4096),
            }),
        };
        let _ = self.dispatch(HookEvent::SubagentStop, payload).await;
    }

    async fn on_pre_compact(&self, trigger: &str, estimated_tokens: usize) {
        let payload = HookPayload {
            event: HookEvent::PreCompact.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({
                "trigger": trigger,
                "estimated_tokens": estimated_tokens,
            }),
        };
        let _ = self.dispatch(HookEvent::PreCompact, payload).await;
    }

    async fn on_notification(&self, kind: &str, message: &str) {
        let payload = HookPayload {
            event: HookEvent::Notification.as_str(),
            tool_name: None,
            workspace_dir: self.workspace_dir.to_string_lossy().into_owned(),
            extras: json!({
                "kind": kind,
                "message": truncate_for_payload(message, 4096),
            }),
        };
        let _ = self.dispatch(HookEvent::Notification, payload).await;
    }
}
