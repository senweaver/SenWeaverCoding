// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! A2A Protocol Types - Agent-to-Agent communication protocol.
//!
//! Provides standardized types for agent discovery, task submission,
//! and status tracking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for A2A tasks.
pub type TaskId = String;

/// Unique identifier for A2A agents.
pub type A2aAgentId = String;

/// Agent card  - public metadata about an agent's capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCard {
    /// Agent name (human-readable identifier).
    pub name: String,
    /// Unique agent identifier.
    pub id: A2aAgentId,
    /// Agent description.
    pub description: String,
    /// Base URL for agent endpoints.
    pub url: String,
    /// Provider/vendor information.
    pub provider: Option<String>,
    /// Capabilities this agent supports.
    pub capabilities: AgentCapabilities,
    /// Authentication requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AgentAuth>,
}

impl AgentCard {
    /// Create a new agent card.
    pub fn new(
        name: impl Into<String>,
        id: impl Into<String>,
        description: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            id: id.into(),
            description: description.into(),
            url: url.into(),
            provider: None,
            capabilities: AgentCapabilities::default(),
            auth: None,
        }
    }

    /// Build an agent card with the standard A2A discovery format.
    pub fn build_agent_card(
        name: impl Into<String>,
        url: impl Into<String>,
        _skills: Vec<String>,
    ) -> Self {
        let name = name.into();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut h);
            h.finish()
        };
        let id = format!("{:016x}-a2a-{:04x}", hash, name.len());
        Self {
            name: name.clone(),
            id: id.clone(),
            description: format!("A2A agent: {}", name),
            url: url.into(),
            provider: Some("SenWeaverCoding".to_string()),
            capabilities: AgentCapabilities {
                streaming: true,
                push_notifications: false,
                state_transition_history: true,
            },
            auth: None,
        }
    }
}

/// Agent capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentCapabilities {
    /// Supports streaming responses.
    pub streaming: bool,
    /// Supports push notifications.
    pub push_notifications: bool,
    /// Tracks state transitions.
    pub state_transition_history: bool,
}

/// Authentication requirements for an agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum AgentAuth {
    /// OAuth2 authentication.
    OAuth2 {
        client_registration_url: String,
        scopes: Vec<String>,
    },
    /// API key authentication.
    ApiKey { location: String },
}

/// A2A Task status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    /// Task submitted but not yet started.
    Submitted,
    /// Task is currently being worked on.
    Working,
    /// Task completed successfully.
    Completed,
    /// Task failed with an error.
    Failed,
    /// Task was cancelled.
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// An A2A Task  - work unit submitted to an agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aTask {
    /// Unique task identifier.
    pub id: TaskId,
    /// Human-readable task name.
    pub name: String,
    /// Task description/prompt.
    pub description: String,
    /// Current task status.
    pub status: TaskStatus,
    /// Task creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Task completion timestamp (if finished).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Task result (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,
    /// Error message (if failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl A2aTask {
    /// Create a new task with the given name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "task-{}-{}",
                now.timestamp_millis(),
                uuid::Uuid::new_v4().to_string()[..8].to_string()
            ),
            name: name.into(),
            description: description.into(),
            status: TaskStatus::Submitted,
            created_at: now,
            completed_at: None,
            result: None,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Mark the task as working.
    pub fn mark_working(&mut self) {
        self.status = TaskStatus::Working;
    }

    /// Mark the task as completed with a result.
    pub fn mark_completed(&mut self, result: TaskResult) {
        self.status = TaskStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.result = Some(result);
    }

    /// Mark the task as failed with an error.
    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = TaskStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error.into());
    }

    /// Mark the task as cancelled.
    pub fn mark_cancelled(&mut self) {
        self.status = TaskStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }

    /// Check if the task is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

/// Task result types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskResult {
    /// Text response.
    Text { text: String },
    /// JSON structured data.
    Data { data: serde_json::Value },
    /// File reference.
    File { url: String, mime_type: String },
    /// Multiple results.
    Multi { results: Vec<TaskResult> },
}

/// Request to send a task to an agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SendTaskRequest {
    /// Task name.
    pub name: String,
    /// Task description/prompt.
    pub description: String,
    /// Optional callback URL for status updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
    /// Request metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl SendTaskRequest {
    /// Convert this request into a task.
    pub fn into_task(self) -> A2aTask {
        A2aTask::new(self.name, self.description)
    }
}

/// Response from sending a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SendTaskResponse {
    /// The created task.
    pub task: A2aTask,
    /// Estimated time to completion (seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_completion_secs: Option<u64>,
}

/// Request to cancel a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CancelTaskRequest {
    /// Reason for cancellation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Response from cancelling a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CancelTaskResponse {
    /// The cancelled task.
    pub task: A2aTask,
    /// Whether cancellation was successful.
    pub success: bool,
}

