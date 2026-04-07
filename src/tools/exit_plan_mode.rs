// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::enter_plan_mode::PlanModeFlag;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;

/// Shared pending plan content. Set by `exit_plan_mode` when a plan is produced.
/// The REPL loop checks this to offer auto-continue (Plan-to-Build).
pub type PendingPlan = Arc<RwLock<Option<String>>>;

pub fn new_pending_plan() -> PendingPlan {
    Arc::new(RwLock::new(None))
}

pub struct ExitPlanModeTool {
    flag: PlanModeFlag,
    pending_plan: PendingPlan,
}

impl ExitPlanModeTool {
    pub fn new(flag: PlanModeFlag) -> Self {
        Self {
            pending_plan: new_pending_plan(),
            flag,
        }
    }

    pub fn new_with_pending_plan(flag: PlanModeFlag, pending_plan: PendingPlan) -> Self {
        Self {
            flag,
            pending_plan,
        }
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "exit_plan_mode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and provide the plan content. Returns to normal mode where all tools are available."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "plan_content": {
                    "type": "string",
                    "description": "The plan that was created during plan mode",
                },
            },
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let plan_text = args
            .get("plan_content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        *self.flag.write() = false;

        // Store plan for auto-continue (Plan-to-Build)
        if !plan_text.is_empty() {
            *self.pending_plan.write() = Some(plan_text.to_string());
        }

        // Also switch CodingMode from Plan back to Vibe (skip in tests)
        if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
            let current = *svc.coding_mode.read();
            if current == crate::agent::coding_mode::CodingMode::Plan {
                *svc.coding_mode.write() = crate::agent::coding_mode::CodingMode::Vibe;
            }
        }

        let output = if plan_text.is_empty() {
            "Exited plan mode. All tools are now available. No plan content was provided."
                .to_string()
        } else {
            let len = plan_text.chars().count();
            let preview: String = plan_text.chars().take(500).collect();
            let truncated = len > 500;
            let body = if truncated {
                format!("{preview}...")
            } else {
                preview
            };
            format!(
                "Exited plan mode. All tools are now available.\n\n\
                 Plan summary ({len} characters):\n{body}\n\n\
                 Press Enter to execute this plan automatically, or type to modify."
            )
        };

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn exit_clears_flag_without_plan() {
        let flag: PlanModeFlag = Arc::new(RwLock::new(true));
        let tool = ExitPlanModeTool::new(Arc::clone(&flag));

        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.success);
        assert!(!*flag.read());
        assert!(result.output.contains("No plan content"));
    }

    #[tokio::test]
    async fn exit_includes_plan_summary() {
        let flag: PlanModeFlag = Arc::new(RwLock::new(true));
        let pending = new_pending_plan();
        let tool = ExitPlanModeTool::new_with_pending_plan(flag, Arc::clone(&pending));

        let result = tool
            .execute(json!({ "plan_content": "Step 1: do thing" }))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("Step 1: do thing"));
        assert!(result.output.contains("16 characters"));
        // Plan should be stored in pending
        assert!(pending.read().is_some());
        assert!(pending.read().as_ref().unwrap().contains("Step 1"));
    }

    #[test]
    fn spec_has_optional_plan_content() {
        let flag: PlanModeFlag = Arc::new(RwLock::new(false));
        let tool = ExitPlanModeTool::new(flag);
        let spec = tool.spec();
        assert_eq!(spec.name, "exit_plan_mode");
        assert_eq!(
            spec.parameters["properties"]["plan_content"]["type"],
            "string"
        );
    }
}
