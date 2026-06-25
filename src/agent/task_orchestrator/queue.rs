// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::TaskQueueError;

pub type TaskId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Critical = 4,
    High = 3,
    #[default]
    Normal = 2,
    Low = 1,
    Background = 0,
}

impl TaskPriority {
    fn weight(self) -> u8 {
        self as u8
    }
}

impl Ord for TaskPriority {
    fn cmp(&self, other: &Self) -> Ordering {

        self.weight().cmp(&other.weight())
    }
}

impl PartialOrd for TaskPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {

    Queued,

    Running,

    Completed,

    Failed,

    Cancelled,

    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {

    pub id: TaskId,

    pub description: String,

    pub prompt: String,

    pub required_capability: String,

    pub priority: TaskPriority,

    pub status: TaskStatus,

    pub submitted_by: String,

    pub claimed_by: Option<String>,

    pub submitted_at: DateTime<Utc>,

    pub claimed_at: Option<DateTime<Utc>>,

    pub finished_at: Option<DateTime<Utc>>,

    pub result: Option<String>,

    pub error: Option<String>,

    pub attempts: u32,

    pub max_retries: u32,

    pub deadline: Option<DateTime<Utc>>,

    pub context: Option<serde_json::Value>,

    pub tags: Vec<String>,
}

impl Task {

    pub fn new(
        description: impl Into<String>,
        prompt: impl Into<String>,
        capability: impl Into<String>,
        submitted_by: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "task-{}-{}",
                now.timestamp_millis(),
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            description: description.into(),
            prompt: prompt.into(),
            required_capability: capability.into(),
            priority: TaskPriority::Normal,
            status: TaskStatus::Queued,
            submitted_by: submitted_by.into(),
            claimed_by: None,
            submitted_at: now,
            claimed_at: None,
            finished_at: None,
            result: None,
            error: None,
            attempts: 0,
            max_retries: 2,
            deadline: None,
            context: None,
            tags: Vec::new(),
        }
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    pub fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = Some(context);
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(deadline) = self.deadline {
            Utc::now() > deadline
        } else {
            false
        }
    }

    pub fn can_retry(&self) -> bool {
        self.attempts < self.max_retries + 1
    }
}

#[derive(Debug, Clone)]
struct PrioritizedTask {
    task_id: TaskId,
    priority: TaskPriority,
    submitted_at: DateTime<Utc>,
}

impl PartialEq for PrioritizedTask {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}

impl Eq for PrioritizedTask {}

impl PartialOrd for PrioritizedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTask {
    fn cmp(&self, other: &Self) -> Ordering {

        self.priority
            .weight()
            .cmp(&other.priority.weight())
            .then_with(|| other.submitted_at.cmp(&self.submitted_at))
    }
}

pub struct TaskQueue {

    tasks: RwLock<HashMap<TaskId, Task>>,

    capability_index: RwLock<BTreeMap<String, BinaryHeap<PrioritizedTask>>>,
}

impl TaskQueue {

    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            capability_index: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn submit(&self, task: Task) -> TaskId {
        let task_id = task.id.clone();
        let prioritized = PrioritizedTask {
            task_id: task_id.clone(),
            priority: task.priority,
            submitted_at: task.submitted_at,
        };

        info!(
            task_id = %task_id,
            capability = %task.required_capability,
            priority = ?task.priority,
            "Task submitted"
        );

        self.capability_index
            .write()
            .entry(task.required_capability.clone())
            .or_default()
            .push(prioritized);

        self.tasks.write().insert(task_id.clone(), task);
        task_id
    }

    pub fn claim(&self, agent_id: &str, capability: &str) -> Option<Task> {
        let mut index = self.capability_index.write();
        let mut tasks = self.tasks.write();

        let queue = index.get_mut(capability)?;

        while let Some(candidate) = queue.pop() {
            if let Some(task) = tasks.get_mut(&candidate.task_id) {

                if task.status != TaskStatus::Queued {
                    continue;
                }
                if task.is_expired() {
                    task.status = TaskStatus::Expired;
                    continue;
                }

                task.status = TaskStatus::Running;
                task.claimed_by = Some(agent_id.to_string());
                task.claimed_at = Some(Utc::now());
                task.attempts += 1;
                debug!(task_id = %task.id, agent = %agent_id, "Task claimed");
                return Some(task.clone());
            }

        }

        None
    }

