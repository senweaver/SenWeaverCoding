// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::Mutex;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tracing::warn;

use super::super::traits::{Tool, ToolResult};
use crate::sop::types::SopRunAction;
use crate::sop::{SopAuditLogger, SopEngine, SopMetricsCollector};

pub struct SopApproveTool {
    engine: Arc<Mutex<SopEngine>>,
    audit: Option<Arc<SopAuditLogger>>,
    collector: Option<Arc<SopMetricsCollector>>,
}

impl SopApproveTool {
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
impl Tool for SopApproveTool {
    fn name(&self) -> &str {
        "sop_approve"
    }

    fn description(&self) -> &str {
        "Approve a pending SOP step that is waiting for operator approval. Returns the step instruction to execute. Use sop_status to see which runs are waiting."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "run_id": {
                    "type": "string",
                    "description": "The run ID to approve"
                }
            },
            "required": ["run_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let run_id = args
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'run_id' parameter"))?;

        let (result, run_snapshot, headless_driven) = {
            let mut engine = self.engine.lock();
            let headless_driven = engine
                .get_run(run_id)
                .map(|r| r.headless_driven)
                .unwrap_or(false);

            match engine.approve_step(run_id) {
                Ok(action) => {
                    let snapshot = engine.get_run(run_id).cloned();
                    (Ok(action), snapshot, headless_driven)
                }
                Err(e) => (Err(e), None, headless_driven),
            }
        };

        if let Some(ref audit) = self.audit {
            if let Some(ref run) = run_snapshot {
                if let Err(e) = audit.log_approval(run, run.current_step).await {
                    warn!("SOP audit log after approve failed: {e}");
                }
            }
        }

        if let Some(ref collector) = self.collector {
            if let Some(ref run) = run_snapshot {
                collector.record_approval(&run.sop_name, &run.run_id);
            }
        }

        match result {
            Ok(action) => {
                if headless_driven {
                    let audit = self.audit.clone().unwrap_or_else(|| {
                        Arc::new(SopAuditLogger::new(Arc::new(
                            crate::memory::none::NoneMemory::new(),
                        )))
                    });
                    crate::sop::runner::enqueue_action(
                        Arc::clone(&self.engine),
                        audit,
                        action.clone(),
                    );
                    let output = match &action {
                        SopRunAction::ExecuteStep { run_id, .. } => {
                            format!(
                                "Approved. Headless driver will continue run {run_id}; do not \
                                 re-execute this step yourself."
                            )
                        }
                        other => format!(
                            "Approved. Headless driver will handle the next action ({other:?})."
                        ),
                    };
                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    })
                } else {
                    let output = match action {
                        SopRunAction::ExecuteStep {
                            run_id, context, ..
                        } => {
                            format!("Approved. Proceeding with run {run_id}.\n\n{context}")
                        }
                        other => format!("Approved. Action: {other:?}"),
                    };
                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Approval failed: {e}")),
            }),
        }
    }
}
