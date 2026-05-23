// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::Mutex;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tracing::warn;

use super::traits::{Tool, ToolResult};
use crate::sop::types::{SopRunAction, SopStepResult, SopStepStatus};
use crate::sop::{SopAuditLogger, SopEngine, SopMetricsCollector};

pub struct SopAdvanceTool {
    engine: Arc<Mutex<SopEngine>>,
    audit: Option<Arc<SopAuditLogger>>,
    collector: Option<Arc<SopMetricsCollector>>,
}

impl SopAdvanceTool {
    pub fn new(engine: Arc<Mutex<SopEngine>>) -> Self {
        Self {
            engine,
            audit: None,
            collector: None,
        }
    }

    pub fn with_audit(mut self, audit: Arc<SopAuditLogger>) -> Self {
        self.audit = Some(audit);
        self
    }

    pub fn with_collector(mut self, collector: Arc<SopMetricsCollector>) -> Self {
        self.collector = Some(collector);
        self
    }
}

#[async_trait]
impl Tool for SopAdvanceTool {
    fn name(&self) -> &str {
        "sop_advance"
    }

    fn description(&self) -> &str {
        "Report the result of the current SOP step and advance to the next step. Provide the run_id, whether the step succeeded or failed, and a brief output summary."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "run_id": {
                    "type": "string",
                    "description": "The run ID to advance"
                },
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed", "skipped"],
                    "description": "Result status of the current step"
                },
                "output": {
                    "type": "string",
                    "description": "Brief summary of what happened in this step"
                }
            },
            "required": ["run_id", "status", "output"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let run_id = args
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'run_id' parameter"))?;

        let status_str = args
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'status' parameter"))?;

        let output = args
            .get("output")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'output' parameter"))?;

        let step_status = match status_str {
            "completed" => SopStepStatus::Completed,
            "failed" => SopStepStatus::Failed,
            "skipped" => SopStepStatus::Skipped,
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Invalid status '{other}'. Must be: completed, failed, or skipped"
                    )),
                });
            }
        };

        let (action, step_result_ok, finished_run) = {
            let mut engine = self.engine.lock();

            let current_step = engine
                .get_run(run_id)
                .map(|r| r.current_step)
                .ok_or_else(|| anyhow::anyhow!("Run not found: {run_id}"))?;

            let now = now_iso8601();
            let step_result = SopStepResult {
                step_number: current_step,
                status: step_status,
                output: output.to_string(),
                started_at: now.clone(),
                completed_at: Some(now),
            };
            let step_result_clone = step_result.clone();

            match engine.advance_step(run_id, step_result) {
                Ok(action) => {

                    let finished = match &action {
                        SopRunAction::Completed { run_id, .. }
                        | SopRunAction::Failed { run_id, .. } => engine.get_run(run_id).cloned(),
                        _ => None,
                    };

                    (Ok(action), Some(step_result_clone), finished)
                }
                Err(e) => (Err(e), None, None),
            }
        };

        if let Some(ref audit) = self.audit {
            if let Some(ref sr) = step_result_ok {
                if let Err(e) = audit.log_step_result(run_id, sr).await {
                    warn!("SOP audit log_step_result failed: {e}");
                }
            }
            if let Some(ref run) = finished_run {
                if let Err(e) = audit.log_run_complete(run).await {
                    warn!("SOP audit log_run_complete failed: {e}");
                }
            }
        }

        if let Some(ref collector) = self.collector {
            if let Some(ref run) = finished_run {
                collector.record_run_complete(run);
            }
        }

        match action {
            Ok(action) => {
                let result_output = match action {
                    SopRunAction::ExecuteStep {
                        run_id, context, ..
                    } => {
                        format!("Step recorded. Next step for run {run_id}:\n\n{context}")
                    }
                    SopRunAction::WaitApproval {
                        run_id, context, ..
                    } => {
                        format!(
                            "Step recorded. Next step for run {run_id} (waiting for approval):\n\n{context}"
                        )
                    }
                    SopRunAction::Completed { run_id, sop_name } => {
                        format!("SOP '{sop_name}' run {run_id} completed successfully.")
                    }
                    SopRunAction::Failed {
                        run_id,
                        sop_name,
                        reason,
                    } => {
                        format!("SOP '{sop_name}' run {run_id} failed: {reason}")
                    }
                    SopRunAction::DeterministicStep { run_id, step, .. } => {
                        format!(
                            "Step recorded. Next deterministic step for run {run_id}: {}",
                            step.title
                        )
                    }
                    SopRunAction::CheckpointWait { run_id, step, .. } => {
                        format!(
                            "Step recorded. Run {run_id} paused at checkpoint: {}",
                            step.title
                        )
                    }
                };
                Ok(ToolResult {
                    success: true,
                    output: result_output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to advance step: {e}")),
            }),
        }
    }
}

use crate::sop::engine::now_iso8601;
