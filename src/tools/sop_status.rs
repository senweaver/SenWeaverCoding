// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::Mutex;
use std::fmt::Write;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::traits::{Tool, ToolResult};
use crate::sop::{SopEngine, SopMetricsCollector};

pub struct SopStatusTool {
    engine: Arc<Mutex<SopEngine>>,
    collector: Option<Arc<SopMetricsCollector>>,
}

impl SopStatusTool {
    pub fn new(engine: Arc<Mutex<SopEngine>>) -> Self {
        Self {
            engine,
            collector: None,
        }
    }

    pub fn with_collector(mut self, collector: Arc<SopMetricsCollector>) -> Self {
        self.collector = Some(collector);
        self
    }

    fn append_gate_status(&self, output: &mut String, include_gate_status: bool) {
        if include_gate_status {
            let _ = writeln!(
                output,
                "\nGate Status: not available (gate evaluation not supported)"
            );
        }
    }
}

#[async_trait]
impl Tool for SopStatusTool {
    fn name(&self) -> &str {
        "sop_status"
    }

    fn description(&self) -> &str {
        "Query SOP execution status. Provide run_id for a specific run, or sop_name to list runs for that SOP. With no arguments, shows all active runs."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "run_id": {
                    "type": "string",
                    "description": "Specific run ID to query"
                },
                "sop_name": {
                    "type": "string",
                    "description": "SOP name to list runs for"
                },
                "include_metrics": {
                    "type": "boolean",
                    "description": "Include aggregated SOP metrics (completion rate, deviation rate, intervention counts, windowed variants)"
                },
                "include_gate_status": {
                    "type": "boolean",
                    "description": "Include trust phase and gate evaluation status"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let run_id = args.get("run_id").and_then(|v| v.as_str());
        let sop_name = args.get("sop_name").and_then(|v| v.as_str());
        let include_metrics = args
            .get("include_metrics")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_gate_status = args
            .get("include_gate_status")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let engine = self.engine.lock();

        if let Some(run_id) = run_id {
            return match engine.get_run(run_id) {
                Some(run) => {
                    let mut output = format!(
                        "Run: {}\nSOP: {}\nStatus: {}\nStep: {} of {}\nStarted: {}\n",
                        run.run_id,
                        run.sop_name,
                        run.status,
                        run.current_step,
                        run.total_steps,
                        run.started_at,
                    );
                    if let Some(ref completed) = run.completed_at {
                        let _ = writeln!(output, "Completed: {completed}");
                    }
                    if !run.step_results.is_empty() {
                        let _ = writeln!(output, "\nStep results:");
                        for step in &run.step_results {
                            let _ = writeln!(
                                output,
                                "  Step {}: {} — {}",
                                step.step_number, step.status, step.output
                            );
                        }
                    }
                    self.append_gate_status(&mut output, include_gate_status);
                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    })
                }
                None => Ok(ToolResult {
                    success: true,
                    output: format!("No run found with ID '{run_id}'."),
                    error: None,
                }),
            };
        }

        let mut output = String::new();

        let active: Vec<_> = engine
            .active_runs()
            .values()
            .filter(|r| sop_name.map_or(true, |name| r.sop_name == name))
            .collect();

        if active.is_empty() {
            let scope = sop_name.map_or(String::new(), |n| format!(" for '{n}'"));
            let _ = writeln!(output, "No active runs{scope}.");
        } else {
            let _ = writeln!(output, "Active runs ({}):", active.len());
            for run in &active {
                let _ = writeln!(
                    output,
                    "  {} — {} [{}] step {}/{}",
                    run.run_id, run.sop_name, run.status, run.current_step, run.total_steps
                );
            }
        }

        let finished = engine.finished_runs(sop_name);
        if !finished.is_empty() {
            let _ = writeln!(output, "\nFinished runs ({}):", finished.len());
            for run in finished.iter().rev().take(10) {
                let _ = writeln!(
                    output,
                    "  {} — {} [{}] ({})",
                    run.run_id,
                    run.sop_name,
                    run.status,
                    run.completed_at.as_deref().unwrap_or("?")
                );
            }
        }

        if include_metrics {
            if let Some(ref collector) = self.collector {
                let prefix = sop_name.map_or("sop".to_string(), |n| format!("sop.{n}"));
                let _ = writeln!(output, "\nMetrics ({prefix}):");
                for suffix in METRIC_SUFFIXES {
                    let key = format!("{prefix}.{suffix}");
                    if let Some(val) = collector.get_metric_value(&key) {
                        let _ = writeln!(output, "  {suffix}: {}", format_metric_value(&val));
                    }
                }
            } else {
                let _ = writeln!(
                    output,
                    "\nMetrics: not available (collector not configured)"
                );
            }
        }

        self.append_gate_status(&mut output, include_gate_status);

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

const METRIC_SUFFIXES: &[&str] = &[
    "runs_completed",
    "runs_failed",
    "runs_cancelled",
    "completion_rate",
    "deviation_rate",
    "protocol_adherence_rate",
    "human_intervention_count",
    "human_intervention_rate",
    "timeout_auto_approvals",
    "timeout_approval_rate",
    "completion_rate_7d",
    "deviation_rate_7d",
    "completion_rate_30d",
    "deviation_rate_30d",
];

fn format_metric_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                format!("{u}")
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 {
                    format!("{f:.0}")
                } else {
                    format!("{f:.4}")
                }
            } else {
                n.to_string()
            }
        }
        other => other.to_string(),
    }
}
