// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// A single step in a task plan managed by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub status: PlanStepStatus,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
}

impl std::fmt::Display for PlanStepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

/// Shared handle to the current plan steps.
pub type PlanHandle = Arc<RwLock<Vec<PlanStep>>>;

/// Lets the LLM create, update, and query a structured task plan.
///
/// Complements `TodoWriteTool` (user-visible tasks) by providing internal
/// planning that the LLM uses to track its own multi-step work.
pub struct UpdatePlanTool {
    plan: PlanHandle,
}

impl UpdatePlanTool {
    pub fn new(plan: PlanHandle) -> Self {
        Self { plan }
    }
}

#[async_trait]
impl Tool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }

    fn description(&self) -> &str {
        "Create, update, or query a structured task plan. Use 'set' to replace \
         the entire plan, 'update' to change a step's status, 'get' to view \
         the current plan. This helps track multi-step work systematically."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action: 'set' (replace plan), 'update' (modify a step), 'get' (view plan)",
                    "enum": ["set", "update", "get"]
                },
                "steps": {
                    "type": "array",
                    "description": "Full plan steps (for 'set' action)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "title": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "skipped"]
                            },
                            "notes": { "type": "string" }
                        },
                        "required": ["id", "title"]
                    }
                },
                "step_id": {
                    "type": "string",
                    "description": "Step ID to update (for 'update' action)"
                },
                "status": {
                    "type": "string",
                    "description": "New status for the step (for 'update' action)",
                    "enum": ["pending", "in_progress", "completed", "skipped"]
                },
                "notes": {
                    "type": "string",
                    "description": "Optional notes to attach to the step"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        match action {
            "set" => {
                let steps_val = args
                    .get("steps")
                    .ok_or_else(|| anyhow::anyhow!("'set' requires 'steps' array"))?;
                let steps: Vec<PlanStep> = serde_json::from_value(steps_val.clone())?;
                let count = steps.len();
                *self.plan.write() = steps;
                Ok(ToolResult {
                    success: true,
                    output: format!("Plan set with {count} steps"),
                    error: None,
                })
            }
            "update" => {
                let step_id = args
                    .get("step_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("'update' requires 'step_id'"))?;

                let mut plan = self.plan.write();
                let step = plan.iter_mut().find(|s| s.id == step_id);

                match step {
                    Some(s) => {
                        if let Some(status_str) = args.get("status").and_then(|v| v.as_str()) {
                            s.status = match status_str {
                                "in_progress" => PlanStepStatus::InProgress,
                                "completed" => PlanStepStatus::Completed,
                                "skipped" => PlanStepStatus::Skipped,
                                _ => PlanStepStatus::Pending,
                            };
                        }
                        if let Some(notes) = args.get("notes").and_then(|v| v.as_str()) {
                            s.notes = Some(notes.to_string());
                        }
                        Ok(ToolResult {
                            success: true,
                            output: format!(
                                "Updated step '{}': status={}",
                                s.title, s.status
                            ),
                            error: None,
                        })
                    }
                    None => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Step '{step_id}' not found in plan")),
                    }),
                }
            }
            "get" => {
                let plan = self.plan.read();
                if plan.is_empty() {
                    return Ok(ToolResult {
                        success: true,
                        output: "No plan set. Use action='set' to create one.".to_string(),
                        error: None,
                    });
                }

                let lines: Vec<String> = plan
                    .iter()
                    .map(|s| {
                        let icon = match s.status {
                            PlanStepStatus::Pending => "⬜",
                            PlanStepStatus::InProgress => "🔄",
                            PlanStepStatus::Completed => "✅",
                            PlanStepStatus::Skipped => "⏭️",
                        };
                        let notes = s
                            .notes
                            .as_deref()
                            .map(|n| format!(" — {n}"))
                            .unwrap_or_default();
                        format!("{icon} [{}] {}{}", s.id, s.title, notes)
                    })
                    .collect();

                let completed = plan
                    .iter()
                    .filter(|s| s.status == PlanStepStatus::Completed)
                    .count();

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Plan ({}/{} completed):\n{}",
                        completed,
                        plan.len(),
                        lines.join("\n")
                    ),
                    error: None,
                })
            }
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{other}'. Use 'set', 'update', or 'get'."
                )),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> UpdatePlanTool {
        UpdatePlanTool::new(Arc::new(RwLock::new(Vec::new())))
    }

    #[tokio::test]
    async fn set_and_get() {
        let tool = make_tool();
        let set_result = tool
            .execute(json!({
                "action": "set",
                "steps": [
                    {"id": "s1", "title": "Step 1"},
                    {"id": "s2", "title": "Step 2"}
                ]
            }))
            .await
            .unwrap();
        assert!(set_result.success);
        assert!(set_result.output.contains("2 steps"));

        let get_result = tool.execute(json!({"action": "get"})).await.unwrap();
        assert!(get_result.success);
        assert!(get_result.output.contains("Step 1"));
        assert!(get_result.output.contains("Step 2"));
    }

    #[tokio::test]
    async fn update_step() {
        let tool = make_tool();
        tool.execute(json!({
            "action": "set",
            "steps": [{"id": "s1", "title": "Do thing"}]
        }))
        .await
        .unwrap();

        let update_result = tool
            .execute(json!({
                "action": "update",
                "step_id": "s1",
                "status": "completed"
            }))
            .await
            .unwrap();
        assert!(update_result.success);
        assert!(update_result.output.contains("completed"));
    }

    #[test]
    fn tool_name_correct() {
        assert_eq!(make_tool().name(), "update_plan");
    }
}
