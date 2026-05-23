// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::Mutex;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tracing::warn;

use super::traits::{Tool, ToolResult};
use crate::sop::types::{SopEvent, SopRunAction, SopTriggerSource};
use crate::sop::{SopAuditLogger, SopEngine};

pub struct SopExecuteTool {
    engine: Arc<Mutex<SopEngine>>,
    audit: Option<Arc<SopAuditLogger>>,
}

impl SopExecuteTool {
    pub fn new(engine: Arc<Mutex<SopEngine>>) -> Self {
        Self {
            engine,
            audit: None,
        }
    }

    pub fn with_audit(mut self, audit: Arc<SopAuditLogger>) -> Self {
        self.audit = Some(audit);
        self
    }
}

#[async_trait]
impl Tool for SopExecuteTool {
    fn name(&self) -> &str {
        "sop_execute"
    }

    fn description(&self) -> &str {
        "Manually trigger a Standard Operating Procedure (SOP) by name. Returns the run ID and first step instruction. Use sop_list to see available SOPs."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the SOP to execute"
                },
                "payload": {
                    "type": "string",
                    "description": "Optional trigger payload (JSON string)"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let sop_name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' parameter"))?;

        let payload = args
            .get("payload")
            .and_then(|v| v.as_str())
            .map(String::from);

        let event = SopEvent {
            source: SopTriggerSource::Manual,
            topic: None,
            payload,
            timestamp: now_iso8601(),
        };

        let (action, run_snapshot) = {
            let mut engine = self.engine.lock();

            match engine.start_run(sop_name, event) {
                Ok(action) => {
                    let run_id = action_run_id(&action);
                    let snapshot = run_id.and_then(|id| engine.get_run(id).cloned());
                    (Ok(action), snapshot)
                }
                Err(e) => (Err(e), None),
            }
        };

        if let Some(ref audit) = self.audit {
            if let Some(ref run) = run_snapshot {
                if let Err(e) = audit.log_run_start(run).await {
                    warn!("SOP audit log_run_start failed: {e}");
                }
            }
        }

        match action {
            Ok(action) => {
                let output = match action {
                    SopRunAction::ExecuteStep {
                        run_id, context, ..
                    } => {
                        format!("SOP run started: {run_id}\n\n{context}")
                    }
                    SopRunAction::WaitApproval {
                        run_id, context, ..
                    } => {
                        format!("SOP run started: {run_id} (waiting for approval)\n\n{context}")
                    }
                    SopRunAction::Completed { run_id, sop_name } => {
                        format!("SOP '{sop_name}' run {run_id} completed immediately (no steps).")
                    }
                    SopRunAction::Failed { run_id, reason, .. } => {
                        format!("SOP run {run_id} failed: {reason}")
                    }
                    SopRunAction::DeterministicStep { run_id, step, .. } => {
                        format!(
                            "SOP run started (deterministic): {run_id}\nFirst step: {}",
                            step.title
                        )
                    }
                    SopRunAction::CheckpointWait { run_id, step, .. } => {
                        format!(
                            "SOP run started: {run_id} (paused at checkpoint: {})",
                            step.title
                        )
                    }
                };
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to start SOP: {e}")),
            }),
        }
    }
}

fn action_run_id(action: &SopRunAction) -> Option<&str> {
    match action {
        SopRunAction::ExecuteStep { run_id, .. }
        | SopRunAction::WaitApproval { run_id, .. }
        | SopRunAction::Completed { run_id, .. }
        | SopRunAction::Failed { run_id, .. }
        | SopRunAction::DeterministicStep { run_id, .. }
        | SopRunAction::CheckpointWait { run_id, .. } => Some(run_id),
    }
}

use crate::sop::engine::now_iso8601;
