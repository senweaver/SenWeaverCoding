// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Tool use summary service — mirrors claude-code-typescript-src`services/toolUseSummary/`.
// Tracks and summarizes tool usage per turn for context compaction
// and user display.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub turn: u32,
    pub duration_ms: u64,
    pub success: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsageStats {
    pub tool_name: String,
    pub call_count: u32,
    pub total_duration_ms: u64,
    pub success_count: u32,
    pub failure_count: u32,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

pub struct ToolUseSummaryService {
    invocations: Vec<ToolInvocation>,
}

impl ToolUseSummaryService {
    pub fn new() -> Self {
        Self {
            invocations: Vec::new(),
        }
    }

    pub fn record(&mut self, invocation: ToolInvocation) {
        self.invocations.push(invocation);
    }

    pub fn aggregate(&self) -> Vec<ToolUsageStats> {
        let mut map: HashMap<String, ToolUsageStats> = HashMap::new();
        for inv in &self.invocations {
            let entry = map
                .entry(inv.tool_name.clone())
                .or_insert_with(|| ToolUsageStats {
                    tool_name: inv.tool_name.clone(),
                    call_count: 0,
                    total_duration_ms: 0,
                    success_count: 0,
                    failure_count: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                });
            entry.call_count += 1;
            entry.total_duration_ms += inv.duration_ms;
            if inv.success {
                entry.success_count += 1;
            } else {
                entry.failure_count += 1;
            }
            entry.total_input_tokens += inv.input_tokens;
            entry.total_output_tokens += inv.output_tokens;
        }
        let mut stats: Vec<ToolUsageStats> = map.into_values().collect();
        stats.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        stats
    }

    pub fn turn_stats(&self, turn: u32) -> Vec<ToolUsageStats> {
        let filtered: Vec<&ToolInvocation> =
            self.invocations.iter().filter(|i| i.turn == turn).collect();
        let mut map: HashMap<String, ToolUsageStats> = HashMap::new();
        for inv in filtered {
            let entry = map
                .entry(inv.tool_name.clone())
                .or_insert_with(|| ToolUsageStats {
                    tool_name: inv.tool_name.clone(),
                    call_count: 0,
                    total_duration_ms: 0,
                    success_count: 0,
                    failure_count: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                });
            entry.call_count += 1;
            entry.total_duration_ms += inv.duration_ms;
            if inv.success {
                entry.success_count += 1;
            } else {
                entry.failure_count += 1;
            }
            entry.total_input_tokens += inv.input_tokens;
            entry.total_output_tokens += inv.output_tokens;
        }
        map.into_values().collect()
    }

    pub fn format_summary(&self) -> String {
        let stats = self.aggregate();
        if stats.is_empty() {
            return "No tools used.".to_string();
        }
        let lines: Vec<String> = stats
            .iter()
            .map(|s| {
                format!(
                    "{}: {} calls, {}ms total, {}/{} success/fail",
                    s.tool_name,
                    s.call_count,
                    s.total_duration_ms,
                    s.success_count,
                    s.failure_count
                )
            })
            .collect();
        lines.join("\n")
    }

    pub fn total_invocations(&self) -> usize {
        self.invocations.len()
    }

    pub fn clear(&mut self) {
        self.invocations.clear();
    }
}

impl Default for ToolUseSummaryService {
    fn default() -> Self {
        Self::new()
    }
}
