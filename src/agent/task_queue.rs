// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Task Queue — work distribution system for multi-agent coordination.
//!
//! Provides a priority-based task queue where:
//! - Tasks can be submitted with required capabilities
//! - Agents claim tasks from the queue based on capability matching
//! - Failed tasks are automatically re-queued with retry limits
//! - Supports priority levels and deadline-based ordering

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Unique task identifier.
pub type TaskId = String;

/// Task priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Critical = 4,
    High = 3,
    Normal = 2,
    Low = 1,
    Background = 0,
}

impl TaskPriority {
    fn weight(&self) -> u8 {
        *self as u8
    }
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Current state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Waiting in the queue for an agent to claim it.
    Queued,
    /// Claimed by an agent and currently running.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed (may be retried).
    Failed,
    /// Cancelled.
    Cancelled,
    /// Deadline expired before completion.
    Expired,
}

/// A task in the queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task ID.
    pub id: TaskId,
    /// Human-readable description.
    pub description: String,
    /// The prompt / work to perform.
    pub prompt: String,
    /// Required capability for the agent.
    pub required_capability: String,
    /// Task priority.
    pub priority: TaskPriority,
    /// Current status.
    pub status: TaskStatus,
    /// Who submitted this task.
    pub submitted_by: String,
    /// Which agent is working on this (if claimed).
    pub claimed_by: Option<String>,
    /// When the task was submitted.
    pub submitted_at: DateTime<Utc>,
    /// When the task was claimed.
    pub claimed_at: Option<DateTime<Utc>>,
    /// When the task finished (completed/failed).
    pub finished_at: Option<DateTime<Utc>>,
    /// Task result (if completed).
    pub result: Option<String>,
    /// Error message (if failed).
    pub error: Option<String>,
    /// Number of times this task has been attempted.
    pub attempts: u32,
    /// Maximum retry attempts (0 = no retries).
    pub max_retries: u32,
    /// Optional deadline (task expires if not completed by this time).
    pub deadline: Option<DateTime<Utc>>,
    /// Optional context data.
    pub context: Option<serde_json::Value>,
    /// Tags for filtering.
    pub tags: Vec<String>,
}

impl Task {
    /// Create a new task.
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
                uuid::Uuid::new_v4().to_string()[..8].to_string()
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

    /// Set priority.
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set max retries.
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set deadline.
    pub fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Set context.
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = Some(context);
        self
    }

    /// Check if the task has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(deadline) = self.deadline {
            Utc::now() > deadline
        } else {
            false
        }
    }

    /// Check if the task can be retried.
    pub fn can_retry(&self) -> bool {
        self.attempts < self.max_retries + 1
    }
}

/// Wrapper for priority queue ordering.
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
        // Higher priority first, then earlier submission time
        self.priority
            .weight()
            .cmp(&other.priority.weight())
            .then_with(|| other.submitted_at.cmp(&self.submitted_at))
    }
}

/// Task queue with capability-indexed priority queues for O(log n) claim.
pub struct TaskQueue {
    /// All tasks by ID (the source of truth).
    tasks: RwLock<HashMap<TaskId, Task>>,
    /// Priority queue for pending tasks (global ordering).
    queue: RwLock<BinaryHeap<PrioritizedTask>>,
    /// Capability index: maps capability to priority-sorted task IDs.
    /// This allows O(log n) claim without rebuilding the entire heap.
    capability_index: RwLock<BTreeMap<String, BinaryHeap<PrioritizedTask>>>,
}

