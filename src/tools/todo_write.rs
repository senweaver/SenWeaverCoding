// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
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

pub const DEFAULT_SESSION_KEY: &str = "default";

pub type TodoStore = Arc<RwLock<HashMap<String, Vec<TodoItem>>>>;

pub fn new_todo_store() -> TodoStore {
    Arc::new(RwLock::new(HashMap::new()))
}

pub fn session_todos(store: &TodoStore, session_id: &str) -> Vec<TodoItem> {
    store
        .read()
        .get(session_id)
        .cloned()
        .unwrap_or_default()
}

pub fn replace_session_todos(store: &TodoStore, session_id: &str, todos: Vec<TodoItem>) {
    let mut guard = store.write();
    if todos.is_empty() {
        guard.remove(session_id);
    } else {
        guard.insert(session_id.to_string(), todos);
    }
}

pub fn clear_session(store: &TodoStore, session_id: &str) {
    store.write().remove(session_id);
}

pub fn session_ids(store: &TodoStore) -> Vec<String> {
    store.read().keys().cloned().collect()
}

pub fn snapshot_all(store: &TodoStore) -> HashMap<String, Vec<TodoItem>> {
    store.read().clone()
}

fn resolve_session_id(explicit: Option<&str>) -> String {
    if let Some(id) = explicit {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(ctx) = crate::session::current_session_context() {
        let trimmed = ctx.session_id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_SESSION_KEY.to_string()
}

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
    #[serde(deserialize_with = "deserialize_todo_id")]
    id: String,
    #[serde(default)]
    content: Option<String>,
    status: String,
    #[serde(default)]
    priority: Option<String>,
}

fn deserialize_todo_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "todo id must be a string or number, got: {other}"
        ))),
    }
}

