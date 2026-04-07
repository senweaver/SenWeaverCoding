// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Plan Mode - structured planning and task tracking for agent sessions.
//!
//! When plan mode is active, the agent generates structured task lists
//! before executing, providing visibility into multi-step operations.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Plan mode configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanModeConfig {
    /// Enable plan mode by default. Default: false.
    #[serde(default)]
    pub enabled: bool,
    /// Auto-activate plan mode for complex queries (> threshold tools). Default: 3.
    #[serde(default = "default_auto_threshold")]
    pub auto_activate_threshold: usize,
    /// Maximum todos per plan. Default: 20.
    #[serde(default = "default_max_todos")]
    pub max_todos: usize,
}

fn default_auto_threshold() -> usize {
    3
}
fn default_max_todos() -> usize {
    20
}

impl Default for PlanModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_activate_threshold: default_auto_threshold(),
            max_todos: default_max_todos(),
        }
    }
}

/// Status of a todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Failed,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Skipped => write!(f, "skipped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A single todo item in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TodoStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tool_hint: Option<String>,
    pub result: Option<String>,
}

impl TodoItem {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            status: TodoStatus::Pending,
            created_at: now,
            updated_at: now,
            tool_hint: None,
            result: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_tool_hint(mut self, hint: impl Into<String>) -> Self {
        self.tool_hint = Some(hint.into());
        self
    }

    pub fn mark_in_progress(&mut self) {
        self.status = TodoStatus::InProgress;
        self.updated_at = Utc::now();
    }

    pub fn mark_completed(&mut self, result: Option<String>) {
        self.status = TodoStatus::Completed;
        self.result = result;
        self.updated_at = Utc::now();
    }

    pub fn mark_failed(&mut self, reason: Option<String>) {
        self.status = TodoStatus::Failed;
        self.result = reason;
        self.updated_at = Utc::now();
    }

    pub fn mark_skipped(&mut self) {
        self.status = TodoStatus::Skipped;
        self.updated_at = Utc::now();
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TodoStatus::Completed | TodoStatus::Failed | TodoStatus::Skipped
        )
    }
}

/// A plan consisting of ordered todo items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub todos: Vec<TodoItem>,
    pub created_at: DateTime<Utc>,
    pub is_active: bool,
}

impl Plan {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: format!("plan-{}", Utc::now().timestamp_millis()),
            title: title.into(),
            todos: Vec::new(),
            created_at: Utc::now(),
            is_active: true,
        }
    }

    pub fn add_todo(&mut self, todo: TodoItem) {
        self.todos.push(todo);
    }

    pub fn progress_summary(&self) -> String {
        let total = self.todos.len();
        let done = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let failed = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::Failed)
            .count();
        let in_progress = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        format!("{done}/{total} completed, {in_progress} in progress, {failed} failed")
    }

    pub fn next_pending(&self) -> Option<&TodoItem> {
        self.todos.iter().find(|t| t.status == TodoStatus::Pending)
    }

    pub fn is_complete(&self) -> bool {
        self.todos.iter().all(|t| t.is_terminal())
    }

    pub fn get_todo_mut(&mut self, id: &str) -> Option<&mut TodoItem> {
        self.todos.iter_mut().find(|t| t.id == id)
    }

    pub fn to_markdown(&self) -> String {
        let mut out = format!("## Plan: {}\n\n", self.title);
        out.push_str(&format!("Progress: {}\n\n", self.progress_summary()));
        for (i, todo) in self.todos.iter().enumerate() {
            let icon = match todo.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[~]",
                TodoStatus::Completed => "[x]",
                TodoStatus::Skipped => "[-]",
                TodoStatus::Failed => "[!]",
            };
            out.push_str(&format!("{}. {} {}\n", i + 1, icon, todo.title));
            if let Some(ref desc) = todo.description {
                out.push_str(&format!("   {}\n", desc));
            }
        }
        out
    }
}

/// Plan mode state tracker for a session.
pub struct PlanModeState {
    config: PlanModeConfig,
    active_plan: Option<Plan>,
    completed_plans: Vec<Plan>,
}

impl PlanModeState {
    pub fn new(config: PlanModeConfig) -> Self {
        Self {
            config,
            active_plan: None,
            completed_plans: Vec::new(),
        }
    }

    pub fn is_plan_mode(&self) -> bool {
        self.config.enabled || self.active_plan.is_some()
    }

    pub fn start_plan(&mut self, title: impl Into<String>) -> &Plan {
        if let Some(mut old) = self.active_plan.take() {
            old.is_active = false;
            self.completed_plans.push(old);
        }
        self.active_plan = Some(Plan::new(title));
        self.active_plan.as_ref().unwrap()
    }

    pub fn active_plan(&self) -> Option<&Plan> {
        self.active_plan.as_ref()
    }

    pub fn active_plan_mut(&mut self) -> Option<&mut Plan> {
        self.active_plan.as_mut()
    }

    pub fn finish_plan(&mut self) -> Option<Plan> {
        if let Some(mut plan) = self.active_plan.take() {
            plan.is_active = false;
            self.completed_plans.push(plan.clone());
            Some(plan)
        } else {
            None
        }
    }

    pub fn plan_prompt_injection(&self) -> Option<String> {
        self.active_plan.as_ref().map(|p| {
            format!(
                "\n\n## Active Plan\n\n{}\n\nUpdate todo status as you work through the plan. \
                 Mark items [x] completed, [!] failed, or [-] skipped.",
                p.to_markdown()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_lifecycle() {
        let mut state = PlanModeState::new(PlanModeConfig::default());
        state.start_plan("Test Plan");

        let plan = state.active_plan_mut().unwrap();
        plan.add_todo(TodoItem::new("1", "First task"));
        plan.add_todo(TodoItem::new("2", "Second task"));

        assert_eq!(
            plan.progress_summary(),
            "0/2 completed, 0 in progress, 0 failed"
        );

        plan.get_todo_mut("1").unwrap().mark_in_progress();
        assert_eq!(
            plan.progress_summary(),
            "0/2 completed, 1 in progress, 0 failed"
        );

        plan.get_todo_mut("1")
            .unwrap()
            .mark_completed(Some("Done".to_string()));
        assert_eq!(
            plan.progress_summary(),
            "1/2 completed, 0 in progress, 0 failed"
        );
        assert!(!plan.is_complete());

        plan.get_todo_mut("2").unwrap().mark_completed(None);
        assert!(plan.is_complete());
    }

    #[test]
    fn test_plan_markdown() {
        let mut plan = Plan::new("Build Feature");
        plan.add_todo(TodoItem::new("1", "Design API"));
        let mut t = TodoItem::new("2", "Implement");
        t.mark_completed(None);
        plan.add_todo(t);

        let md = plan.to_markdown();
        assert!(md.contains("[ ] Design API"));
        assert!(md.contains("[x] Implement"));
    }

    #[test]
    fn test_prompt_injection() {
        let mut state = PlanModeState::new(PlanModeConfig {
            enabled: true,
            ..Default::default()
        });
        state.start_plan("My Plan");
        state
            .active_plan_mut()
            .unwrap()
            .add_todo(TodoItem::new("1", "Task A"));

        let injection = state.plan_prompt_injection().unwrap();
        assert!(injection.contains("Active Plan"));
        assert!(injection.contains("Task A"));
    }
}
