// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tool-surface extensions: global allow/deny lists, per-tool budgets.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolsExtras {

    #[serde(default)]
    pub global_deny: Vec<String>,

    #[serde(default)]
    pub force_read_only: Vec<String>,

    #[serde(default)]
    pub timeout_secs_overrides: std::collections::HashMap<String, u64>,

    #[serde(default = "default_max_tool_output_bytes")]
    pub max_tool_output_bytes: usize,
}

fn default_max_tool_output_bytes() -> usize {
    1_048_576
}

impl Default for ToolsExtras {
    fn default() -> Self {
        Self {
            global_deny: Vec::new(),
            force_read_only: Vec::new(),
            timeout_secs_overrides: std::collections::HashMap::new(),
            max_tool_output_bytes: default_max_tool_output_bytes(),
        }
    }
}

impl ToolsExtras {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (name, secs) in &self.timeout_secs_overrides {
            if *secs == 0 {
                errors.push(format!(
                    "tools.timeout_secs_overrides['{name}'] = 0 is not permitted (use a sensible positive value)"
                ));
            }
            if *secs > 3600 {
                errors.push(format!(
                    "tools.timeout_secs_overrides['{name}'] = {secs}s (> 1 hour) is almost certainly a misconfiguration"
                ));
            }
        }
        for name in &self.global_deny {
            if name.is_empty() {
                errors.push("tools.global_deny contains empty string".into());
            }
        }
        errors
    }

    pub fn is_denied(&self, tool_name: &str) -> bool {
        self.global_deny.iter().any(|d| d == tool_name)
    }

    pub fn is_read_only(&self, tool_name: &str) -> bool {
        self.force_read_only.iter().any(|d| d == tool_name)
    }

    pub fn timeout_for(&self, tool_name: &str) -> Option<u64> {
        self.timeout_secs_overrides.get(tool_name).copied()
    }
}
