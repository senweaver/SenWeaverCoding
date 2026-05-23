// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type TaskId = String;

pub type A2aAgentId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCard {

    pub name: String,

    pub id: A2aAgentId,

    pub description: String,

    pub url: String,

    pub provider: Option<String>,

    pub capabilities: AgentCapabilities,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AgentAuth>,
}

impl AgentCard {

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentCapabilities {

    pub streaming: bool,

    pub push_notifications: bool,

    pub state_transition_history: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum AgentAuth {

    OAuth2 {
        client_registration_url: String,
        scopes: Vec<String>,
    },

    ApiKey { location: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {

    Submitted,

    Working,

    Completed,

    Failed,

    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aTask {

    pub id: TaskId,

    pub name: String,

    pub description: String,

    pub status: TaskStatus,

    pub created_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl A2aTask {

    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "task-{}-{}",
                now.timestamp_millis(),
                &uuid::Uuid::new_v4().to_string()[..8]
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

    pub fn mark_working(&mut self) {
        self.status = TaskStatus::Working;
    }

    pub fn mark_completed(&mut self, result: TaskResult) {
        self.status = TaskStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.result = Some(result);
    }

    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = TaskStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error.into());
    }

    pub fn mark_cancelled(&mut self) {
        self.status = TaskStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskResult {

    Text { text: String },

    Data { data: serde_json::Value },

    File { url: String, mime_type: String },

    Multi { results: Vec<TaskResult> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SendTaskRequest {

    pub name: String,

    pub description: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl SendTaskRequest {

    pub fn into_task(self) -> A2aTask {
        A2aTask::new(self.name, self.description)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SendTaskResponse {

    pub task: A2aTask,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_completion_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CancelTaskRequest {

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CancelTaskResponse {

    pub task: A2aTask,

    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListAgentsResponse {

    pub agents: Vec<AgentCard>,

    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoverAgentRequest {

    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A2aError {

    pub code: String,

    pub message: String,
}

impl A2aError {

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn task_not_found(task_id: &str) -> Self {
        Self::new("TASK_NOT_FOUND", format!("Task '{}' not found", task_id))
    }

    pub fn agent_not_found(agent_id: &str) -> Self {
        Self::new("AGENT_NOT_FOUND", format!("Agent '{}' not found", agent_id))
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("INVALID_REQUEST", message)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", message)
    }
}

#[derive(Debug, Clone, Default)]
pub struct A2aTaskStore {
    tasks: std::collections::HashMap<TaskId, A2aTask>,
}

impl A2aTaskStore {

    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(&mut self, task: A2aTask) {
        self.tasks.insert(task.id.clone(), task);
    }

    pub fn get(&self, id: &TaskId) -> Option<&A2aTask> {
        self.tasks.get(id)
    }

    pub fn get_mut(&mut self, id: &TaskId) -> Option<&mut A2aTask> {
        self.tasks.get_mut(id)
    }

    pub fn update(&mut self, task: A2aTask) {
        self.tasks.insert(task.id.clone(), task);
    }

    pub fn remove(&mut self, id: &TaskId) -> Option<A2aTask> {
        self.tasks.remove(id)
    }

    pub fn list_all(&self) -> Vec<&A2aTask> {
        self.tasks.values().collect()
    }

    pub fn list_by_status(&self, status: TaskStatus) -> Vec<&A2aTask> {
        self.tasks.values().filter(|t| t.status == status).collect()
    }

    pub fn count(&self) -> usize {
        self.tasks.len()
    }

    pub fn cleanup_old(&mut self, max_age: chrono::Duration) {
        let cutoff = Utc::now() - max_age;
        self.tasks.retain(|_, task| {
            !task.is_terminal() || task.completed_at.map_or(true, |t| t > cutoff)
        });
    }
}
