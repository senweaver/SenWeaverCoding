// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use super::super::{TaskManagerHandle, TaskState};
use async_trait::async_trait;
use serde_json::json;

pub struct TaskUpdateTool {
    manager: TaskManagerHandle,
}

impl TaskUpdateTool {
    pub fn new(manager: TaskManagerHandle) -> Self {
        Self { manager }
    }
}

fn parse_state(s: &str) -> anyhow::Result<TaskState> {
    match s.to_ascii_lowercase().as_str() {
        "pending" => Ok(TaskState::Pending),
        "running" => Ok(TaskState::Running),
        "completed" => Ok(TaskState::Completed),
        "failed" => Ok(TaskState::Failed),
        "stopped" => Ok(TaskState::Stopped),
        _ => Err(anyhow::anyhow!(
            "Invalid state: expected pending, running, completed, failed, or stopped"
        )),
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "task_update"
    }

    fn description(&self) -> &str {
        "Update state, output, and/or error fields of a tracked background task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Task identifier" },
                "state": {
                    "type": "string",
                    "description": "New task state",
                    "enum": ["pending", "running", "completed", "failed", "stopped"]
                },
                "output": { "type": "string", "description": "Latest output text" },
                "error": { "type": "string", "description": "Error message if any" }
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
        let state = if let Some(v) = args.get("state").and_then(|x| x.as_str()) {
            Some(parse_state(v)?)
        } else {
            None
        };
        let output = args
            .get("output")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let error = args
            .get("error")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);

        let ok = self
            .manager
            .write()
            .update_task(task_id, state, output, error);
        if ok {
            Ok(ToolResult {
                success: true,
                output: json!({ "updated": true, "task_id": task_id }).to_string(),
                error: None,
            })
        } else {
            Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Task not found: {task_id}")),
            })
        }
    }
}
