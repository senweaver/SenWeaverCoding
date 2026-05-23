// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;

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
