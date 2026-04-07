// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::TaskManagerHandle;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct TaskCreateTool {
    manager: TaskManagerHandle,
}

impl TaskCreateTool {
    pub fn new(manager: TaskManagerHandle) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "task_create"
    }

    fn description(&self) -> &str {
        "Create a new background task for tracking long-running operations like shell commands, agent delegations, or remote work."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_type": { "type": "string", "description": "Type of task (shell, agent, remote)", "enum": ["shell", "agent", "remote"] },
                "description": { "type": "string", "description": "Human-readable description of the task" }
            },
            "required": ["task_type", "description"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let task_type = args
            .get("task_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_type'"))?;
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'description'"))?;
        let id = self
            .manager
            .write()
            .create_task(task_type.to_string(), description.to_string());
        Ok(ToolResult {
            success: true,
            output: json!({ "task_id": id, "state": "pending" }).to_string(),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::task_manager::{TaskManager, TaskManagerHandle};
    use std::sync::Arc;

    #[test]
    fn name() {
        let mgr: TaskManagerHandle = Arc::new(parking_lot::RwLock::new(TaskManager::new()));
        let tool = TaskCreateTool::new(mgr);
        assert_eq!(tool.name(), "task_create");
    }
}