struct TodoPatch {
    id: String,
    content: Option<String>,
    status: TodoStatus,
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

fn args_to_patches(todos: &[serde_json::Value]) -> anyhow::Result<Vec<TodoPatch>> {
    let mut out = Vec::with_capacity(todos.len());
    for raw in todos {
        let arg: TodoArg = serde_json::from_value(raw.clone())?;
        let status = parse_status(arg.status.trim())?;
        out.push(TodoPatch {
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

        "Use this tool to plan and track work for a multi-step user request. \
You MUST use it whenever the request needs more than one tool call or \
involves more than one logical step  -  registering the steps up front \
keeps your work transparent and recoverable across turns.\n\
\n\
WHEN TO USE\n\
- Multi-step coding tasks (search → read → edit → verify).\n\
- Investigations that touch several files or systems.\n\
- Any user request that says \"do X then Y then Z\".\n\
- Refactors, migrations, or cleanups with more than one target.\n\
- Even in Ask / Plan modes: list the analysis steps you intend to take.\n\
\n\
WHEN NOT TO USE\n\
- Single trivial edits (\"add a comment to this function\").\n\
- Pure conversational replies, jokes, or one-line answers.\n\
- Quick lookups answerable from one tool call.\n\
\n\
LIFECYCLE RULES\n\
- Mark exactly one todo as `in_progress` at a time, complete it, then \
move on. Don't leave todos `in_progress` indefinitely.\n\
- **Update statuses incrementally, never silently in bulk at the end.** \
Flip a step to `completed` *immediately* after finishing it, BEFORE you \
start the next one. The user's task bar mirrors the latest call  -  if \
you do five steps and only update once at the end, the bar is stuck at \
0/5 for the whole turn.\n\
- **If a single turn finishes multiple items**, update **all** of them \
in one call (e.g. step 1 → completed, step 2 → completed, step 3 → \
in_progress together). Don't skip an update just because more than one \
status moved.\n\
- **Pass `merge: true`** when only changing a few statuses so the rest \
of the list survives. Pass `merge: false` only when the plan has \
fundamentally changed shape.\n\
- Cancel todos that are no longer needed (`status:\"cancelled\"`) \
instead of silently dropping them.\n\
\n\
EXAMPLES\n\
1. User: \"Refactor user auth to use JWT.\"\n\
   First call: todos = [\n\
     {id:1, content:\"Audit current auth flow\", status:\"in_progress\"},\n\
     {id:2, content:\"Implement JWT issue/verify\", status:\"pending\"},\n\
     {id:3, content:\"Update tests / docs\", status:\"pending\"},\n\
   ]\n\
2. User: \"What does the agent loop do?\" → DO NOT call this tool; reply directly.\n\
3. Mid-task progress (step 1 done, starting step 2): re-call with \
merge:true, todos = [{id:1, status:\"completed\"}, {id:2, \
status:\"in_progress\"}]. Don't wait until the end of the turn.\n\
4. Single turn finished steps 1, 2, and 3 of 5: re-call with merge:true, \
todos = [{id:1, status:\"completed\"}, {id:2, status:\"completed\"}, \
{id:3, status:\"completed\"}, {id:4, status:\"in_progress\"}]."
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
                                "description": "Unique ID for this todo (string or number accepted)"
                            },
                            "content": {
                                "type": "string",
                                "description": "Description of the task. Required when creating a todo; optional on merge:true status-only updates of existing todos"
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
                        "required": ["id", "status"]
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

        let incoming = args_to_patches(todos_val)?;

        let merge = args.get("merge").and_then(|v| v.as_bool()).unwrap_or(false);

        let session_id_arg = args
            .get("session_id")
            .or_else(|| args.get("list_id"))
            .and_then(|v| v.as_str());
        let session_id = resolve_session_id(session_id_arg);

        let mut guard = self.store.write();
        let existing = guard.get(&session_id).cloned().unwrap_or_default();
        let old_count = existing.len();

        let open_count = existing
            .iter()
            .filter(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
            .count();

        let normalized = !merge && open_count > 0;
        let effective_merge = merge || normalized;

        let next = if effective_merge {
            let mut base = existing;
            for patch in incoming {
                if let Some(pos) = base.iter().position(|t| t.id == patch.id) {
                    base[pos].status = patch.status;
                    if let Some(content) = patch.content {
                        base[pos].content = content;
                    }
                    if patch.priority.is_some() {
                        base[pos].priority = patch.priority;
                    }
                } else {
                    let Some(content) = patch.content else {
                        drop(guard);
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "todo '{}' does not exist in the current list, so 'content' is required to create it",
                                patch.id
                            )),
                        });
                    };
                    base.push(TodoItem {
                        id: patch.id,
                        content,
                        status: patch.status,
                        priority: patch.priority,
                    });
                }
            }
            base
        } else {
            let mut items = Vec::with_capacity(incoming.len());
            for patch in incoming {
                let Some(content) = patch.content else {
                    drop(guard);
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "'content' is required for every todo when creating a new list (missing for id '{}')",
                            patch.id
                        )),
                    });
                };
                items.push(TodoItem {
                    id: patch.id,
                    content,
                    status: patch.status,
                    priority: patch.priority,
                });
            }
            items
        };

        let new_count = next.len();
        if next.is_empty() {
            guard.remove(&session_id);
        } else {
            guard.insert(session_id.clone(), next);
        }
        drop(guard);

        let mut payload = json!({
            "old_count": old_count,
            "new_count": new_count,
            "session_id": session_id,
        });
        if normalized {
            payload["normalized"] = json!(true);
            payload["note"] = json!(format!(
                "The current list still had {open_count} unfinished item(s), so this full-replace \
                 was merged into the existing list instead of recreating it, preserving in-progress \
                 work. To adjust the plan, call todo_write with merge:true to update or append \
                 items; only start a brand-new list (merge:false) once every item is completed or \
                 cancelled."
            ));
        }

        Ok(ToolResult {
            success: true,
            output: payload.to_string(),
            error: None,
        })
    }
}
