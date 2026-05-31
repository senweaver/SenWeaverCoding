// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

use crate::security::permissions::is_mcp_tool_name;
use crate::tools::Tool;

fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.find('*') {
        None => pattern == name,
        Some(star) => {
            let prefix = &pattern[..star];
            let suffix = &pattern[star + 1..];
            name.starts_with(prefix)
                && name.ends_with(suffix)
                && name.len() >= prefix.len() + suffix.len()
        }
    }
}

pub(crate) fn filter_tool_specs_for_turn(
    tool_specs: Vec<crate::tools::ToolSpec>,
    groups: &[crate::config::schema::ToolFilterGroup],
    user_message: &str,
) -> Vec<crate::tools::ToolSpec> {
    use crate::config::schema::ToolFilterGroupMode;

    if groups.is_empty() {
        return tool_specs;
    }

    let msg_lower = user_message.to_ascii_lowercase();

    tool_specs
        .into_iter()
        .filter(|spec| {
            if !is_mcp_tool_name(&spec.name) {
                return true;
            }

            groups.iter().any(|group| {
                let pattern_matches = group.tools.iter().any(|pat| glob_match(pat, &spec.name));
                if !pattern_matches {
                    return false;
                }
                match group.mode {
                    ToolFilterGroupMode::Always => true,
                    ToolFilterGroupMode::Dynamic => group
                        .keywords
                        .iter()
                        .any(|kw| msg_lower.contains(&kw.to_ascii_lowercase())),
                }
            })
        })
        .collect()
}

pub(crate) fn is_plan_mode_allowed(tool_name: &str) -> bool {
    crate::security::permissions::is_plan_mode_allowed_tool(tool_name)
}

pub(crate) fn compute_excluded_mcp_tools(
    tools_registry: &[Box<dyn Tool>],
    groups: &[crate::config::schema::ToolFilterGroup],
    user_message: &str,
) -> Vec<String> {
    if groups.is_empty() {
        return Vec::new();
    }
    let filtered_specs = filter_tool_specs_for_turn(
        tools_registry.iter().map(|t| t.spec()).collect(),
        groups,
        user_message,
    );
    let included: HashSet<&str> = filtered_specs.iter().map(|s| s.name.as_str()).collect();
    tools_registry
        .iter()
        .filter(|t| is_mcp_tool_name(t.name()) && !included.contains(t.name()))
        .map(|t| t.name().to_string())
        .collect()
}
