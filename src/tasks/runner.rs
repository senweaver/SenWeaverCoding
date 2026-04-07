// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// TaskRunner — manages the lifecycle of all background tasks.
// Mirrors claude-code's task state management in AppState.
//
// This is the **runtime-facing** task manager used by `ServiceContainer` to track
// actual background processes (shell commands, sub-agents, workflows). It uses
// typed `TaskId`, async `tokio::sync::RwLock`, and rich `TaskState` with timing,
// output files, and cancellation support.
//
// For the **tool-facing** lightweight task CRUD store that the LLM manipulates
// through the `task_*` tool family, see `crate::tools::task_types::TaskManager`.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::{TaskId, TaskState, TaskStatus, TaskType, is_terminal_status};

/// Central task manager tracking all active and completed tasks.
#[derive(Debug, Clone)]
pub struct TaskRunner {
    inner: Arc<RwLock<TaskRunnerInner>>,
}

#[derive(Debug, Default)]
struct TaskRunnerInner {
    tasks: HashMap<TaskId, TaskState>,
}

impl TaskRunner {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TaskRunnerInner::default())),
        }
    }

    /// Register a new task.
    pub async fn register(&self, state: TaskState) {
        let mut inner = self.inner.write().await;
        inner.tasks.insert(state.id.clone(), state);
    }

    /// Get a snapshot of a task's state.
    pub async fn get(&self, id: &TaskId) -> Option<TaskState> {
        let inner = self.inner.read().await;
        inner.tasks.get(id).cloned()
    }

    /// Update a task's status.
    pub async fn update_status(&self, id: &TaskId, status: TaskStatus) {
        let mut inner = self.inner.write().await;
        if let Some(task) = inner.tasks.get_mut(id) {
            match status {
                TaskStatus::Running => task.mark_running(),
                TaskStatus::Completed => task.mark_completed(),
                TaskStatus::Failed => task.mark_failed(),
                TaskStatus::Killed => task.mark_killed(),
                TaskStatus::Pending => {}
            }
        }
    }

    /// List all tasks, optionally filtered by type or status.
    pub async fn list(
        &self,
        type_filter: Option<TaskType>,
        status_filter: Option<TaskStatus>,
    ) -> Vec<TaskState> {
        let inner = self.inner.read().await;
        inner
            .tasks
            .values()
            .filter(|t| type_filter.map_or(true, |ty| t.task_type == ty))
            .filter(|t| status_filter.map_or(true, |st| t.status == st))
            .cloned()
            .collect()
    }

    /// List only active (non-terminal) tasks.
    pub async fn list_active(&self) -> Vec<TaskState> {
        let inner = self.inner.read().await;
        inner
            .tasks
            .values()
            .filter(|t| !is_terminal_status(t.status))
            .cloned()
            .collect()
    }

    /// Remove terminal tasks older than `max_age`.
    pub async fn evict_completed(&self, max_age: std::time::Duration) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let threshold = now_ms.saturating_sub(max_age.as_millis() as u64);
        let mut inner = self.inner.write().await;
        inner.tasks.retain(|_, t| {
            if is_terminal_status(t.status) {
                t.end_time_epoch_ms.unwrap_or(u64::MAX) > threshold
            } else {
                true
            }
        });
    }

    /// Kill all active tasks (used during graceful shutdown).
    pub async fn kill_all(&self) {
        let mut inner = self.inner.write().await;
        for task in inner.tasks.values_mut() {
            if !is_terminal_status(task.status) {
                task.mark_killed();
            }
        }
    }
}

impl Default for TaskRunner {
    fn default() -> Self {
        Self::new()
    }
}
