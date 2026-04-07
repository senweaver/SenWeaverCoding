// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// InProcessTeammateTask — runs a teammate agent in the same process.
// Mirrors claude-code-typescript-src`tasks/InProcessTeammateTask/`.

use super::types::{Task, TaskHandle, TaskId, TaskState, TaskType, generate_task_id};
use tokio::sync::watch;

pub struct TeammateSpawnInput {
    pub prompt: String,
    pub description: String,
    pub team_name: String,
    pub tool_use_id: Option<String>,
    pub allowed_tools: Vec<String>,
}

pub struct InProcessTeammateTask;

impl InProcessTeammateTask {
    pub async fn spawn(input: TeammateSpawnInput) -> anyhow::Result<(TaskState, TaskHandle)> {
        let task_id = generate_task_id(TaskType::InProcessTeammate);
        let mut state = TaskState::new(
            task_id.clone(),
            TaskType::InProcessTeammate,
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
impl Task for InProcessTeammateTask {
    fn name(&self) -> &str {
        "InProcessTeammateTask"
    }

    fn task_type(&self) -> TaskType {
        TaskType::InProcessTeammate
    }

    async fn kill(&self, _task_id: &TaskId) -> anyhow::Result<()> {
        Ok(())
    }
}
