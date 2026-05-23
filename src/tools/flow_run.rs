// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::json;

use crate::agent::flows::builtins::{CodeEditFlow, ResearchFlow};
use crate::agent::flows::plan_exec_verify::PlanExecVerifyOptions;
use crate::agent::flows::registry::global_agent_handle;
use crate::agent::flows::traits::{Flow, FlowContext, FlowError};

use super::traits::{Tool, ToolResult};

pub struct FlowRunTool;

impl FlowRunTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FlowRunTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FlowRunTool {
    fn name(&self) -> &str {
        "flow_run"
    }

    fn description(&self) -> &str {
        "Run a named built-in flow (code_edit | research) against the \
         currently-registered agent handle.  Flows own their own \
         plan → execute → verify → fix loop and return the final \
         artefacts as a single structured payload."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "flow": {
                    "type": "string",
                    "enum": ["code_edit", "research"],
                    "description": "Which built-in flow to run.",
                },
                "goal": {
                    "type": "string",
                    "description": "High-level goal passed to the flow planner.",
                },
                "language": {
                    "type": "string",
                    "description": "Language hint for code_edit (rust, python, ...).",
                },
                "max_fix_attempts": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Maximum fix-loop attempts per step (default 3).",
                },
            },
            "required": ["flow", "goal"],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let flow_name = args.get("flow").and_then(|v| v.as_str()).unwrap_or("");
        let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or("");
        if flow_name.is_empty() || goal.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("`flow` and `goal` are required".into()),
            });
        }

        let max_fix = args
            .get("max_fix_attempts")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(10) as u32)
            .unwrap_or(3);
        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("rust")
            .to_string();

        let agent = match global_agent_handle() {
            Some(h) => h,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "no flow agent handle registered; call \
                         `senweavercoding::agent::flows::set_global_agent_handle` \
                         at startup"
                            .into(),
                    ),
                });
            }
        };

        let options = PlanExecVerifyOptions {
            max_fix_attempts: max_fix,
            allow_single_replan: false,
            emit_checkpoints: false,
            ..PlanExecVerifyOptions::default()
        };

        let started_at = std::time::Instant::now();
        let mut ctx = FlowContext::new(goal);
        let outcome = match flow_name {
            "code_edit" => {
                let flow = CodeEditFlow {
                    language,
                    options,
                    ..CodeEditFlow::default()
                };
                flow.run(&mut ctx, agent.as_ref()).await
            }
            "research" => {
                let flow = ResearchFlow {
                    min_sources: 1,
                    options,
                };
                flow.run(&mut ctx, agent.as_ref()).await
            }
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("unknown flow: {other}")),
                });
            }
        };

        let duration = started_at.elapsed();
        let (outcome_label, result) = match outcome {
            Ok(outcome) => {
                let payload = json!({
                    "flow": flow_name,
                    "iterations": outcome.iterations,
                    "artifacts": outcome
                        .artifacts
                        .iter()
                        .map(|a| json!({
                            "step_id": a.step_id,
                            "language": a.language,
                            "content": a.content,
                        }))
                        .collect::<Vec<_>>(),
                });
                (
                    "success".to_string(),
                    Ok(ToolResult {
                        success: true,
                        output: payload.to_string(),
                        error: None,
                    }),
                )
            }
            Err(e) => {
                let (label, msg) = match &e {
                    FlowError::FixLoopExhausted(n) => (
                        "fix_loop_exhausted".to_string(),
                        format!("fix loop exhausted after {n} attempts"),
                    ),
                    FlowError::Planner(_) => ("planner_error".into(), e.to_string()),
                    FlowError::Executor(_) => ("executor_error".into(), e.to_string()),
                    FlowError::Verifier(_) => ("verifier_error".into(), e.to_string()),
                    FlowError::AgentHandle(_) => ("agent_handle_error".into(), e.to_string()),
                    FlowError::Cancelled => ("cancelled".into(), e.to_string()),
                    FlowError::Other(_) => ("other".into(), e.to_string()),
                };
                (
                    label,
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(msg),
                    }),
                )
            }
        };

        if let Some(obs) = crate::observability::global_observer() {
            obs.record_metric(&crate::observability::traits::ObserverMetric::FlowRun {
                flow: flow_name.to_string(),
                outcome: outcome_label,
                duration,
            });
        }
        result
    }
}
