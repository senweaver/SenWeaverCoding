// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::queue::{Task, TaskQueue};
use crate::memory::blackboard::BlackboardHandle;
use crate::runtime::task_manager::TaskHandle;

pub type TaskWorkerExecutor = Arc<
    dyn Fn(Task) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync,
>;

pub struct TaskQueueWorker {
    queue: Arc<TaskQueue>,
    blackboard: Option<BlackboardHandle>,
    agent_id: String,
    capabilities: Vec<String>,
    poll_interval: Duration,
    executor: TaskWorkerExecutor,
}

impl TaskQueueWorker {
    pub fn new(
        queue: Arc<TaskQueue>,
        capabilities: Vec<String>,
        executor: TaskWorkerExecutor,
    ) -> Self {
        let capabilities = if capabilities.is_empty() {
            vec!["general".to_string()]
        } else {
            capabilities
        };
        Self {
            queue,
            blackboard: None,
            agent_id: format!("task-worker-{}", &uuid::Uuid::new_v4().to_string()[..8]),
            capabilities,
            poll_interval: Duration::from_secs(2),
            executor,
        }
    }

    pub fn with_blackboard(mut self, blackboard: BlackboardHandle) -> Self {
        self.blackboard = Some(blackboard);
        self
    }

    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = agent_id.into();
        self
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval.max(Duration::from_millis(200));
        self
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn spawn(self) -> TaskHandle {
        let name = format!("task_orchestrator.worker.{}", self.agent_id);
        crate::runtime::task_manager::spawn_supervised(name, async move {
            self.run_loop().await;
        })
    }

    async fn run_loop(self) {
        tracing::info!(
            target: "agent.task_orchestrator.worker",
            agent_id = %self.agent_id,
            capabilities = ?self.capabilities,
            "task queue worker started",
        );

        loop {
            if crate::security::estop::is_kill_all() {
                tracing::warn!(
                    target: "agent.task_orchestrator.worker",
                    agent_id = %self.agent_id,
                    "estop kill_all engaged; task worker exiting",
                );
                return;
            }

            let mut claimed_any = false;
            for capability in &self.capabilities {
                if let Some(task) = self.queue.claim(&self.agent_id, capability) {
                    claimed_any = true;
                    self.run_task(task).await;
                    if crate::security::estop::is_kill_all() {
                        return;
                    }
                }
            }

            if !claimed_any {
                tokio::time::sleep(self.poll_interval).await;
            }
        }
    }

    async fn run_task(&self, task: Task) {
        let task_id = task.id.clone();
        let description = task.description.clone();

        self.write_blackboard(
            &task_id,
            serde_json::json!({
                "task_id": &task_id,
                "agent_id": &self.agent_id,
                "status": "running",
                "description": &description,
                "started_at": chrono::Utc::now().to_rfc3339(),
            }),
        );

        let result = (self.executor)(task).await;

        match result {
            Ok(output) => {
                if let Err(e) = self.queue.complete(&task_id, output.clone()) {
                    tracing::warn!(
                        target: "agent.task_orchestrator.worker",
                        task_id = %task_id,
                        error = %e,
                        "failed to mark task completed",
                    );
                }
                self.write_blackboard(
                    &task_id,
                    serde_json::json!({
                        "task_id": &task_id,
                        "agent_id": &self.agent_id,
                        "status": "completed",
                        "description": &description,
                        "result_preview": output.chars().take(400).collect::<String>(),
                        "finished_at": chrono::Utc::now().to_rfc3339(),
                    }),
                );
            }
            Err(err) => {
                if let Err(e) = self.queue.fail(&task_id, err.clone()) {
                    tracing::warn!(
                        target: "agent.task_orchestrator.worker",
                        task_id = %task_id,
                        error = %e,
                        "failed to mark task failed",
                    );
                }
                self.write_blackboard(
                    &task_id,
                    serde_json::json!({
                        "task_id": &task_id,
                        "agent_id": &self.agent_id,
                        "status": "failed",
                        "description": &description,
                        "error": err,
                        "finished_at": chrono::Utc::now().to_rfc3339(),
                    }),
                );
            }
        }
    }

    fn write_blackboard(&self, task_id: &str, value: serde_json::Value) {
        if let Some(blackboard) = self.blackboard.as_ref() {
            blackboard.inner().write(
                format!("task_worker/{task_id}"),
                value,
                "task_queue_worker",
                "task_worker",
            );
        }
    }
}

pub fn agent_run_executor(config: crate::config::Config) -> TaskWorkerExecutor {
    Arc::new(move |task: Task| {
        let config = config.clone();
        Box::pin(async move {
            let temperature = config.default_temperature;
            crate::agent::run(
                config,
                Some(task.prompt.clone()),
                None,
                None,
                temperature,
                Vec::new(),
                false,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| e.to_string())
        })
    })
}