/// List of agents response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListAgentsResponse {
    /// Available agents.
    pub agents: Vec<AgentCard>,
    /// Total count.
    pub total: usize,
}

/// Agent discovery request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoverAgentRequest {
    /// URL to discover agent at.
    pub url: String,
}

/// Standard error response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aError {
    /// Error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl A2aError {
    /// Create a new A2A error.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Task not found error.
    pub fn task_not_found(task_id: &str) -> Self {
        Self::new("TASK_NOT_FOUND", format!("Task '{}' not found", task_id))
    }

    /// Agent not found error.
    pub fn agent_not_found(agent_id: &str) -> Self {
        Self::new("AGENT_NOT_FOUND", format!("Agent '{}' not found", agent_id))
    }

    /// Invalid request error.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("INVALID_REQUEST", message)
    }

    /// Internal error.
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", message)
    }
}

/// A2A Task store for in-memory task tracking.
#[derive(Debug, Clone, Default)]
pub struct A2aTaskStore {
    tasks: std::collections::HashMap<TaskId, A2aTask>,
}

impl A2aTaskStore {
    /// Create a new empty task store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a task.
    pub fn store(&mut self, task: A2aTask) {
        self.tasks.insert(task.id.clone(), task);
    }

    /// Get a task by ID.
    pub fn get(&self, id: &TaskId) -> Option<&A2aTask> {
        self.tasks.get(id)
    }

    /// Get a mutable task by ID.
    pub fn get_mut(&mut self, id: &TaskId) -> Option<&mut A2aTask> {
        self.tasks.get_mut(id)
    }

    /// Update a task.
    pub fn update(&mut self, task: A2aTask) {
        self.tasks.insert(task.id.clone(), task);
    }

    /// Remove a task.
    pub fn remove(&mut self, id: &TaskId) -> Option<A2aTask> {
        self.tasks.remove(id)
    }

    /// List all tasks.
    pub fn list_all(&self) -> Vec<&A2aTask> {
        self.tasks.values().collect()
    }

    /// List tasks by status.
    pub fn list_by_status(&self, status: TaskStatus) -> Vec<&A2aTask> {
        self.tasks.values().filter(|t| t.status == status).collect()
    }

    /// Count of tasks in the store.
    pub fn count(&self) -> usize {
        self.tasks.len()
    }

    /// Clean up old completed tasks (older than max_age).
    pub fn cleanup_old(&mut self, max_age: chrono::Duration) {
        let cutoff = Utc::now() - max_age;
        self.tasks.retain(|_, task| {
            !task.is_terminal() || task.completed_at.map_or(true, |t| t > cutoff)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_card_creation() {
        let card = AgentCard::new(
            "Test Agent",
            "agent-123",
            "A test agent",
            "http://localhost:8080",
        );
        assert_eq!(card.name, "Test Agent");
        assert_eq!(card.id, "agent-123");
    }

    #[test]
    fn test_a2a_task_lifecycle() {
        let mut task = A2aTask::new("Test Task", "Do something");
        assert_eq!(task.status, TaskStatus::Submitted);

        task.mark_working();
        assert_eq!(task.status, TaskStatus::Working);
        assert!(!task.is_terminal());

        task.mark_completed(TaskResult::Text {
            text: "Done".to_string(),
        });
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.is_terminal());
        assert!(task.result.is_some());
    }

    #[test]
    fn test_task_store() {
        let mut store = A2aTaskStore::new();
        let task = A2aTask::new("Test", "Description");
        let task_id = task.id.clone();

        store.store(task);
        assert_eq!(store.count(), 1);

        let retrieved = store.get(&task_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test");
    }

    #[test]
    fn test_task_store_cleanup() {
        let mut store = A2aTaskStore::new();

        // Add completed task with old completion time
        let mut old_task = A2aTask::new("Old", "Old task");
        old_task.mark_completed(TaskResult::Text {
            text: "Done".to_string(),
        });
        // Simulate old task by manipulating timestamp
        old_task.completed_at = Some(Utc::now() - chrono::Duration::hours(25));
        store.store(old_task);

        // Add new task
        let new_task = A2aTask::new("New", "New task");
        store.store(new_task);

        assert_eq!(store.count(), 2);

        // Cleanup tasks older than 24 hours
        store.cleanup_old(chrono::Duration::hours(24));

        // Old task should be removed, new one stays
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_a2a_error() {
        let err = A2aError::task_not_found("task-123");
        assert_eq!(err.code, "TASK_NOT_FOUND");
        assert!(err.message.contains("task-123"));
    }

    #[test]
    fn test_task_result_types() {
        let text = TaskResult::Text {
            text: "Hello".to_string(),
        };
        let data = TaskResult::Data {
            data: serde_json::json!({"key": "value"}),
        };

        assert!(matches!(text, TaskResult::Text { .. }));
        assert!(matches!(data, TaskResult::Data { .. }));
    }
}
