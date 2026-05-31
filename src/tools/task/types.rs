// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Running,
    Completed,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub task_type: String,
    pub description: String,
    pub state: TaskState,
    pub output: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub type TaskManagerHandle = Arc<RwLock<TaskManager>>;

pub struct TaskManager {
    tasks: HashMap<String, TaskInfo>,
    next_id: u64,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn create_task(&mut self, task_type: String, description: String) -> String {
        let id = format!("task-{}", self.next_id);
        self.next_id += 1;
        let now = chrono::Utc::now().to_rfc3339();
        self.tasks.insert(
            id.clone(),
            TaskInfo {
                id: id.clone(),
                task_type,
                description,
                state: TaskState::Pending,
                output: String::new(),
                error: None,
                created_at: now.clone(),
                updated_at: now,
            },
        );
        id
    }

    pub fn get_task(&self, id: &str) -> Option<&TaskInfo> {
        self.tasks.get(id)
    }

    pub fn update_task(
        &mut self,
        id: &str,
        state: Option<TaskState>,
        output: Option<String>,
        error: Option<String>,
    ) -> bool {
        if let Some(task) = self.tasks.get_mut(id) {
            if let Some(s) = state {
                task.state = s;
            }
            if let Some(o) = output {
                task.output = o;
            }
            if let Some(e) = error {
                task.error = Some(e);
            }
            task.updated_at = chrono::Utc::now().to_rfc3339();
            true
        } else {
            false
        }
    }

    pub fn list_tasks(&self) -> Vec<&TaskInfo> {
        self.tasks.values().collect()
    }

    pub fn stop_task(&mut self, id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(id) {
            if task.state == TaskState::Running || task.state == TaskState::Pending {
                task.state = TaskState::Stopped;
                task.updated_at = chrono::Utc::now().to_rfc3339();
                return true;
            }
        }
        false
    }

    pub fn get_output(&self, id: &str) -> Option<String> {
        self.tasks.get(id).map(|t| t.output.clone())
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}
