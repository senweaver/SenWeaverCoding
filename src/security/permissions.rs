// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Permission system — mirrors cc-typescript-src's `utils/permissions/`.
//!
//! Provides permission modes (auto/ask/plan), rule-based tool filtering,
//! dangerous command classification, and deny lists.

use serde::{Deserialize, Serialize};

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
    "cron_create",
    "cron_update",
    "cron_delete",
    "cron_run",
    "cron_pause",
    "cron_resume",

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

pub fn is_mcp_tool_name(name: &str) -> bool {
    if name.starts_with("mcp_") {
        return true;
    }
    if let Some((head, _rest)) = name.split_once("__") {
        return !head.is_empty();
    }
    false
}

pub fn gate_decision(
    mode: ComposerPermissionMode,
    tool_name: &str,
    auto_approve: &std::collections::HashSet<String>,
    protect_browser: bool,
    protect_mcp: bool,
) -> GateDecision {

    if is_interactive_question_tool(tool_name) {
        return GateDecision::Ask;
    }

    let read_only = is_read_only_tool(tool_name);
    let browser_protected = protect_browser && is_browser_tool(tool_name);
    let mcp_protected = protect_mcp && is_mcp_tool_name(tool_name);

    if (browser_protected || mcp_protected) && !read_only {
        return GateDecision::Ask;
    }

    let allowlist_eligible = !matches!(mode, ComposerPermissionMode::AskEveryTime);
    if allowlist_eligible
        && (auto_approve.contains("*") || auto_approve.contains(tool_name))
    {
        return GateDecision::Auto;
    }

    if read_only {
        return GateDecision::Auto;
    }
    let is_edit = is_edit_tool(tool_name);
    let is_system = is_system_tool(tool_name);
    match mode {
        ComposerPermissionMode::Bypass => GateDecision::Auto,
        ComposerPermissionMode::Plan => {

            if is_edit || is_system {
                GateDecision::Deny
            } else if is_plan_mode_allowed_tool(tool_name) {
                GateDecision::Auto
            } else {

                GateDecision::Deny
            }
        }
        ComposerPermissionMode::AcceptEdits => {
            if is_edit {
                GateDecision::Auto
            } else if is_system {
                GateDecision::Ask
            } else {

                GateDecision::Auto
            }
        }
        ComposerPermissionMode::AskEveryTime => GateDecision::Ask,
        ComposerPermissionMode::Default => GateDecision::Ask,
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

    "grep",
    "code_search",
    "code_outline",
    "code_graph_query",
    "tool_search",
    "lsp_symbols",
    "pdf_read",
    "multi_search",
    "tavily_search",
    "exa_search",
    "youtube_search",
    "github_search",
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
    "brief",
    "now",
];

pub fn is_plan_mode_allowed_tool(name: &str) -> bool {
    PLAN_MODE_ALLOWED_TOOLS.contains(&name)
}

pub fn plan_mode_allowed_tools() -> Vec<&'static str> {
    PLAN_MODE_ALLOWED_TOOLS.to_vec()
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
