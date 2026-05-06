// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::TaskManagerHandle;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct TaskListTool {
    manager: TaskManagerHandle,
}

impl TaskListTool {
    pub fn new(manager: TaskManagerHandle) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "task_list"
    }

    fn description(&self) -> &str {
        "List all tracked background tasks and their metadata."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let list: Vec<_> = self
            .manager
            .read()
            .list_tasks()
            .into_iter()
            .cloned()
            .collect();
        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&list)?,
            error: None,
        })
    }
}
