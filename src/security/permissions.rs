// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {

    Auto,

    Ask,

    Plan,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Ask
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerPermissionMode {

    Default,

    AcceptEdits,

    Plan,

    Bypass,

    AskEveryTime,
}

impl ComposerPermissionMode {

    pub fn from_wire(s: &str) -> Self {
        match s {
            "acceptEdits" => Self::AcceptEdits,
            "plan" => Self::Plan,

            "bypassPermissions" | "dontAsk" => Self::Bypass,
            "askEveryTime" => Self::AskEveryTime,
            _ => Self::Default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {

    Auto,

    Ask,

    Deny,
}

const EDIT_TOOLS: &[&str] = &[
    "file_write",
    "file_edit",
    "multi_edit",
    "glob_edit",
    "notebook_edit",
    "patch_apply",
    "diff_apply",
    "lsp_rename",
    "restore_file",
    "copy_path",
    "move_path",
    "delete_path",
    "create_directory",
    "code_xfile_refactor",
    "lsp_format",
];

const SYSTEM_TOOLS: &[&str] = &[
    "shell",
    "powershell",
    "cron_add",
    "cron_list",
    "cron_remove",
    "cron_update",
    "cron_run",

];

const BROWSER_FAMILY_TOOLS: &[&str] = &[
    "browser",
    "browser_open",
    "browser_delegate",
    "text_browser",
];

fn is_edit_tool(name: &str) -> bool {
    EDIT_TOOLS.contains(&name)
}

fn is_system_tool(name: &str) -> bool {
    SYSTEM_TOOLS.contains(&name)
}

pub fn is_browser_tool(name: &str) -> bool {
    BROWSER_FAMILY_TOOLS.contains(&name)
}

pub fn is_shell_tool(name: &str) -> bool {
    matches!(name, "shell" | "powershell")
}

pub fn is_mcp_tool_name(name: &str) -> bool {
    if name.starts_with("mcp_") {
        return true;
    }
    if let Some((head, _rest)) = name.split_once("__") {
        return !head.is_empty();
    }
    false
}

pub fn gate_decision<S: ::std::hash::BuildHasher>(
    mode: ComposerPermissionMode,
    tool_name: &str,
    auto_approve: &std::collections::HashSet<String, S>,
    protect_browser: bool,
    protect_mcp: bool,
) -> GateDecision {

    if is_interactive_question_tool(tool_name) {
        return GateDecision::Ask;
    }

    let read_only = is_read_only_tool(tool_name);
    let is_edit = is_edit_tool(tool_name);
    let is_system = is_system_tool(tool_name);
    let browser_protected = protect_browser && is_browser_tool(tool_name);
    let mcp_protected = protect_mcp && is_mcp_tool_name(tool_name);

    if matches!(mode, ComposerPermissionMode::Bypass) {
        return GateDecision::Auto;
    }

    if matches!(mode, ComposerPermissionMode::AskEveryTime) {
        if read_only {
            return GateDecision::Auto;
        }
        return GateDecision::Ask;
    }

    if matches!(mode, ComposerPermissionMode::Plan) {
        if read_only || is_plan_mode_allowed_tool(tool_name) {
            return GateDecision::Auto;
        }
        return GateDecision::Deny;
    }

    if (browser_protected || mcp_protected) && !read_only {
        return GateDecision::Ask;
    }

    if auto_approve.contains("*") || auto_approve.contains(tool_name) {
        return GateDecision::Auto;
    }

    if read_only {
        return GateDecision::Auto;
    }

    match mode {
        ComposerPermissionMode::AcceptEdits => {
            if is_edit {
                GateDecision::Auto
            } else if is_system {
                GateDecision::Ask
            } else {
                GateDecision::Auto
            }
        }
        ComposerPermissionMode::Default => GateDecision::Ask,
        ComposerPermissionMode::Bypass
        | ComposerPermissionMode::AskEveryTime
        | ComposerPermissionMode::Plan => GateDecision::Ask,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub tool_name: String,
    pub action: PermissionAction,
    pub condition: Option<PermissionCondition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCondition {
    PathPrefix(String),
    CommandPattern(String),
    DomainPattern(String),
}

const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -r /",
    "sudo rm",
    "mkfs",
    "dd if=",
    "> /dev/",
    "chmod 777",
    "curl | sh",
    "curl | bash",
    "wget | sh",
    ":(){ :|:& };:",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "kill -9",
    "pkill -9",
    "DROP TABLE",
    "DROP DATABASE",
    "TRUNCATE TABLE",
    "DELETE FROM",
    "git push --force",
    "git reset --hard",
    "npm publish",
    "cargo publish",
];

const READ_ONLY_TOOLS: &[&str] = &[
    "file_read",
    "glob_search",
    "content_search",
    "dir_list",
    "present_files",
    "view_image",
    "image_info",
    "screenshot",
    "memory_recall",
    "memory_export",
    "calculator",
    "weather",
    "web_search",
    "web_fetch",
    "mcp_resources_list",
    "mcp_resources_read",
    "lsp",
    "todo_write",
    "enter_plan_mode",
    "exit_plan_mode",
    "task_list",
    "task_get",
    "task_output",
    "structured_output",
];

pub fn is_read_only_tool(name: &str) -> bool {
    READ_ONLY_TOOLS.contains(&name)
}

pub const PLAN_MODE_ALLOWED_TOOLS: &[&str] = &[

    "file_read",
    "glob_search",
    "content_search",
    "dir_list",
    "present_files",
    "view_image",
    "image_info",
    "screenshot",
    "memory_recall",
    "memory_export",
    "calculator",
    "weather",
    "web_search",
    "web_fetch",
    "mcp_resources_list",
    "mcp_resources_read",
    "lsp",
    "task_list",
    "task_get",
    "task_output",
    "structured_output",

    "code_outline",
    "code_graph_query",
    "tool_search",
    "pdf_read",
    "multi_search",
    "tavily_search",
    "exa_search",
    "youtube_search",
    "github_search",
    "github_advanced_search",
    "workspace_deep_search",
    "reddit_search",
    "image_search",
    "discord_search",
    "cron_list",
    "cron_runs",
    "web_search_tool",

    "enter_plan_mode",
    "exit_plan_mode",
    "update_plan",

    "ask_question",
    "ask_user",

    "read_skill",
    "cloud_patterns",
    "send_user_message",
    "now",
];

pub fn is_plan_mode_allowed_tool(name: &str) -> bool {
    PLAN_MODE_ALLOWED_TOOLS.contains(&name)
}

pub fn plan_mode_allowed_tools() -> Vec<&'static str> {
    PLAN_MODE_ALLOWED_TOOLS.to_vec()
}

pub const CURATOR_MODE_ALLOWED_TOOLS: &[&str] = &[
    "file_read",
    "glob_search",
    "content_search",
    "dir_list",
    "present_files",
    "view_image",
    "image_info",
    "screenshot",
    "memory_recall",
    "memory_export",
    "calculator",
    "weather",
    "web_search",
    "web_search_tool",
    "web_fetch",
    "multi_search",
    "tavily_search",
    "exa_search",
    "youtube_search",
    "github_search",
    "github_advanced_search",
    "reddit_search",
    "image_search",
    "discord_search",
    "workspace_deep_search",
    "code_outline",
    "code_graph_query",
    "tool_search",
    "lsp",
    "pdf_read",
    "mcp_resources_list",
    "mcp_resources_read",
    "task_list",
    "task_get",
    "task_output",
    "structured_output",
    "todo_write",
    "ask_question",
    "ask_user",
    "read_skill",
    "cloud_patterns",
    "send_user_message",
    "now",
    "file_write",
    "file_edit",
    "enter_curator_mode",
    "exit_curator_mode",
    "curator_collect",
    "curator_deep_collect",
    "curator_git_reference",
    "curator_local_reference",
    "curator_template_list",
    "curator_template_apply",
    "multi_persona_review",
    "scenario_matrix",
    "security_audit",
];

pub fn is_curator_mode_allowed_tool(name: &str) -> bool {
    CURATOR_MODE_ALLOWED_TOOLS.contains(&name)
}

pub fn curator_mode_allowed_tools() -> Vec<&'static str> {
    CURATOR_MODE_ALLOWED_TOOLS.to_vec()
}

pub fn is_curator_write_path_allowed(path: &std::path::Path) -> bool {
    let comps: Vec<_> = path.components().collect();
    let target_root = std::ffi::OsStr::new(".senweavercoding");
    let target_dir = std::ffi::OsStr::new("curators");
    for window in comps.windows(2) {
        if let [std::path::Component::Normal(a), std::path::Component::Normal(b)] = window {
            if a.eq_ignore_ascii_case(target_root) && b.eq_ignore_ascii_case(target_dir) {
                return true;
            }
        }
    }
    false
}

pub fn read_only_tool_names() -> Vec<&'static str> {
    READ_ONLY_TOOLS.to_vec()
}

pub fn is_interactive_question_tool(name: &str) -> bool {
    matches!(name, "ask_question" | "ask_user" | "AskQuestion")
}

pub fn is_dangerous_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    DANGEROUS_PATTERNS
        .iter()
        .any(|p| lower.contains(&p.to_lowercase()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,
    Moderate,
    Dangerous,
}

pub fn classify_command_risk(command: &str) -> RiskLevel {
    if is_dangerous_command(command) {
        RiskLevel::Dangerous
    } else if command.contains("sudo") || command.contains("rm ") || command.contains("mv ") {
        RiskLevel::Moderate
    } else {
        RiskLevel::Safe
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolActivationDecision {
    Yes,
    Always,
    No,
}

#[async_trait]
pub trait ToolActivationGate: Send + Sync {
    async fn request_tool_activation(
        &self,
        workspace_key: &str,
        tool_name: &str,
    ) -> anyhow::Result<ToolActivationDecision>;
}

pub type ToolActivationGateHandle = Arc<dyn ToolActivationGate>;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct PermissionsConfig {
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
}
pub struct AutoAllowGate;

#[async_trait]
impl ToolActivationGate for AutoAllowGate {
    async fn request_tool_activation(
        &self,
        _workspace_key: &str,
        _tool_name: &str,
    ) -> anyhow::Result<ToolActivationDecision> {
        Ok(ToolActivationDecision::Yes)
    }
}

pub struct CliStdinGate;

#[async_trait]
impl ToolActivationGate for CliStdinGate {
    async fn request_tool_activation(
        &self,
        _workspace_key: &str,
        tool_name: &str,
    ) -> anyhow::Result<ToolActivationDecision> {
        let tool_name = tool_name.to_string();
        {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                tracing::warn!(
                    target: "security.permissions",
                    tool = %tool_name,
                    "tool activation requested without an interactive terminal; defaulting to deny"
                );
                return Ok(ToolActivationDecision::No);
            }
        }
        let decision = tokio::task::spawn_blocking(move || {
            use std::io::{self, BufRead, Write};
            eprintln!();
            eprintln!("⚠️  Agent requested a high-risk tool: {tool_name}");
            eprint!(
                "    是否允许助手使用工具 {tool_name}？(y)es / (a)lways / (n)o: "
            );
            let _ = io::stderr().flush();
            let stdin = io::stdin();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).is_err() {
                return ToolActivationDecision::No;
            }
            match line.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => ToolActivationDecision::Yes,
                "a" | "always" => ToolActivationDecision::Always,
                _ => ToolActivationDecision::No,
            }
        })
        .await
        .unwrap_or(ToolActivationDecision::No);
        Ok(decision)
    }
}

pub struct SessionActivationGate {
    sink: crate::session::SessionEventSink,
    bus: tokio::sync::broadcast::Sender<crate::session::SessionEvent>,
    timeout_ms: u64,
}

impl SessionActivationGate {
    pub fn new(
        sink: crate::session::SessionEventSink,
        bus: tokio::sync::broadcast::Sender<crate::session::SessionEvent>,
        timeout_ms: u64,
    ) -> Self {
        Self {
            sink,
            bus,
            timeout_ms,
        }
    }
}

#[async_trait]
impl ToolActivationGate for SessionActivationGate {
    async fn request_tool_activation(
        &self,
        workspace_key: &str,
        tool_name: &str,
    ) -> anyhow::Result<ToolActivationDecision> {
        use crate::session::SessionEventKind;
        let request_id = format!(
            "tool_activate_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let arguments = serde_json::json!({
            "kind": "tool_activation",
            "tool_name": tool_name,
            "workspace_key": workspace_key,
        });
        self.sink.emit_kind(SessionEventKind::ApprovalRequested {
            id: request_id.clone(),
            tool_name: format!("tool_search/activate:{tool_name}"),
            arguments,
            issued_at: chrono::Utc::now(),
        });
        crate::approval::register_pending_gateway_approval(request_id.clone());

        let mut rx = self.bus.subscribe();
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(self.timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Ok(ToolActivationDecision::No);
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(evt)) => {
                    if let SessionEventKind::ApprovalResponded { id, decision, .. } = &evt.kind {
                        if id == &request_id {
                            return Ok(match decision.to_ascii_lowercase().as_str() {
                                "yes" | "y" => ToolActivationDecision::Yes,
                                "always" | "a" => ToolActivationDecision::Always,
                                _ => ToolActivationDecision::No,
                            });
                        }
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => {
                    return Ok(ToolActivationDecision::No);
                }
            }
        }
    }
}
