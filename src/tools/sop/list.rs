// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::Mutex;
use std::fmt::Write;

use async_trait::async_trait;
use serde_json::json;

use super::super::traits::{Tool, ToolResult};
use crate::sop::SopEngine;

pub struct SopListTool {
    engine: std::sync::Arc<Mutex<SopEngine>>,
}

impl SopListTool {
    pub fn new(engine: std::sync::Arc<Mutex<SopEngine>>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Tool for SopListTool {
    fn name(&self) -> &str {
        "sop_list"
    }

    fn description(&self) -> &str {
        "List all loaded Standard Operating Procedures (SOPs) with their triggers, priority, step count, and active run count. Optionally filter by name or priority."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "string",
                    "description": "Filter SOPs by name substring or priority (low/normal/high/critical)"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
        let filter_lower = filter.to_lowercase();

        let engine = self.engine.lock();
        let sops = engine.sops();

        if sops.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No SOPs loaded.".into(),
                error: None,
            });
        }

        let filtered: Vec<_> = if filter_lower.is_empty() {
            sops.iter().collect()
        } else {
            sops.iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&filter_lower)
                        || s.priority.to_string() == filter_lower
                })
                .collect()
        };

        if filtered.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No SOPs match filter '{filter}'."),
                error: None,
            });
        }

        let active_runs = engine.active_runs();
        let mut output = format!(
            "Loaded SOPs ({} total, {} shown):\n\n",
            sops.len(),
            filtered.len()
        );

        for sop in &filtered {
            let active_count = active_runs
                .values()
                .filter(|r| r.sop_name == sop.name)
                .count();
            let triggers: Vec<String> = sop.triggers.iter().map(|t| t.to_string()).collect();

            let _ = writeln!(
                output,
                "- **{}** [{}]  - {} steps, {} trigger(s): {}{}",
                sop.name,
                sop.priority,
                sop.steps.len(),
                sop.triggers.len(),
                triggers.join(", "),
                if active_count > 0 {
                    format!(" (active runs: {active_count})")
                } else {
                    String::new()
                }
            );
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