impl TaskQueue {
    /// Create a new empty task queue.
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            queue: RwLock::new(BinaryHeap::new()),
            capability_index: RwLock::new(BTreeMap::new()),
        }
    }

    /// Submit a new task to the queue. Returns the task ID.
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

        // Insert into capability index for fast claim
        self.capability_index
            .write()
            .entry(task.required_capability.clone())
            .or_default()
            .push(prioritized.clone());

        self.tasks.write().insert(task_id.clone(), task);
        self.queue.write().push(prioritized);
        task_id
    }

    /// Claim the highest-priority task matching a capability.
    ///
    /// Returns the task if one was available and claimed.
    /// This is now O(log n) per capability instead of O(n) for the entire queue.
    pub fn claim(&self, agent_id: &str, capability: &str) -> Option<Task> {
        let mut index = self.capability_index.write();
        let mut tasks = self.tasks.write();

        // Get the priority queue for this capability
        let queue = index.get_mut(capability)?;

        // Find the highest-priority available task
        while let Some(candidate) = queue.pop() {
            if let Some(task) = tasks.get_mut(&candidate.task_id) {
                // Skip expired or already-claimed tasks
                if task.status != TaskStatus::Queued {
                    continue;
                }
                if task.is_expired() {
                    task.status = TaskStatus::Expired;
                    continue;
                }

                // Found a valid task - claim it
                task.status = TaskStatus::Running;
                task.claimed_by = Some(agent_id.to_string());
                task.claimed_at = Some(Utc::now());
                task.attempts += 1;
                debug!(task_id = %task.id, agent = %agent_id, "Task claimed");
                return Some(task.clone());
            }
            // Task doesn't exist anymore, continue to next
        }

        None
    }

    /// Complete a task successfully.
    pub fn complete(&self, task_id: &str, result: impl Into<String>) -> bool {
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status != TaskStatus::Running {
                return false;
            }
            task.status = TaskStatus::Completed;
            task.result = Some(result.into());
            task.finished_at = Some(Utc::now());
            info!(task_id = %task_id, "Task completed");
            true
        } else {
            false
        }
    }

    /// Fail a task. If retries remain, re-queues it automatically.
    pub fn fail(&self, task_id: &str, error: impl Into<String>) -> bool {
        let error_str = error.into();
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status != TaskStatus::Running {
                return false;
            }

            if task.can_retry() {
                // Re-queue for retry
                task.status = TaskStatus::Queued;
                task.claimed_by = None;
                task.claimed_at = None;
                task.error = Some(error_str);
                warn!(task_id = %task_id, attempts = task.attempts, "Task failed, re-queuing");

                let prioritized = PrioritizedTask {
                    task_id: task_id.to_string(),
                    priority: task.priority,
                    submitted_at: task.submitted_at,
                };

                // Re-insert into capability index
                self.capability_index
                    .write()
                    .entry(task.required_capability.clone())
                    .or_default()
                    .push(prioritized.clone());

                self.queue.write().push(prioritized);
            } else {
                task.status = TaskStatus::Failed;
                task.error = Some(error_str);
                task.finished_at = Some(Utc::now());
                warn!(task_id = %task_id, "Task failed permanently (retries exhausted)");
            }
            true
        } else {
            false
        }
    }

    /// Cancel a task.
    pub fn cancel(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Completed || task.status == TaskStatus::Cancelled {
                return false;
            }
            task.status = TaskStatus::Cancelled;
            task.finished_at = Some(Utc::now());
            info!(task_id = %task_id, "Task cancelled");
            true
        } else {
            false
        }
    }

    /// Get a task by ID.
    pub fn get(&self, task_id: &str) -> Option<Task> {
        self.tasks.read().get(task_id).cloned()
    }

    /// Get the number of queued (pending) tasks.
    pub fn pending_count(&self) -> usize {
        self.tasks
            .read()
            .values()
            .filter(|t| t.status == TaskStatus::Queued)
            .count()
    }

    /// Get the number of running tasks.
    pub fn running_count(&self) -> usize {
        self.tasks
            .read()
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .count()
    }

    /// Get total task count.
    pub fn total_count(&self) -> usize {
        self.tasks.read().len()
    }

    /// Get tasks by status.
    pub fn by_status(&self, status: TaskStatus) -> Vec<Task> {
        self.tasks
            .read()
            .values()
            .filter(|t| t.status == status)
            .cloned()
            .collect()
    }

    /// Get tasks claimed by a specific agent.
    pub fn by_agent(&self, agent_id: &str) -> Vec<Task> {
        self.tasks
            .read()
            .values()
            .filter(|t| t.claimed_by.as_deref() == Some(agent_id))
            .cloned()
            .collect()
    }

    /// Expire overdue tasks. Returns count of expired tasks.
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

    /// Get a summary of task statuses.
    pub fn status_summary(&self) -> HashMap<String, usize> {
        let mut summary = HashMap::new();
        for task in self.tasks.read().values() {
            *summary.entry(format!("{:?}", task.status)).or_insert(0) += 1;
        }
        summary
    }

    /// Remove completed/failed/cancelled tasks older than the given age.
    /// Returns the number of tasks purged.
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

