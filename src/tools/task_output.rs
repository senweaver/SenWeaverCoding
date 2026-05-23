// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::TaskManagerHandle;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct TaskOutputTool {
    manager: TaskManagerHandle,
}

impl TaskOutputTool {
    pub fn new(manager: TaskManagerHandle) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "task_output"
    }

    fn description(&self) -> &str {
        "Get the accumulated output text for a tracked background task."
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
        match self.manager.read().get_output(task_id) {
            Some(output) => Ok(ToolResult {
                success: true,
                output: json!({ "output": output }).to_string(),
                error: None,
            }),
            None => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Task not found: {task_id}")),
            }),
        }
    }
}
