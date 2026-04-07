// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;

/// Shared plan-mode state. When `true`, the agent is in plan-only mode.
pub type PlanModeFlag = Arc<RwLock<bool>>;

pub struct EnterPlanModeTool {
    flag: PlanModeFlag,
}

impl EnterPlanModeTool {
    pub fn new(flag: PlanModeFlag) -> Self {
        Self { flag }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "enter_plan_mode"
    }

    fn description(&self) -> &str {
        "Enter plan mode to create a detailed plan before making changes. In plan mode, only read-only tools are available."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": [],
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        *self.flag.write() = true;
        Ok(ToolResult {
            success: true,
            output: "Entered plan mode. Only read-only tools are now available.".to_string(),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use serde_json::json;

    #[tokio::test]
    async fn enter_sets_flag_and_returns_message() {
        let flag: PlanModeFlag = Arc::new(RwLock::new(false));
        let tool = EnterPlanModeTool::new(Arc::clone(&flag));

        assert!(!*flag.read());

        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.success);
        assert_eq!(
            result.output,
            "Entered plan mode. Only read-only tools are now available."
        );
        assert!(result.error.is_none());
        assert!(*flag.read());
    }

    #[test]
    fn spec_matches_tool() {
        let flag: PlanModeFlag = Arc::new(RwLock::new(false));
        let tool = EnterPlanModeTool::new(flag);
        let spec = tool.spec();
        assert_eq!(spec.name, "enter_plan_mode");
        assert!(spec.description.contains("plan mode"));
        assert_eq!(spec.parameters["type"], "object");
    }
}