/// Thread-safe handle to the task queue.
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

    pub fn submit(&self, task: Task) -> TaskId {
        self.inner.submit(task)
    }

    pub fn claim(&self, agent_id: &str, capability: &str) -> Option<Task> {
        self.inner.claim(agent_id, capability)
    }

    pub fn complete(&self, task_id: &str, result: impl Into<String>) -> bool {
        self.inner.complete(task_id, result)
    }

    pub fn fail(&self, task_id: &str, error: impl Into<String>) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_and_claim() {
        let queue = TaskQueue::new();
        let task = Task::new("Test task", "Do something", "code_review", "user");
        let task_id = queue.submit(task);

        let claimed = queue.claim("agent-1", "code_review");
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed.id, task_id);
        assert_eq!(claimed.status, TaskStatus::Running);
    }

    #[test]
    fn claim_wrong_capability_returns_none() {
        let queue = TaskQueue::new();
        queue.submit(Task::new("Test", "Do", "code_review", "user"));

        let claimed = queue.claim("agent-1", "summarization");
        assert!(claimed.is_none());
    }

    #[test]
    fn priority_ordering() {
        let queue = TaskQueue::new();
        queue.submit(Task::new("Low", "low task", "cap", "user").with_priority(TaskPriority::Low));
        queue.submit(
            Task::new("Critical", "critical task", "cap", "user")
                .with_priority(TaskPriority::Critical),
        );
        queue.submit(
            Task::new("Normal", "normal task", "cap", "user").with_priority(TaskPriority::Normal),
        );

        let first = queue.claim("a1", "cap").unwrap();
        assert!(first.description.contains("Critical"));

        let second = queue.claim("a2", "cap").unwrap();
        assert!(second.description.contains("Normal"));

        let third = queue.claim("a3", "cap").unwrap();
        assert!(third.description.contains("Low"));
    }

    #[test]
    fn complete_task() {
        let queue = TaskQueue::new();
        let task_id = queue.submit(Task::new("Test", "Do", "cap", "user"));
        queue.claim("agent-1", "cap");

        assert!(queue.complete(&task_id, "done"));
        let task = queue.get(&task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(task.result.as_deref(), Some("done"));
    }

    #[test]
    fn fail_and_retry() {
        let queue = TaskQueue::new();
        let task_id = queue.submit(Task::new("Test", "Do", "cap", "user").with_max_retries(1));
        queue.claim("agent-1", "cap");

        assert!(queue.fail(&task_id, "oops"));

        // Should be re-queued
        let task = queue.get(&task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.attempts, 1);

        // Claim again
        let reclaimed = queue.claim("agent-2", "cap");
        assert!(reclaimed.is_some());

        // Fail again — retries exhausted
        assert!(queue.fail(&task_id, "oops again"));
        let task = queue.get(&task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Failed);
    }

    #[test]
    fn cancel_task() {
        let queue = TaskQueue::new();
        let task_id = queue.submit(Task::new("Test", "Do", "cap", "user"));
        assert!(queue.cancel(&task_id));
        assert_eq!(queue.get(&task_id).unwrap().status, TaskStatus::Cancelled);
    }

    #[test]
    fn status_summary() {
        let queue = TaskQueue::new();
        queue.submit(Task::new("T1", "D1", "cap", "user"));
        queue.submit(Task::new("T2", "D2", "cap", "user"));
        queue.claim("a1", "cap");

        let summary = queue.status_summary();
        assert_eq!(summary.get("Queued"), Some(&1));
        assert_eq!(summary.get("Running"), Some(&1));
    }

    #[test]
    fn by_agent() {
        let queue = TaskQueue::new();
        queue.submit(Task::new("T1", "D1", "cap", "user"));
        queue.submit(Task::new("T2", "D2", "cap", "user"));
        queue.claim("a1", "cap");

        let tasks = queue.by_agent("a1");
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn handle_operations() {
        let handle = TaskQueueHandle::new(TaskQueue::new());
        let task_id = handle.submit(Task::new("T1", "D1", "cap", "user"));
        assert_eq!(handle.pending_count(), 1);

        handle.claim("a1", "cap");
        assert_eq!(handle.running_count(), 1);

        assert!(handle.complete(&task_id, "result"));
    }
}
