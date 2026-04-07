// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// DreamTask — background "dreaming" task for proactive insights.
// Mirrors claude-code-typescript-src`tasks/DreamTask/`.

use super::types::{Task, TaskHandle, TaskId, TaskState, TaskType, generate_task_id};
use tokio::sync::watch;

pub struct DreamSpawnInput {
    pub description: String,
    pub tool_use_id: Option<String>,
    pub interval_ms: Option<u64>,
}

pub struct DreamTask;

impl DreamTask {
    pub async fn spawn(input: DreamSpawnInput) -> anyhow::Result<(TaskState, TaskHandle)> {
        let task_id = generate_task_id(TaskType::Dream);
        let mut state = TaskState::new(
            task_id.clone(),
            TaskType::Dream,
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
impl Task for DreamTask {
    fn name(&self) -> &str {
        "DreamTask"
    }

    fn task_type(&self) -> TaskType {
        TaskType::Dream
    }

    async fn kill(&self, _task_id: &TaskId) -> anyhow::Result<()> {
        Ok(())
    }
}
