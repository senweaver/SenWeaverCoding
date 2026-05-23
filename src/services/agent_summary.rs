// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryGranularity {
    Brief,
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub session_id: String,
    pub granularity: SummaryGranularity,
    pub files_changed: Vec<String>,
    pub tools_used: Vec<String>,
    pub tasks_completed: u32,
    pub tasks_pending: u32,
    pub key_decisions: Vec<String>,
    pub summary_text: String,
    pub duration_ms: u64,
}

pub struct AgentSummaryService;

impl AgentSummaryService {

    pub fn summarize(
        session_id: &str,
        tools_used: &[ToolUsageRecord],
        files_changed: &[String],
        granularity: SummaryGranularity,
    ) -> AgentSummary {
        let key_decisions: Vec<String> = tools_used
            .iter()
            .filter(|t| t.is_write_operation)
            .map(|t| format!("{}: {}", t.tool_name, t.description))
            .collect();

        let summary_text = match granularity {
            SummaryGranularity::Brief => format!(
                "Used {} tools, changed {} files.",
                tools_used.len(),
                files_changed.len()
            ),
            SummaryGranularity::Standard => {
                let tool_names: Vec<&str> =
                    tools_used.iter().map(|t| t.tool_name.as_str()).collect();
                let unique_tools: std::collections::HashSet<&str> =
                    tool_names.into_iter().collect();
                format!(
                    "Used {} unique tools ({} total invocations), changed {} files. Key actions: {}",
                    unique_tools.len(),
                    tools_used.len(),
                    files_changed.len(),
                    key_decisions.join("; ")
                )
            }
            SummaryGranularity::Detailed => {
                let mut parts = Vec::new();
                parts.push(format!("Files changed ({}):", files_changed.len()));
                for f in files_changed {
                    parts.push(format!("  - {f}"));
                }
                parts.push(format!("\nTool invocations ({}):", tools_used.len()));
                for t in tools_used {
                    parts.push(format!(
                        "  - {} [{}ms]: {}",
                        t.tool_name, t.duration_ms, t.description
                    ));
                }
                if !key_decisions.is_empty() {
                    parts.push(format!("\nKey decisions ({}):", key_decisions.len()));
                    for d in &key_decisions {
                        parts.push(format!("  - {d}"));
                    }
                }
                parts.join("\n")
            }
        };

        let total_duration: u64 = tools_used.iter().map(|t| t.duration_ms).sum();

        AgentSummary {
            session_id: session_id.to_string(),
            granularity,
            files_changed: files_changed.to_vec(),
            tools_used: tools_used.iter().map(|t| t.tool_name.clone()).collect(),
            tasks_completed: tools_used.iter().filter(|t| t.success).count() as u32,
            tasks_pending: tools_used.iter().filter(|t| !t.success).count() as u32,
            key_decisions,
            summary_text,
            duration_ms: total_duration,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsageRecord {
    pub tool_name: String,
    pub description: String,
    pub duration_ms: u64,
    pub success: bool,
    pub is_write_operation: bool,
}
