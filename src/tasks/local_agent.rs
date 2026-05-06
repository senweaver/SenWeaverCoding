// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// LocalAgentTask — spawns a local sub-agent process.
// Mirrors claude-code-typescript-src`tasks/LocalAgentTask/`.

use std::path::PathBuf;

use super::types::{Task, TaskHandle, TaskId, TaskState, TaskType, generate_task_id};
use tokio::sync::watch;

pub struct LocalAgentSpawnInput {
    pub prompt: String,
    pub description: String,
    pub agent_definition: Option<String>,
    pub tool_use_id: Option<String>,
    pub allowed_tools: Vec<String>,
    pub cwd: PathBuf,
}

pub struct LocalAgentTask;

impl LocalAgentTask {

    pub async fn spawn(input: LocalAgentSpawnInput) -> anyhow::Result<(TaskState, TaskHandle)> {
        let task_id = generate_task_id(TaskType::LocalAgent);
        let mut state = TaskState::new(
            task_id.clone(),
            TaskType::LocalAgent,
            input.description.clone(),
            input.tool_use_id.clone(),
        );
        state.mark_running();

        let (cancel_tx, cancel_rx) = watch::channel(false);

        let prompt = input.prompt.clone();
        let task_id_clone = task_id.clone();
        let _cwd = input.cwd.clone();
        let allowed = input.allowed_tools.clone();

        let _ = crate::runtime::spawn_supervised(
            format!("tasks.local_agent.{}", task_id_clone),
            async move {
                tracing::info!(task_id = %task_id_clone, "Sub-agent task starting");

                let allowed_tools = if allowed.is_empty() {
                    None
                } else {
                    Some(allowed)
                };

                let config = match crate::config::Config::load_or_init().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(task_id = %task_id_clone, error = %e, "Failed to load config for sub-agent");
                        return;
                    }
                };

                tokio::select! {
                    result = crate::agent::run(
                        config,
                        Some(prompt),
                        None,
                        None,
                        0.7,
                        Vec::new(),
                        false,
                        None,
                        allowed_tools,
                    ) => {
                        match result {
                            Ok(response) => {
                                tracing::info!(task_id = %task_id_clone, "Sub-agent completed: {}", &response[..response.len().min(200)]);
                            }
                            Err(e) => {
                                tracing::error!(task_id = %task_id_clone, error = %e, "Sub-agent failed");
                            }
                        }
                    }
                    _ = async {
                        let mut rx = cancel_rx;
                        while !*rx.borrow() {
                            if rx.changed().await.is_err() {
                                break;
                            }
                        }
                    } => {
                        tracing::info!(task_id = %task_id_clone, "Sub-agent cancelled");
                    }
                }
            },
        );

        let handle = TaskHandle {
            task_id,
            cancel_tx: Some(cancel_tx),
            cleanup: None,
        };

        Ok((state, handle))
    }
}

#[async_trait::async_trait]
impl Task for LocalAgentTask {
    fn name(&self) -> &str {
        "LocalAgentTask"
    }

    fn task_type(&self) -> TaskType {
        TaskType::LocalAgent
    }

    async fn kill(&self, _task_id: &TaskId) -> anyhow::Result<()> {
        Ok(())
    }
}
