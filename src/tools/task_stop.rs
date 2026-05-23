// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::TaskManagerHandle;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct TaskStopTool {
    manager: TaskManagerHandle,
}

impl TaskStopTool {
    pub fn new(manager: TaskManagerHandle) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "task_stop"
    }

    fn description(&self) -> &str {
        "Request stop for a pending or running background task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task identifier" }
            },
            "required": ["task_id"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_id'"))?;
        let ok = self.manager.write().stop_task(task_id);
        Ok(ToolResult {
            success: ok,
            output: json!({ "stopped": ok }).to_string(),
            error: if ok {
                None
            } else {
                Some("Task not found or not in a stoppable state (pending/running)".to_string())
            },
        })
    }
}
