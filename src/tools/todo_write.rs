// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    #[serde(default)]
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

pub type TodoStore = Arc<RwLock<Vec<TodoItem>>>;

pub struct TodoWriteTool {
    store: TodoStore,
}

impl TodoWriteTool {
    pub fn new(store: TodoStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &TodoStore {
        &self.store
    }
}

#[derive(Deserialize)]
struct TodoArg {
    id: String,
    content: String,
    status: String,
    #[serde(default)]
    priority: Option<String>,
}

fn parse_status(s: &str) -> anyhow::Result<TodoStatus> {
    match s {
        "pending" => Ok(TodoStatus::Pending),
        "in_progress" => Ok(TodoStatus::InProgress),
        "completed" => Ok(TodoStatus::Completed),
        "cancelled" => Ok(TodoStatus::Cancelled),
        _ => anyhow::bail!(
            "Invalid status '{s}': expected pending, in_progress, completed, or cancelled"
        ),
    }
}

fn args_to_items(todos: &[serde_json::Value]) -> anyhow::Result<Vec<TodoItem>> {
    let mut out = Vec::with_capacity(todos.len());
    for raw in todos {
        let arg: TodoArg = serde_json::from_value(raw.clone())?;
        let status = parse_status(arg.status.trim())?;
        out.push(TodoItem {
            id: arg.id,
            content: arg.content,
            status,
            priority: arg.priority,
        });
    }
    Ok(out)
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }

    fn description(&self) -> &str {
        "Create and manage a structured task list for the current session. Use to track progress on complex multi-step tasks."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "Array of todo items for this session.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique ID for this todo"
                            },
                            "content": {
                                "type": "string",
                                "description": "Description of the task"
                            },
                            "status": {
                                "type": "string",
                                "description": "Lifecycle status",
                                "enum": ["pending", "in_progress", "completed", "cancelled"]
                            },
                            "priority": {
                                "type": "string",
                                "description": "Optional priority level"
                            }
                        },
                        "required": ["id", "content", "status"]
                    }
                },
                "merge": {
                    "type": "boolean",
                    "description": "If true, merge with existing todos by id; if false, replace the entire list",
                    "default": false
                }
            },
            "required": ["todos"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let todos_val = args
            .get("todos")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'todos' array"))?;

        let incoming = args_to_items(todos_val)?;

        let merge = args.get("merge").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut guard = self.store.write();
        let old_count = guard.len();

        let mut next = if merge {
            let mut base = guard.clone();
            for item in incoming {
                if let Some(pos) = base.iter().position(|t| t.id == item.id) {
                    base[pos] = item;
                } else {
                    base.push(item);
                }
            }
            base
        } else {
            incoming
        };

        if !next.is_empty() && next.iter().all(|t| t.status == TodoStatus::Completed) {
            next.clear();
        }

        let new_count = next.len();
        *guard = next;
        drop(guard);

        Ok(ToolResult {
            success: true,
            output: json!({
                "old_count": old_count,
                "new_count": new_count,
            })
            .to_string(),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool(store: TodoStore) -> TodoWriteTool {
        TodoWriteTool::new(store)
    }

    #[test]
    fn name_matches() {
        let store = Arc::new(RwLock::new(Vec::new()));
        let tool = make_tool(store);
        assert_eq!(tool.name(), "todo_write");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn replace_todos() {
        let store = Arc::new(RwLock::new(vec![TodoItem {
            id: "a".into(),
            content: "old".into(),
            status: TodoStatus::Pending,
            priority: None,
        }]));
        let tool = make_tool(store.clone());

        let result = tool
            .execute(json!({
                "merge": false,
                "todos": [{
                    "id": "b",
                    "content": "new task",
                    "status": "in_progress"
                }]
            }))
            .await
            .unwrap();

        assert!(result.success);
        let out: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(out["old_count"], 1);
        assert_eq!(out["new_count"], 1);

        let items = store.read().clone();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "b");
        assert_eq!(items[0].status, TodoStatus::InProgress);
    }

    #[tokio::test]
    async fn merge_todos() {
        let store = Arc::new(RwLock::new(vec![
            TodoItem {
                id: "1".into(),
                content: "first".into(),
                status: TodoStatus::Pending,
                priority: None,
            },
            TodoItem {
                id: "2".into(),
                content: "second".into(),
                status: TodoStatus::Pending,
                priority: None,
            },
        ]));
        let tool = make_tool(store.clone());

        let result = tool
            .execute(json!({
                "merge": true,
                "todos": [
                    {
                        "id": "1",
                        "content": "first updated",
                        "status": "completed"
                    },
                    {
                        "id": "3",
                        "content": "third",
                        "status": "pending"
                    }
                ]
            }))
            .await
            .unwrap();

        assert!(result.success);
        let out: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(out["old_count"], 2);
        assert_eq!(out["new_count"], 3);

        let items = store.read().clone();
        let one = items.iter().find(|t| t.id == "1").unwrap();
        assert_eq!(one.content, "first updated");
        assert_eq!(one.status, TodoStatus::Completed);
        assert!(items.iter().any(|t| t.id == "3"));
    }

    #[tokio::test]
    async fn clears_when_all_completed() {
        let store = Arc::new(RwLock::new(vec![TodoItem {
            id: "x".into(),
            content: "done".into(),
            status: TodoStatus::InProgress,
            priority: Some("high".into()),
        }]));
        let tool = make_tool(store.clone());

        let result = tool
            .execute(json!({
                "merge": false,
                "todos": [{
                    "id": "x",
                    "content": "done",
                    "status": "completed"
                }]
            }))
            .await
            .unwrap();

        assert!(result.success);
        let out: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(out["old_count"], 1);
        assert_eq!(out["new_count"], 0);
        assert!(store.read().is_empty());
    }
}
