// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::json;

use crate::agent::flows::registry::global_checkpoint_store;

use super::traits::{Tool, ToolResult};

pub struct FlowRollbackTool;

impl FlowRollbackTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FlowRollbackTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FlowRollbackTool {
    fn name(&self) -> &str {
        "flow_rollback"
    }

    fn description(&self) -> &str {
        "Rewind the flow checkpoint store by the requested number of \
         steps and return the restore target (artefacts + transcript).\n\
         Rolling back by N requires at least N+1 captured checkpoints."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 64,
                    "description": "Number of step-checkpoints to unwind (default 1).",
                },
                "session": {
                    "type": "string",
                    "description": "Session id to look up a persistent \
                                    checkpoint under `.sen/checkpoints/<session>/`.  When \
                                    omitted falls back to the in-memory FIFO rolled back \
                                    by `steps`."
                },
                "checkpoint": {
                    "type": "string",
                    "description": "Specific checkpoint id to load and \
                                    revert to (requires `session`).  Reverts every file \
                                    touched by the checkpoint's `edit_batch_id` through \
                                    the EditHistory journal."
                },
            },
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let steps = args
            .get("steps")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(64) as usize)
            .unwrap_or(1);

        let session = args
            .get("session")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let checkpoint_id = args
            .get("checkpoint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let store = global_checkpoint_store();

        if let (Some(session), Some(cp_id)) = (session.clone(), checkpoint_id) {
            if let Some(cp) = store.load_persisted(&session, &cp_id).await {
                let mut reverted_paths: Vec<String> = Vec::new();
                if let Some(batch_id) = cp.edit_batch_id.as_ref() {
                    let workspace = std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let history = crate::tools::edit_history::EditHistory::new(workspace);
                    match history.revert_batch(batch_id) {
                        Ok(paths) => reverted_paths = paths,
                        Err(err) => {
                            tracing::warn!(error = %err, "revert_batch failed");
                        }
                    }
                }
                let payload = json!({
                    "mode": "session_checkpoint",
                    "session": session,
                    "checkpoint_id": cp.id,
                    "label": cp.label,
                    "edit_batch_id": cp.edit_batch_id,
                    "reverted_paths": reverted_paths,
                });
                return Ok(ToolResult {
                    success: true,
                    output: payload.to_string(),
                    error: None,
                });
            }
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "persistent checkpoint '{cp_id}' for session '{session}' not found; \
                     falling back requires `steps` instead of `checkpoint`"
                )),
            });
        }

        let before_len = store.len();
        let target = match store.rollback(steps) {
            Some(t) => t,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "rollback by {steps} failed: only {before_len} checkpoint(s) \
                         available; need at least {} to roll back {steps} step(s)",
                        steps + 1
                    )),
                });
            }
        };

        let payload = json!({
            "id": target.id,
            "label": target.label,
            "artifacts": target
                .artifacts
                .iter()
                .map(|a| json!({
                    "step_id": a.step_id,
                    "content": a.content,
                }))
                .collect::<Vec<_>>(),
            "transcript_entries": target.transcript.len(),
            "checkpoints_remaining": store.len(),
            "edit_batch_id": target.edit_batch_id,
            "session_id": target.session_id,
        });
        Ok(ToolResult {
            success: true,
            output: payload.to_string(),
            error: None,
        })
    }
}
