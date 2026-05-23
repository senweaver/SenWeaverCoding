// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::TaskManagerHandle;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct TaskGetTool {
    manager: TaskManagerHandle,
}

impl TaskGetTool {
    pub fn new(manager: TaskManagerHandle) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        "task_get"
    }

    fn description(&self) -> &str {
        "Get full details for a tracked background task by id."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task identifier returned by task_create" }
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
        let guard = self.manager.read();
        let Some(task) = guard.get_task(task_id) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Task not found: {task_id}")),
            });
        };
        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(task)?,
            error: None,
        })
    }
}
