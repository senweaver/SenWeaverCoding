// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Coordinator mode — restricted tool sets for coordination-only agents.
//!
//! In coordinator mode, the agent uses a limited set of tools focused on
//! planning, delegation, and communication rather than direct code changes.

use std::collections::HashSet;

/// Tools allowed in coordinator mode.
const COORDINATOR_ALLOWED_TOOLS: &[&str] = &[
    "delegate",
    "send_message",
    "team_create",
    "team_delete",
    "todo_write",
    "enter_plan_mode",
    "exit_plan_mode",
    "task_create",
    "task_get",
    "task_update",
    "task_list",
    "task_output",
    "task_stop",
    "file_read",
    "glob_search",
    "content_search",
    "memory_store",
    "memory_recall",
    "send_user_message",
    "sleep",
    "lsp",
];

/// Check if coordinator mode restricts a given tool.
pub fn is_coordinator_tool(tool_name: &str) -> bool {
    COORDINATOR_ALLOWED_TOOLS.contains(&tool_name)
}

/// Get the set of tools allowed in coordinator mode.
pub fn coordinator_tool_set() -> HashSet<String> {
    COORDINATOR_ALLOWED_TOOLS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Filter a tool registry to only coordinator-allowed tools.
pub fn filter_for_coordinator(tool_names: &[&str]) -> Vec<String> {
    tool_names
        .iter()
        .filter(|name| is_coordinator_tool(name))
        .map(|s| (*s).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegate_is_coordinator_tool() {
        assert!(is_coordinator_tool("delegate"));
    }

    #[test]
    fn shell_is_not_coordinator_tool() {
        assert!(!is_coordinator_tool("shell"));
    }

    #[test]
    fn coordinator_set_not_empty() {
        let set = coordinator_tool_set();
        assert!(!set.is_empty());
        assert!(set.contains("delegate"));
    }
}
