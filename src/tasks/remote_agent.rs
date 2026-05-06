// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// RemoteAgentTask — represents a task running on a remote agent instance.
// Mirrors claude-code-typescript-src`tasks/RemoteAgentTask/`.

use super::types::{Task, TaskHandle, TaskId, TaskState, TaskType, generate_task_id};
use tokio::sync::watch;

pub struct RemoteAgentSpawnInput {
    pub endpoint_url: String,
    pub prompt: String,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub auth_token: Option<String>,
}

pub struct RemoteAgentTask;

impl RemoteAgentTask {

    pub async fn spawn(input: RemoteAgentSpawnInput) -> anyhow::Result<(TaskState, TaskHandle)> {
        let task_id = generate_task_id(TaskType::RemoteAgent);
        let mut state = TaskState::new(
            task_id.clone(),
            TaskType::RemoteAgent,
            input.description.clone(),
            input.tool_use_id.clone(),
        );
        state.mark_running();

        let (cancel_tx, _cancel_rx) = watch::channel(false);

        let handle = TaskHandle {
            task_id,
            cancel_tx: Some(cancel_tx),
            cleanup: None,
        };

        Ok((state, handle))
    }
}

#[async_trait::async_trait]
impl Task for RemoteAgentTask {
    fn name(&self) -> &str {
        "RemoteAgentTask"
    }

    fn task_type(&self) -> TaskType {
        TaskType::RemoteAgent
    }

    async fn kill(&self, _task_id: &TaskId) -> anyhow::Result<()> {
        Ok(())
    }
}