    pub fn complete(&self, task_id: &str, result: impl Into<String>) -> Result<(), TaskQueueError> {
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status != TaskStatus::Running {
                return Err(TaskQueueError::StatusMismatch {
                    task_id: task_id.to_string(),
                    expected: format!("{:?}", TaskStatus::Running),
                    found: format!("{:?}", task.status),
                });
            }
            task.status = TaskStatus::Completed;
            task.result = Some(result.into());
            task.finished_at = Some(Utc::now());
            info!(task_id = %task_id, "Task completed");
            Ok(())
        } else {
            Err(TaskQueueError::TaskNotFound(task_id.to_string()))
        }
    }

    pub fn fail(&self, task_id: &str, error: impl Into<String>) -> Result<(), TaskQueueError> {
        let error_str = error.into();

        let requeue = {
            let mut tasks = self.tasks.write();
            let Some(task) = tasks.get_mut(task_id) else {
                return Err(TaskQueueError::TaskNotFound(task_id.to_string()));
            };
            if task.status != TaskStatus::Running {
                return Err(TaskQueueError::StatusMismatch {
                    task_id: task_id.to_string(),
                    expected: format!("{:?}", TaskStatus::Running),
                    found: format!("{:?}", task.status),
                });
            }

            if task.can_retry() {
                task.status = TaskStatus::Queued;
                task.claimed_by = None;
                task.claimed_at = None;
                task.error = Some(error_str);
                warn!(task_id = %task_id, attempts = task.attempts, "Task failed, re-queuing");

                Some((
                    task.required_capability.clone(),
                    PrioritizedTask {
                        task_id: task_id.to_string(),
                        priority: task.priority,
                        submitted_at: task.submitted_at,
                    },
                ))
            } else {
                task.status = TaskStatus::Failed;
                task.error = Some(error_str);
                task.finished_at = Some(Utc::now());
                warn!(task_id = %task_id, "Task failed permanently (retries exhausted)");
                None
            }
        };

        if let Some((capability, prioritized)) = requeue {
            self.capability_index
                .write()
                .entry(capability)
                .or_default()
                .push(prioritized);
        }

        Ok(())
    }

    pub fn cancel(&self, task_id: &str) -> Result<(), TaskQueueError> {
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Completed || task.status == TaskStatus::Cancelled {
                return Err(TaskQueueError::StatusMismatch {
                    task_id: task_id.to_string(),
                    expected: "Queued | Running | PendingRetry".to_string(),
                    found: format!("{:?}", task.status),
                });
            }
            task.status = TaskStatus::Cancelled;
            task.finished_at = Some(Utc::now());
            info!(task_id = %task_id, "Task cancelled");
            Ok(())
        } else {
            Err(TaskQueueError::TaskNotFound(task_id.to_string()))
        }
    }

    pub fn get(&self, task_id: &str) -> Option<Task> {
        self.tasks.read().get(task_id).cloned()
    }

    pub fn pending_count(&self) -> usize {
        self.tasks
            .read()
            .values()
            .filter(|t| t.status == TaskStatus::Queued)
            .count()
    }

    pub fn running_count(&self) -> usize {
        self.tasks
            .read()
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .count()
    }

    pub fn total_count(&self) -> usize {
        self.tasks.read().len()
    }

    pub fn by_status(&self, status: TaskStatus) -> Vec<Task> {
        self.tasks
            .read()
            .values()
            .filter(|t| t.status == status)
            .cloned()
            .collect()
    }

    pub fn by_agent(&self, agent_id: &str) -> Vec<Task> {
        self.tasks
            .read()
            .values()
            .filter(|t| t.claimed_by.as_deref() == Some(agent_id))
            .cloned()
            .collect()
    }

    pub fn expire_overdue(&self) -> usize {
        let mut tasks = self.tasks.write();
        let mut count = 0;
        for task in tasks.values_mut() {
            if task.status == TaskStatus::Queued && task.is_expired() {
                task.status = TaskStatus::Expired;
                task.finished_at = Some(Utc::now());
                count += 1;
            }
        }
        if count > 0 {
            debug!(count, "Expired overdue tasks");
        }
        count
    }

    pub fn status_summary(&self) -> HashMap<String, usize> {
        let mut summary = HashMap::new();
        for task in self.tasks.read().values() {
            *summary.entry(format!("{:?}", task.status)).or_insert(0) += 1;
        }
        summary
    }

    pub fn purge_old(&self, max_age: Duration) -> usize {
        let cutoff = Utc::now() - chrono::Duration::from_std(max_age).unwrap_or_default();
        let mut tasks = self.tasks.write();
        let before = tasks.len();
        tasks.retain(|_, t| {
            if matches!(
                t.status,
                TaskStatus::Completed
                    | TaskStatus::Failed
                    | TaskStatus::Cancelled
                    | TaskStatus::Expired
            ) {
                t.finished_at.map(|f| f > cutoff).unwrap_or(true)
            } else {
                true
            }
        });
        before - tasks.len()
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct TaskQueueHandle {
    inner: Arc<TaskQueue>,
}

impl TaskQueueHandle {
    pub fn new(queue: TaskQueue) -> Self {
        Self {
            inner: Arc::new(queue),
        }
    }

    pub fn from_arc(arc: Arc<TaskQueue>) -> Self {
        Self { inner: arc }
    }

    pub fn inner(&self) -> &TaskQueue {
        &self.inner
    }

    pub fn inner_arc(&self) -> &Arc<TaskQueue> {
        &self.inner
    }

    pub fn submit(&self, task: Task) -> TaskId {
        self.inner.submit(task)
    }

    pub fn claim(&self, agent_id: &str, capability: &str) -> Option<Task> {
        self.inner.claim(agent_id, capability)
    }

    pub fn complete(&self, task_id: &str, result: impl Into<String>) -> Result<(), TaskQueueError> {
        self.inner.complete(task_id, result)
    }

    pub fn fail(&self, task_id: &str, error: impl Into<String>) -> Result<(), TaskQueueError> {
        self.inner.fail(task_id, error)
    }

    pub fn pending_count(&self) -> usize {
        self.inner.pending_count()
    }

    pub fn running_count(&self) -> usize {
        self.inner.running_count()
    }
}

impl From<TaskQueue> for TaskQueueHandle {
    fn from(queue: TaskQueue) -> Self {
        Self::new(queue)
    }
}
